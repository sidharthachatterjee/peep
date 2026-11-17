//! Every token and component of the design language, in every state.
//!
//! ```sh
//! cargo run --example gallery            # follows the system appearance
//! cargo run --example gallery -- dark    # or `light`
//! ```

use gpui::{
    Animation, AnimationExt, App, Application, Context, Div, Hsla, IntoElement, Render,
    SharedString, Window, div, prelude::*,
};
use peep::{
    theme::{self, ActiveTheme, Mode, Theme},
    ui::{
        self, Avatar, Badge, Button, ButtonVariant, Checkbox, Color, ControlSize, Divider,
        Elevation, Label, List, ListItem, MessageRow, Preview, Root, SegmentedControl, Surface,
        Switch, TextSize,
    },
};

const MODES: [Mode; 3] = [Mode::System, Mode::Light, Mode::Dark];

const CONVERSATIONS: [(&str, &str, Option<&str>); 4] = [
    (
        "Ada Lovelace",
        "The engine is ready for its first run.",
        Some("2"),
    ),
    ("Design", "Grace: shared a new mockup", None),
    (
        "Alan Turing",
        "Can you look at the proof before Friday?",
        Some("12"),
    ),
    ("Katherine Johnson", "Trajectory numbers attached.", None),
];

fn main() {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("light") => Mode::Light,
        Some("dark") => Mode::Dark,
        _ => Mode::System,
    };

    Application::new().run(move |cx| {
        ui::init(cx);
        theme::set_mode(mode, cx);

        let opened = cx.open_window(ui::window_options("Peep gallery", cx), |window, cx| {
            let gallery = cx.new(|_| Gallery {
                muted: false,
                notifications: true,
                conversation: Some(0),
            });
            cx.new(|cx| Root::new(gallery, window, cx))
        });
        if let Err(error) = opened {
            eprintln!("The gallery could not open its window: {error:#}");
        }
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}

struct Gallery {
    muted: bool,
    notifications: bool,
    conversation: Option<usize>,
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mode = MODES
            .iter()
            .position(|mode| *mode == theme::mode(cx))
            .unwrap_or_default();

        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .px(theme.spacing.xl)
            .pb(theme.spacing.md)
            .child(
                Label::new("Peep design language")
                    .size(TextSize::Large)
                    .emphasized(),
            )
            .child(
                SegmentedControl::new("mode", mode)
                    .segment("System")
                    .segment("Light")
                    .segment("Dark")
                    .on_change(|ix, _, cx| theme::set_mode(MODES[*ix], cx)),
            );

        let sections = div()
            .flex()
            .flex_col()
            .gap(theme.spacing.xxl)
            .p(theme.spacing.xl)
            .child(colours(&theme))
            .child(typography(&theme, cx))
            .child(spacing_and_radius(&theme))
            .child(elevation(&theme))
            .child(motion(&theme))
            .child(buttons(&theme))
            .child(self.toggles(&theme, cx))
            .child(segmented_controls(&theme))
            .child(self.lists(&theme, cx))
            .child(messages(&theme))
            .child(avatars_and_badges(&theme));

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(Divider::horizontal())
            .child(
                div()
                    .id("sections")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(sections),
            )
    }
}

impl Gallery {
    fn toggles(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let checkbox_states = |checked: bool, name: &'static str| {
            states(theme, |ix, preview, disabled| {
                let checkbox = Checkbox::new((name, ix), checked).disabled(disabled);
                match preview {
                    Some(preview) => checkbox.preview(preview),
                    None => checkbox,
                }
            })
        };
        let switch_states = |on: bool, name: &'static str| {
            states(theme, |ix, preview, disabled| {
                let switch = Switch::new((name, ix), on).disabled(disabled);
                match preview {
                    Some(preview) => switch.preview(preview),
                    None => switch,
                }
            })
        };

        section("Checkbox and switch", theme)
            .child(
                group("Try them", theme).child(
                    row(theme)
                        .child(
                            Checkbox::new("try-checkbox", self.muted)
                                .label("Mute this conversation")
                                .on_change(cx.listener(|this, muted: &bool, _, cx| {
                                    this.muted = *muted;
                                    cx.notify();
                                })),
                        )
                        .child(Switch::new("try-switch", self.notifications).on_change(
                            cx.listener(|this, on: &bool, _, cx| {
                                this.notifications = *on;
                                cx.notify();
                            }),
                        ))
                        .child(Label::new("Notifications")),
                ),
            )
            .child(group("Checkbox, unchecked", theme).child(checkbox_states(false, "unchecked")))
            .child(group("Checkbox, checked", theme).child(checkbox_states(true, "checked")))
            .child(group("Switch, off", theme).child(switch_states(false, "off")))
            .child(group("Switch, on", theme).child(switch_states(true, "on")))
    }

    fn lists(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let rows = CONVERSATIONS
            .iter()
            .enumerate()
            .map(|(ix, (name, preview, unread))| {
                ListItem::new(ix)
                    .selected(self.conversation == Some(ix))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.conversation = Some(ix);
                        cx.notify();
                    }))
                    .child(Avatar::new(*name))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .overflow_hidden()
                            .child(Label::new(*name).emphasized().truncate())
                            .child(
                                Label::new(*preview)
                                    .size(TextSize::Small)
                                    .color(Color::Muted)
                                    .truncate(),
                            ),
                    )
                    .children(unread.map(Badge::new))
            });
        let conversations = List::new("conversations", CONVERSATIONS.len())
            .selected(self.conversation)
            .on_select(cx.listener(|this, ix: &usize, _, cx| {
                this.conversation = Some(*ix);
                cx.notify();
            }))
            .children(rows);

        let item = |id: &'static str, color: Color| {
            ListItem::new(id).child(Label::new("Conversation").color(color))
        };
        let row_states = row(theme)
            .child(captioned("Rest", item("row-rest", Color::Default), theme))
            .child(captioned(
                "Hover",
                item("row-hover", Color::Default).preview(Preview::Hover),
                theme,
            ))
            .child(captioned(
                "Pressed",
                item("row-pressed", Color::Default).preview(Preview::Pressed),
                theme,
            ))
            .child(captioned(
                "Selected",
                item("row-selected", Color::Default).selected(true),
                theme,
            ))
            .child(captioned(
                "Disabled",
                item("row-disabled", Color::Disabled).disabled(true),
                theme,
            ));
        let focused_list = List::new("list-focused", 2)
            .selected(Some(0))
            .preview(Preview::Focused)
            .child(item("list-focused-first", Color::Default).selected(true))
            .child(item("list-focused-second", Color::Default));

        section("List", theme)
            .child(
                group(
                    "Try it: click a row, or Tab to it and use the arrow keys",
                    theme,
                )
                .child(
                    div().flex().child(
                        Surface::new(Elevation::Flat).child(
                            div()
                                .w(theme.size.window_min.width / 2.)
                                .p(theme.spacing.sm)
                                .child(conversations),
                        ),
                    ),
                ),
            )
            .child(group("Row", theme).child(row_states))
            .child(group("List with keyboard focus", theme).child(div().flex().child(focused_list)))
    }
}

fn colours(theme: &Theme) -> Div {
    let c = &theme.colors;
    let groups: [(&str, Vec<(&str, Hsla)>); 8] = [
        (
            "Surfaces",
            vec![
                ("background", c.background),
                ("surface", c.surface),
                ("surface_raised", c.surface_raised),
                ("scrim", c.scrim),
            ],
        ),
        (
            "Borders",
            vec![
                ("border", c.border),
                ("border_control", c.border_control),
                ("focus_ring", c.focus_ring),
            ],
        ),
        (
            "Text",
            vec![
                ("text", c.text),
                ("text_muted", c.text_muted),
                ("text_disabled", c.text_disabled),
                ("text_accent", c.text_accent),
                ("text_on_accent", c.text_on_accent),
                ("text_on_danger", c.text_on_danger),
                ("text_danger", c.text_danger),
            ],
        ),
        (
            "Element",
            vec![
                ("element", c.element),
                ("element_hover", c.element_hover),
                ("element_active", c.element_active),
                ("element_disabled", c.element_disabled),
            ],
        ),
        (
            "Ghost",
            vec![
                ("ghost_hover", c.ghost_hover),
                ("ghost_active", c.ghost_active),
                ("ghost_selected", c.ghost_selected),
            ],
        ),
        (
            "Accent",
            vec![
                ("accent", c.accent),
                ("accent_hover", c.accent_hover),
                ("accent_active", c.accent_active),
            ],
        ),
        (
            "Danger",
            vec![
                ("danger", c.danger),
                ("danger_hover", c.danger_hover),
                ("danger_active", c.danger_active),
            ],
        ),
        (
            "Status and controls",
            vec![
                ("success", c.success),
                ("warning", c.warning),
                ("switch_off", c.switch_off),
                ("switch_thumb", c.switch_thumb),
                ("segment_selected", c.segment_selected),
            ],
        ),
    ];

    groups
        .into_iter()
        .fold(section("Colour", theme), |section, (name, swatches)| {
            let swatches = swatches.into_iter().map(|(name, colour)| {
                let swatch = div()
                    .w(theme.spacing.xxl * 2.)
                    .h(theme.spacing.xxl)
                    .rounded(theme.radius.md)
                    .border(theme.size.border)
                    .border_color(c.border)
                    .bg(colour);
                captioned(name, swatch, theme)
            });
            section.child(group(name, theme).child(row(theme).children(swatches)))
        })
}

fn typography(theme: &Theme, cx: &App) -> Div {
    let steps = [
        ("display", TextSize::Display),
        ("title", TextSize::Title),
        ("large", TextSize::Large),
        ("body", TextSize::Body),
        ("small", TextSize::Small),
        ("caption", TextSize::Caption),
    ];
    let column = steps.into_iter().map(|(name, size)| {
        let token = size.token(cx);
        captioned(
            format!(
                "{name} · {}/{}",
                f32::from(token.size),
                f32::from(token.line_height)
            ),
            Label::new("Sixty zippers were quickly picked from the woven jute bag").size(size),
            theme,
        )
    });

    section("Type", theme)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(theme.spacing.md)
                .children(column),
        )
        .child(
            group("Emphasis and colour", theme).child(
                row(theme)
                    .child(Label::new("Emphasized").emphasized())
                    .child(Label::new("Muted").color(Color::Muted))
                    .child(Label::new("Disabled").color(Color::Disabled))
                    .child(Label::new("Accent").color(Color::Accent))
                    .child(Label::new("Danger").color(Color::Danger)),
            ),
        )
}

fn spacing_and_radius(theme: &Theme) -> Div {
    let s = theme.spacing;
    let r = theme.radius;
    let spacing = [
        ("xxs", s.xxs),
        ("xs", s.xs),
        ("sm", s.sm),
        ("md", s.md),
        ("lg", s.lg),
        ("xl", s.xl),
        ("xxl", s.xxl),
    ]
    .map(|(name, width)| {
        let bar = div()
            .w(width)
            .h(theme.size.checkbox)
            .bg(theme.colors.accent);
        captioned(format!("{name} · {}", f32::from(width)), bar, theme)
    });
    let radius = [
        ("xs", r.xs),
        ("sm", r.sm),
        ("md", r.md),
        ("lg", r.lg),
        ("xl", r.xl),
        ("full", r.full),
    ]
    .map(|(name, radius)| {
        let corner = div()
            .size(theme.size.avatar_lg)
            .rounded(radius)
            .border(theme.size.border)
            .border_color(theme.colors.border_control)
            .bg(theme.colors.element);
        captioned(name, corner, theme)
    });

    section("Spacing and radius", theme)
        .child(group("Spacing", theme).child(row(theme).items_end().children(spacing)))
        .child(group("Radius", theme).child(row(theme).children(radius)))
}

fn elevation(theme: &Theme) -> Div {
    let surfaces = [
        ("Flat", Elevation::Flat),
        ("Raised", Elevation::Raised),
        ("Overlay", Elevation::Overlay),
        ("Modal", Elevation::Modal),
    ]
    .map(|(name, elevation)| {
        Surface::new(elevation).child(
            div()
                .flex()
                .flex_col()
                .gap(theme.spacing.xs)
                .p(theme.spacing.lg)
                .child(Label::new(name).emphasized())
                .child(
                    Label::new("Surface")
                        .size(TextSize::Small)
                        .color(Color::Muted),
                ),
        )
    });

    section("Elevation", theme).child(row(theme).gap(theme.spacing.xl).children(surfaces))
}

fn motion(theme: &Theme) -> Div {
    let m = theme.motion;
    let travel = theme.spacing.xxl * 2.;
    let durations = [("fast", m.fast), ("normal", m.normal), ("slow", m.slow)]
        .into_iter()
        .enumerate()
        .map(|(ix, (name, duration))| {
            let dot = div()
                .size(theme.size.checkbox)
                .rounded(theme.radius.full)
                .bg(theme.colors.accent)
                .with_animation(
                    ("motion", ix),
                    Animation::new(duration * 6).repeat().with_easing(move |t| {
                        // Travel for one duration, then rest, so the speeds can be compared.
                        (m.easing)((t * 6.).min(1.))
                    }),
                    move |dot, progress| dot.ml(travel * progress),
                );
            let track = div()
                .w(travel + theme.size.checkbox)
                .rounded(theme.radius.full)
                .bg(theme.colors.element)
                .child(dot);
            captioned(
                format!("{name} · {} ms", duration.as_millis()),
                track,
                theme,
            )
        });

    section("Motion", theme).child(row(theme).gap(theme.spacing.xl).children(durations))
}

fn buttons(theme: &Theme) -> Div {
    let variants = [
        ("Primary", ButtonVariant::Primary),
        ("Secondary", ButtonVariant::Secondary),
        ("Ghost", ButtonVariant::Ghost),
        ("Danger", ButtonVariant::Danger),
    ];
    let sizes = [
        ("Small", ControlSize::Small),
        ("Medium", ControlSize::Medium),
        ("Large", ControlSize::Large),
    ]
    .into_iter()
    .enumerate()
    .map(|(ix, (name, size))| {
        captioned(
            name,
            Button::new(("size", ix), "Send message")
                .variant(ButtonVariant::Primary)
                .size(size),
            theme,
        )
    });

    variants
        .into_iter()
        .fold(section("Button", theme), |section, (name, variant)| {
            section.child(
                group(name, theme).child(states(theme, |ix, preview, disabled| {
                    let button = Button::new((name, ix), "Send message")
                        .variant(variant)
                        .disabled(disabled);
                    match preview {
                        Some(preview) => button.preview(preview),
                        None => button,
                    }
                })),
            )
        })
        .child(group("Sizes", theme).child(row(theme).items_end().children(sizes)))
}

fn segmented_controls(theme: &Theme) -> Div {
    let control = |id: &'static str| {
        SegmentedControl::new(id, 1)
            .segment("All")
            .segment("Unread")
            .segment("Mentions")
    };

    section("Segmented control", theme).child(
        row(theme)
            .child(captioned("Rest", control("segments"), theme))
            .child(captioned(
                "Focused",
                control("segments-focused").preview(Preview::Focused),
                theme,
            )),
    )
}

fn avatars_and_badges(theme: &Theme) -> Div {
    let avatars = [
        ("Small", ControlSize::Small),
        ("Medium", ControlSize::Medium),
        ("Large", ControlSize::Large),
    ]
    .map(|(name, size)| captioned(name, Avatar::new("Ada Lovelace").size(size), theme));
    let badges = ["2", "12", "99+"].map(Badge::new);

    section("Avatar and badge", theme)
        .child(group("Avatar", theme).child(row(theme).items_end().children(avatars)))
        .child(group("Badge", theme).child(row(theme).gap(theme.spacing.sm).children(badges)))
}

fn messages(theme: &Theme) -> Div {
    let conversation = Surface::new(Elevation::Flat)
        .child(
            MessageRow::new("Ada Lovelace", "09:12")
                .day("Yesterday")
                .text("The engine is ready for its first run."),
        )
        .child(
            MessageRow::new("Ada Lovelace", "09:12")
                .continues(true)
                .text("I have punched the cards for the Bernoulli numbers. If they come out right we should write the method up, with a table of every step so anyone can follow it.")
                .edited(true),
        )
        .child(MessageRow::notice("Grace Hopper joined"))
        .child(
            MessageRow::new("You", "09:20")
                .day("Today")
                .text("Sending the notes now.")
                .attachment("Notes on the engine.pdf"),
        )
        .child(MessageRow::new("You", "09:21").continues(true).deleted())
        .child(MessageRow::new("Grace Hopper", "09:30").attachment("Video call"))
        .child(MessageRow::new("Grace Hopper", "09:31").continues(true))
        .child(div().h(theme.spacing.md));

    section("Messages", theme).child(div().max_w(theme.size.window_min.width).child(conversation))
}

/// One control drawn in each state. `build` receives an index for the control's id,
/// the state to draw it in, and whether it is disabled.
fn states<E: IntoElement>(theme: &Theme, build: impl Fn(usize, Option<Preview>, bool) -> E) -> Div {
    row(theme)
        .items_end()
        .child(captioned("Rest", build(0, None, false), theme))
        .child(captioned(
            "Hover",
            build(1, Some(Preview::Hover), false),
            theme,
        ))
        .child(captioned(
            "Pressed",
            build(2, Some(Preview::Pressed), false),
            theme,
        ))
        .child(captioned(
            "Focused",
            build(3, Some(Preview::Focused), false),
            theme,
        ))
        .child(captioned("Disabled", build(4, None, true), theme))
}

fn section(title: &'static str, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(theme.spacing.lg)
        .child(Label::new(title).size(TextSize::Title))
}

fn group(name: &'static str, theme: &Theme) -> Div {
    div().flex().flex_col().gap(theme.spacing.sm).child(
        Label::new(name)
            .size(TextSize::Small)
            .color(Color::Muted)
            .emphasized(),
    )
}

fn row(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(theme.spacing.lg)
}

fn captioned(caption: impl Into<SharedString>, element: impl IntoElement, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(theme.spacing.xs)
        .child(element)
        .child(
            Label::new(caption)
                .size(TextSize::Caption)
                .color(Color::Muted),
        )
}
