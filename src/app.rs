//! The application: its menu, its key bindings and its main window.

mod conversations;
mod messages;

use gpui::{
    App, Application, Context, Entity, Global, IntoElement, KeyBinding, Menu, MenuItem, Render,
    Window, actions, div, prelude::*,
};

use self::{conversations::Conversations, messages::Messages};
use crate::{
    account::{Account, Status},
    theme::ActiveTheme,
    ui::{self, Button, ButtonVariant, Color, Divider, Label, Root, TextSize},
};

/// The account lives as long as the app, so it outlasts the window.
struct SignedIn(Entity<Account>);

impl Global for SignedIn {}

actions!(peep, [Quit, Hide, Minimize, CloseWindow]);

/// Runs Peep until the person quits it.
pub fn run() {
    let app = Application::new();
    // Closing the window leaves Peep running; clicking its Dock icon brings the window back.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_window(cx);
        }
    });
    app.run(|cx| {
        init(cx);
        let account = cx.new(Account::new);
        cx.set_global(SignedIn(account));
        open_window(cx);
        cx.activate(true);
    });
}

fn init(cx: &mut App) {
    ui::init(cx);

    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &Minimize, cx| with_active_window(cx, |window| window.minimize_window()));
    cx.on_action(|_: &CloseWindow, cx| with_active_window(cx, |window| window.remove_window()));
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
    ]);
    cx.set_menus(vec![
        Menu {
            name: "Peep".into(),
            items: vec![
                MenuItem::action("Hide Peep", Hide),
                MenuItem::separator(),
                MenuItem::action("Quit Peep", Quit),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Close Window", CloseWindow),
            ],
        },
    ]);
}

fn open_window(cx: &mut App) {
    let account = cx.global::<SignedIn>().0.clone();
    let opened = cx.open_window(ui::window_options("Peep", cx), |window, cx| {
        let main = cx.new(|cx| Main::new(account, window, cx));
        cx.new(|cx| Root::new(main, window, cx))
    });
    if let Err(error) = opened {
        eprintln!("Peep could not open its window: {error:#}");
        cx.quit();
    }
}

/// Runs `action` on the frontmost window once the current event has been handled.
/// An app-wide action arrives while that window is being updated, so it cannot be
/// updated again until then.
fn with_active_window(cx: &mut App, action: impl FnOnce(&mut Window) + 'static) {
    cx.defer(move |cx| {
        if let Some(window) = cx.active_window()
            && let Err(error) = window.update(cx, |_, window, _| action(window))
        {
            eprintln!("Peep could not reach its window: {error:#}");
        }
    });
}

/// The content of the main window.
struct Main {
    account: Entity<Account>,
    conversations: Entity<Conversations>,
    messages: Entity<Messages>,
}

impl Main {
    fn new(account: Entity<Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let conversations = cx.new(|cx| Conversations::new(account.clone(), cx));
        let messages = cx.new(|cx| Messages::new(account.clone(), window, cx));
        cx.observe(&account, |_, _, cx| cx.notify()).detach();
        cx.observe_in(&conversations, window, |this, conversations, window, cx| {
            let selected = conversations.read(cx).selected_id();
            this.messages
                .update(cx, |messages, cx| messages.show(selected, window, cx));
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.account
                    .update(cx, |account, cx| account.refresh_if_stale(cx));
            }
        })
        .detach();
        Self {
            account,
            conversations,
            messages,
        }
    }
}

impl Render for Main {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let centered = || {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(theme.spacing.sm)
        };
        let note = |text: &'static str| Label::new(text).color(Color::Muted);

        match self.account.read(cx).status() {
            Status::Starting => centered(),
            Status::Loading => centered().child(note("Loading your messages…")),
            Status::SigningIn => {
                centered().child(note("Finish signing in in the Chrome window that opened."))
            }
            Status::SignedOut => centered()
                .child(Label::new("Peep").size(TextSize::Title))
                .child(note("Sign in to see your Google Chat direct messages."))
                .child(
                    div().pt(theme.spacing.sm).child(
                        Button::new("sign-in", "Sign in with Google")
                            .variant(ButtonVariant::Primary)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.account.update(cx, |account, cx| account.sign_in(cx));
                            })),
                    ),
                ),
            Status::Failed(message) => centered()
                .child(Label::new(message.clone()).color(Color::Danger))
                .child(div().pt(theme.spacing.sm).child(
                    Button::new("retry", "Try again").on_click(cx.listener(|this, _, _, cx| {
                        this.account.update(cx, |account, cx| account.retry(cx));
                    })),
                )),
            Status::Ready => div()
                .size_full()
                .flex()
                .child(self.conversations.clone())
                .child(Divider::vertical())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.messages.clone()),
                ),
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn it_opens_one_window_and_cmd_w_closes_it(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init(cx);
            let account = cx.new(|_| Account::with(Status::SignedOut, Vec::new()));
            cx.set_global(SignedIn(account));
            open_window(cx);
        });
        let windows = cx.update(|cx| cx.windows());
        assert_eq!(windows.len(), 1);

        cx.update_window(windows[0], |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(windows[0], "cmd-w");

        assert!(cx.update(|cx| cx.windows()).is_empty());
    }
}
