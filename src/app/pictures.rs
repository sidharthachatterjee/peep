//! Pictures of Peep's window, drawn without a window: nothing shows on the screen and
//! nothing takes focus. Each is compared with the one kept in `tests/pictures`.
//!
//! `PEEP_PICTURES=keep cargo test pictures` keeps what is drawn now as what to expect.
//! What was drawn is always left in `target/pictures` to look at.

use std::{path::PathBuf, rc::Rc, sync::Arc, time::Duration};

use gpui::{AnyWindowHandle, HeadlessAppContext};

use super::*;
use crate::{
    chat::{Availability, Status as PersonStatus, fake::Fake},
    theme::{self, Mode},
};

/// How far a colour channel may be from what is expected around it before a pixel counts
/// as different: blending rounds a little differently between graphics cards.
const CHANNEL_TOLERANCE: u8 = 8;
/// Pixels per point, as on a Retina display.
const SCALE: f32 = 2.;

/// Peep drawn without a window.
struct Drawn {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
}

impl Drawn {
    /// Starts Peep signed in to `service`, or signed out without one.
    fn start(mode: Mode, service: Option<&Fake>) -> Self {
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(()),
            gpui_platform::current_headless_renderer,
        );
        let service = service.cloned();
        let window = cx.update(|cx| {
            init(cx);
            theme::set_mode(mode, cx);
            let account = cx.new(|cx| match service {
                Some(service) => Account::over(Rc::new(service), cx),
                None => Account::with(Status::SignedOut, Vec::new()),
            });
            launch(account, true, cx);
            cx.windows()[0]
        });
        cx.simulate_window_scale_factor_change(window, SCALE);
        let mut this = Self { cx, window };
        this.update(|window, _| window.activate_window());
        this
    }

    fn update(&mut self, change: impl FnOnce(&mut Window, &mut App)) {
        self.cx
            .update_window(self.window, |_, window, cx| change(window, cx))
            .expect("the window is open");
        self.settle();
    }

    /// Opens the conversation with this id.
    fn open(&mut self, id: &str) {
        self.update(|_, cx| carry_out(Command::OpenConversation(id.to_owned()), cx));
    }

    /// Lets everything Peep was waiting to do happen.
    fn settle(&mut self) {
        self.cx.run_until_parked();
        self.cx.advance_clock(Duration::from_secs(1));
        self.cx.run_until_parked();
    }

    /// Draws the window and checks it against the picture kept under `name`.
    #[track_caller]
    fn looks_like(&mut self, name: &str) {
        self.settle();
        let drawn = self
            .cx
            .update_window(self.window, |_, window, cx| {
                window.draw(cx).clear(cx);
                window.render_to_image()
            })
            .expect("the window is open")
            .expect("the window draws to a picture");

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let latest = root.join("target/pictures").join(format!("{name}.png"));
        std::fs::create_dir_all(latest.parent().unwrap()).unwrap();
        drawn.save(&latest).unwrap();

        let kept = root.join("tests/pictures").join(format!("{name}.png"));
        if std::env::var("PEEP_PICTURES").as_deref() == Ok("keep") {
            std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
            drawn.save(&kept).unwrap();
            return;
        }
        let expected = image::open(&kept)
            .unwrap_or_else(|error| {
                panic!("no picture is kept for {name:?} ({error}); see the top of this file")
            })
            .into_rgba8();
        assert_eq!(
            drawn.dimensions(),
            expected.dimensions(),
            "{name:?} changed size; compare {latest:?} with {kept:?}"
        );
        let different = different_pixels(&drawn, &expected);
        assert!(
            different == 0,
            "{name:?} looks different in {different} pixels; compare {latest:?} with {kept:?}"
        );
    }
}

/// How many pixels of `drawn` are not what `expected` has there. The edges of letters come
/// out a shade lighter or darker from one Mac to the next, so a pixel may be anything
/// between its expected neighbours; a letter that changed or moved still shows.
fn different_pixels(drawn: &image::RgbaImage, expected: &image::RgbaImage) -> usize {
    let (width, height) = expected.dimensions();
    let near = |drawn: u8, low: u8, high: u8| {
        (low.saturating_sub(CHANNEL_TOLERANCE)..=high.saturating_add(CHANNEL_TOLERANCE))
            .contains(&drawn)
    };
    drawn
        .enumerate_pixels()
        .filter(|(x, y, drawn)| {
            let around = |channel: usize| {
                let xs = x.saturating_sub(1)..=(x + 1).min(width - 1);
                let ys = y.saturating_sub(1)..=(y + 1).min(height - 1);
                let values = ys.flat_map(|y| xs.clone().map(move |x| (x, y)));
                values.fold((u8::MAX, u8::MIN), |(low, high), (x, y)| {
                    let value = expected.get_pixel(x, y).0[channel];
                    (low.min(value), high.max(value))
                })
            };
            let there = expected.get_pixel(*x, *y).0;
            (0..4).any(|channel| {
                let value = drawn.0[channel];
                // Most pixels are simply the same, which is quick to see.
                !near(value, there[channel], there[channel]) && {
                    let (low, high) = around(channel);
                    !near(value, low, high)
                }
            })
        })
        .count()
}

/// A set afternoon on the Mac's wall clock, in microseconds since the Unix epoch, so the
/// times in a picture read the same whenever and wherever it is drawn.
fn an_afternoon() -> i64 {
    // SAFETY: `tm` is plain data that `mktime` reads and tidies; the pointer is valid for
    // the call and nothing keeps it afterwards.
    let seconds = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        (tm.tm_year, tm.tm_mon, tm.tm_mday) = (2025 - 1900, 2, 14);
        (tm.tm_hour, tm.tm_min, tm.tm_isdst) = (15, 9, -1);
        libc::mktime(&mut tm)
    };
    seconds * 1_000_000
}

fn doing(availability: Availability, quiet: bool, note: &str) -> PersonStatus {
    PersonStatus {
        availability,
        quiet,
        note: note.into(),
    }
}

/// A handful of people in every state the list can show.
fn people() -> Fake {
    Fake::new()
        .around(an_afternoon())
        .conversation(
            "Ada Lovelace",
            &[
                ("ada lovelace", "Did the cards arrive?"),
                ("me", "This morning. All forty of them, in order."),
                ("me", "I will run the first set after lunch."),
                (
                    "ada lovelace",
                    "Wonderful. Mind the ninth: its third column was punched twice, so it may \
                     need doing again before the engine will take it.",
                ),
                ("ada lovelace", "Tell me how it goes."),
            ],
            2,
            3,
        )
        .conversation(
            "Grace Hopper",
            &[("grace hopper", "Found it. It was a moth.")],
            40,
            0,
        )
        .conversation(
            "Alan Turing",
            &[("me", "Friday works."), ("alan turing", "See you then.")],
            190,
            2,
        )
        .conversation("Katherine Johnson", &[("me", "Numbers attached.")], 1500, 1)
        .conversation("Margaret Hamilton", &[("me", "Thank you!")], 4400, 1)
        .status("ada lovelace", doing(Availability::Active, false, ""))
        .status("grace hopper", doing(Availability::Idle, false, "At lunch"))
        .status("alan turing", doing(Availability::Active, true, ""))
        .status("katherine johnson", doing(Availability::Away, false, ""))
}

#[test]
fn signed_out() {
    Drawn::start(Mode::Light, None).looks_like("signed-out");
}

#[test]
fn the_list_with_nothing_open() {
    Drawn::start(Mode::Light, Some(&people())).looks_like("list");
}

#[test]
fn a_conversation_in_light_and_dark() {
    for (mode, name) in [
        (Mode::Light, "conversation-light"),
        (Mode::Dark, "conversation-dark"),
    ] {
        let mut drawn = Drawn::start(mode, Some(&people()));
        drawn.open("dm/ada lovelace-dm");
        drawn.looks_like(name);
    }
}
