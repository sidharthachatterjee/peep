//! The application: its menu, its key bindings and its main window.

mod composer;
mod conversations;
#[cfg(test)]
mod end_to_end;
mod messages;
#[cfg(test)]
mod pictures;

use futures::{StreamExt as _, channel::mpsc};
use gpui::{
    App, Context, Entity, Global, IntoElement, KeyBinding, Menu, MenuItem, Render, WeakEntity,
    Window, actions, div, prelude::*,
};

use self::{
    conversations::{Conversations, Opened},
    messages::Messages,
};
use crate::{
    account::{Account, Status},
    chat::Showing,
    dock,
    menu_bar::{Choice, Command, Content, MenuBarItem},
    session,
    theme::ActiveTheme,
    time,
    ui::{self, Button, ButtonVariant, Color, Divider, Label, Root, TextSize},
};

/// The account lives as long as the app, so it outlasts the window.
struct SignedIn(Entity<Account>);

impl Global for SignedIn {}

/// Peep's icon in the menu bar, there for as long as Peep runs.
struct InMenuBar(MenuBarItem);

impl Global for InMenuBar {}

/// Whether notifications are paused. Peep has none yet, so for now this only shows.
#[derive(Default)]
struct NotificationsPaused(bool);

impl Global for NotificationsPaused {}

/// The content of the open window, if there is one.
#[derive(Default)]
struct OpenWindow(Option<WeakEntity<Main>>);

impl Global for OpenWindow {}

/// How many unread conversations the menu bar's menu lists.
const UNREAD_IN_MENU: usize = 5;

actions!(peep, [Quit, Hide, Minimize, CloseWindow]);

/// Started with this, Peep opens no window and stays behind other apps: it is in the menu bar
/// and the Dock until its window is asked for.
const IN_BACKGROUND: &str = "--background";

/// Runs Peep until the person quits it.
pub fn run() {
    let in_background = std::env::args().any(|argument| argument == IN_BACKGROUND);
    let app = gpui_platform::application();
    // Closing the window leaves Peep running; clicking its Dock icon brings the window back.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_window(cx);
        }
    });
    app.run(move |cx| {
        init(cx);
        let account = cx.new(Account::new);
        launch(account, !in_background, cx);
    });
}

/// Starts Peep for `account`: in the menu bar and the Dock, and with its window open in
/// front of other apps if `in_front`.
fn launch(account: Entity<Account>, in_front: bool, cx: &mut App) {
    cx.set_global(SignedIn(account.clone()));
    cx.set_global(NotificationsPaused::default());
    show_in_menu_bar(&account, cx);
    show_on_icon(&account, cx);
    // With its window closed Peep stays in the menu bar, and does only what that needs.
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            let account = cx.global::<SignedIn>().0.clone();
            account.update(cx, |account, cx| account.rest(cx));
        }
    })
    .detach();
    if in_front {
        open_window(cx);
        cx.activate(true);
    }
}

/// Keeps the count of what is unread on Peep's icon in the Dock and the app switcher.
fn show_on_icon(account: &Entity<Account>, cx: &mut App) {
    let mut shown = account.read(cx).unread();
    dock::show_unread(shown);
    cx.observe(account, move |account, cx| {
        let unread = account.read(cx).unread();
        if unread != shown {
            shown = unread;
            dock::show_unread(unread);
        }
    })
    .detach();
}

/// Puts Peep's icon in the menu bar, with the count of what is unread beside it and a menu
/// behind it.
fn show_in_menu_bar(account: &Entity<Account>, cx: &mut App) {
    let (commands, mut chosen) = mpsc::unbounded();
    let Some(item) = MenuBarItem::new(move |command| {
        // Nothing listens once Peep is quitting.
        commands.unbounded_send(command).ok();
    }) else {
        return;
    };
    cx.set_global(InMenuBar(item));
    update_menu_bar(cx);
    cx.observe(account, |_, cx| update_menu_bar(cx)).detach();
    cx.observe_global::<NotificationsPaused>(update_menu_bar)
        .detach();
    cx.spawn(async move |cx| {
        while let Some(command) = chosen.next().await {
            cx.update(|cx| carry_out(command, cx));
        }
    })
    .detach();
}

/// Shows the account as it is now in the menu bar.
fn update_menu_bar(cx: &mut App) {
    cx.global::<InMenuBar>().0.show(menu_bar_content(cx));
}

/// What the menu bar shows for the account as it is now.
fn menu_bar_content(cx: &App) -> Content {
    let account = cx.global::<SignedIn>().0.read(cx);
    Content {
        showing: account.showing(),
        paused: cx.global::<NotificationsPaused>().0,
        unread: account.unread(),
        conversations: account
            .unread_rows(UNREAD_IN_MENU)
            .map(|row| (row.id.to_string(), row.title.to_string()))
            .collect(),
    }
}

/// Does what was chosen from the menu bar's menu.
fn carry_out(command: Command, cx: &mut App) {
    match command {
        Command::Show(choice) => {
            let now = (session::now() * 1_000_000.) as i64;
            let showing = match choice {
                Choice::Automatic => Showing::Automatic,
                Choice::Away => Showing::Away,
                Choice::DoNotDisturbFor(time) => Showing::DoNotDisturb {
                    until: now + i64::try_from(time.as_micros()).unwrap_or(i64::MAX - now),
                },
                Choice::DoNotDisturbUntilTomorrow => Showing::DoNotDisturb {
                    until: time::tomorrow_morning(),
                },
            };
            let account = cx.global::<SignedIn>().0.clone();
            account.update(cx, |account, cx| account.show(showing, cx));
        }
        Command::Pause(paused) => cx.set_global(NotificationsPaused(paused)),
        Command::OpenConversation(id) => {
            come_forward(cx);
            let main = cx
                .global::<OpenWindow>()
                .0
                .as_ref()
                .and_then(WeakEntity::upgrade);
            if let Some(main) = main {
                let conversations = main.read(cx).conversations.clone();
                conversations.update(cx, |conversations, cx| conversations.select_id(&id, cx));
                // It was opened to be answered.
                let messages = main.read(cx).messages.clone();
                cx.defer(move |cx| {
                    let Some(window) = cx.windows().first().copied() else {
                        return;
                    };
                    window
                        .update(cx, |_, window, cx| {
                            messages.update(cx, |messages, cx| messages.write(window, cx));
                        })
                        .ok();
                });
            }
        }
        Command::Quit => cx.quit(),
    }
}

/// Brings Peep in front of other apps, with its window open.
fn come_forward(cx: &mut App) {
    cx.activate(true);
    match cx.windows().first() {
        Some(window) => {
            if let Err(error) = window.update(cx, |_, window, _| window.activate_window()) {
                eprintln!("Peep could not reach its window: {error:#}");
            }
        }
        None => open_window(cx),
    }
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
            disabled: false,
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Close Window", CloseWindow),
            ],
            disabled: false,
        },
    ]);
}

fn open_window(cx: &mut App) {
    let account = cx.global::<SignedIn>().0.clone();
    let opened = cx.open_window(ui::window_options("Peep", cx), |window, cx| {
        let main = cx.new(|cx| Main::new(account, window, cx));
        cx.set_global(OpenWindow(Some(main.downgrade())));
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
        cx.subscribe_in(&conversations, window, |this, _, _: &Opened, window, cx| {
            this.messages
                .update(cx, |messages, cx| messages.write(window, cx));
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            let active = window.is_window_active();
            this.account.update(cx, |account, cx| {
                account.set_frontmost(active, cx);
                if active {
                    account.refresh_if_stale(cx);
                }
            });
        })
        .detach();
        let active = window.is_window_active();
        account.update(cx, |account, cx| account.set_frontmost(active, cx));
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
