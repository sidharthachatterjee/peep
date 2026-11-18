use gpui::{
    AnyView, App, Bounds, Context, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*,
};

use super::{
    actions::{FocusNext, FocusPrevious},
    style::set_focus_visible,
};
use crate::theme::{self, ActiveTheme};

/// Options for a window whose root view is a [`Root`]: centred, at the theme's window size,
/// with the title bar drawn by the root so the whole window is one surface.
pub fn window_options(title: impl Into<SharedString>, cx: &App) -> WindowOptions {
    let size = cx.theme().size;
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size.window,
            cx,
        ))),
        window_min_size: Some(size.window_min),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(size.window_buttons),
        }),
        ..Default::default()
    }
}

/// The root of every window. It paints the window background and the title bar strip,
/// sets the base text style, moves keyboard focus on Tab and shows focus rings only after
/// it, takes focus back when the focused control goes away, and follows the system
/// appearance.
pub struct Root {
    view: AnyView,
    focus_handle: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl Root {
    pub fn new(view: impl Into<AnyView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            view: view.into(),
            _subscriptions: [
                cx.on_focus_lost(window, |this, window, cx| {
                    window.focus(&this.focus_handle, cx)
                }),
                cx.observe_window_appearance(window, |_, _, cx| {
                    theme::system_appearance_changed(cx)
                }),
            ],
            focus_handle,
        }
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        // The system drags the window by this strip; a double click zooms or minimizes it
        // as the system setting says.
        let titlebar = div()
            .id("titlebar")
            .flex_none()
            .h(theme.size.titlebar)
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            });

        div()
            .track_focus(&self.focus_handle)
            .on_action(|_: &FocusNext, window, cx| {
                set_focus_visible(true, cx);
                window.focus_next(cx);
            })
            .on_action(|_: &FocusPrevious, window, cx| {
                set_focus_visible(true, cx);
                window.focus_prev(cx);
            })
            .on_any_mouse_down(|_, _, cx| set_focus_visible(false, cx))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.colors.background)
            .text_color(theme.colors.text)
            .font_family(theme.text.family.clone())
            .text_size(theme.text.body.size)
            .line_height(theme.text.body.line_height)
            .child(titlebar)
            .child(div().flex_1().overflow_hidden().child(self.view.clone()))
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;
    use crate::ui::{
        Button,
        style::focus_visible,
        test::{click, focus_is_on_root, open, press},
    };

    struct Host {
        show_first: bool,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .when(self.show_first, |this| {
                    this.child(Button::new("first", "First").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.show_first = false;
                            cx.notify();
                        },
                    )))
                })
                .child(
                    div()
                        .debug_selector(|| "second".into())
                        .child(Button::new("second", "Second")),
                )
        }
    }

    #[gpui::test]
    fn tab_moves_focus_and_shows_rings_until_the_next_click(cx: &mut TestAppContext) {
        let (_, cx) = open(cx, |_, _| Host { show_first: true });
        assert!(focus_is_on_root(cx));
        assert!(!cx.update(|_, cx| focus_visible(cx)));

        press(cx, "tab");
        assert!(!focus_is_on_root(cx));
        assert!(cx.update(|_, cx| focus_visible(cx)));

        press(cx, "tab tab");
        assert!(!focus_is_on_root(cx), "Tab wraps around the controls");

        press(cx, "shift-tab");
        assert!(!focus_is_on_root(cx));

        click(cx, "second");
        assert!(!cx.update(|_, cx| focus_visible(cx)));
    }

    #[gpui::test]
    fn focus_returns_to_the_root_when_the_focused_control_goes_away(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host { show_first: true });

        press(cx, "tab enter");
        cx.run_until_parked();

        assert!(!host.read_with(cx, |host, _| host.show_first));
        assert!(focus_is_on_root(cx));

        press(cx, "tab");
        assert!(!focus_is_on_root(cx), "Tab still works afterwards");
    }

    #[gpui::test]
    fn the_theme_follows_the_chosen_mode(cx: &mut TestAppContext) {
        let (_, cx) = open(cx, |_, _| Host { show_first: true });

        cx.update(|_, cx| theme::set_mode(theme::Mode::Dark, cx));
        assert_eq!(
            cx.update(|_, cx| (theme::mode(cx), cx.theme().appearance)),
            (theme::Mode::Dark, theme::Appearance::Dark)
        );

        cx.update(|_, cx| theme::set_mode(theme::Mode::Light, cx));
        assert_eq!(
            cx.update(|_, cx| cx.theme().appearance),
            theme::Appearance::Light
        );
    }
}
