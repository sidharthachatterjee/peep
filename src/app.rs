//! The application: its menu, its key bindings and its main window.

use gpui::{
    App, Application, Context, IntoElement, KeyBinding, Menu, MenuItem, Render, Window, actions,
    div, prelude::*,
};

use crate::{
    theme::ActiveTheme,
    ui::{self, Color, Label, Root, TextSize},
};

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

    open_window(cx);
}

fn open_window(cx: &mut App) {
    let opened = cx.open_window(ui::window_options("Peep", cx), |window, cx| {
        let main = cx.new(|_| Main);
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
struct Main;

impl Render for Main {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(cx.theme().spacing.xs)
            .child(Label::new("Peep").size(TextSize::Title))
            .child(Label::new("Nothing to show yet.").color(Color::Muted))
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn it_opens_one_window_and_cmd_w_closes_it(cx: &mut TestAppContext) {
        cx.update(init);
        let windows = cx.update(|cx| cx.windows());
        assert_eq!(windows.len(), 1);

        cx.update_window(windows[0], |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(windows[0], "cmd-w");

        assert!(cx.update(|cx| cx.windows()).is_empty());
    }
}
