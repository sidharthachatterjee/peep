use std::rc::Rc;

use gpui::{
    App, ElementId, Hsla, IntoElement, MouseButton, PathBuilder, Pixels, RenderOnce, SharedString,
    Window, canvas, div, point, prelude::*, transparent_black,
};

use super::{
    actions::Confirm,
    style::{Preview, focus, focus_ring, text_style},
};
use crate::theme::{ActiveTheme, Colors};

type ChangeHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// The fill and border of the box.
fn paint(checked: bool, disabled: bool, preview: Option<Preview>, colors: &Colors) -> (Hsla, Hsla) {
    if disabled {
        return (colors.element_disabled, colors.border);
    }
    match (checked, preview) {
        (true, Some(Preview::Hover)) => (colors.accent_hover, transparent_black()),
        (true, Some(Preview::Pressed)) => (colors.accent_active, transparent_black()),
        (true, _) => (colors.accent, transparent_black()),
        (false, Some(Preview::Hover)) => (colors.element, colors.border_control),
        (false, Some(Preview::Pressed)) => (colors.element_hover, colors.border_control),
        (false, _) => (colors.surface_raised, colors.border_control),
    }
}

/// A box that is checked or not, with an optional label that toggles it too.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    preview: Option<Preview>,
    on_change: Option<ChangeHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            label: None,
            disabled: false,
            preview: None,
            on_change: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the new value when the person toggles the checkbox.
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn preview(mut self, state: Preview) -> Self {
        self.preview = Some(state);
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let interactive = !self.disabled && self.preview.is_none();
        let (focus_handle, focused) = focus(&self.id, interactive, self.preview, window, cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let checked = self.checked;
        let (fill, border) = paint(checked, self.disabled, self.preview, colors);
        let (hover, _) = paint(checked, false, Some(Preview::Hover), colors);
        let (pressed, _) = paint(checked, false, Some(Preview::Pressed), colors);
        let (mark, text) = if self.disabled {
            (colors.text_disabled, colors.text_disabled)
        } else {
            (colors.text_on_accent, colors.text)
        };
        let toggle = self.on_change.filter(|_| interactive).map(|on_change| {
            move |window: &mut Window, cx: &mut App| on_change(&!checked, window, cx)
        });

        let mark_box = div()
            .id("box")
            .flex_none()
            .size(theme.size.checkbox)
            .rounded(theme.radius.xs)
            .border(theme.size.border)
            .border_color(border)
            .bg(fill)
            .when(checked, |this| {
                this.child(check_mark(mark, theme.size.stroke))
            })
            .when(interactive, |this| {
                this.hover(|style| style.bg(hover))
                    .active(|style| style.bg(pressed))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_action(|_: &Confirm, _, _| {})
            })
            .when_some(toggle.clone(), |this, toggle| {
                this.on_click(move |_, window, cx| toggle(window, cx))
            })
            .when_some(focus_handle, |this, handle| this.track_focus(&handle))
            .when(focused, |this| {
                this.child(focus_ring(theme.radius.xs, theme))
            });

        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(theme.spacing.sm)
            .child(mark_box)
            .when_some(self.label, |this, label| {
                this.child(
                    text_style(div(), theme.text.body)
                        .id("label")
                        .text_color(text)
                        .when(interactive, |this| {
                            this.on_mouse_down(MouseButton::Left, |_, window, _| {
                                window.prevent_default()
                            })
                        })
                        .when_some(toggle, |this, toggle| {
                            this.on_click(move |_, window, cx| toggle(window, cx))
                        })
                        .child(label),
                )
            })
    }
}

fn check_mark(color: Hsla, stroke: Pixels) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let at = |x: f32, y: f32| {
                bounds.origin + point(bounds.size.width * x, bounds.size.height * y)
            };
            let mut path = PathBuilder::stroke(stroke);
            path.move_to(at(0.22, 0.52));
            path.line_to(at(0.42, 0.72));
            path.line_to(at(0.78, 0.28));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size_full()
}

/// A setting that is on or off and takes effect at once.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    disabled: bool,
    preview: Option<Preview>,
    on_change: Option<ChangeHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            disabled: false,
            preview: None,
            on_change: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the new value when the person flips the switch.
    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn preview(mut self, state: Preview) -> Self {
        self.preview = Some(state);
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let interactive = !self.disabled && self.preview.is_none();
        let (focus_handle, focused) = focus(&self.id, interactive, self.preview, window, cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let on = self.on;
        // The track carries the state by its fill alone; it has no outline.
        let track = |preview: Option<Preview>| match (on, preview) {
            (false, _) => colors.switch_off,
            (true, Some(Preview::Hover)) => colors.accent_hover,
            (true, Some(Preview::Pressed)) => colors.accent_active,
            (true, _) => colors.accent,
        };
        let (fill, thumb) = match (self.disabled, on) {
            (true, _) => (colors.element_active, colors.surface_raised),
            (false, true) => (track(self.preview), colors.text_on_accent),
            (false, false) => (track(self.preview), colors.switch_thumb),
        };
        let (hover, pressed) = (track(Some(Preview::Hover)), track(Some(Preview::Pressed)));

        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .w(theme.size.switch_width)
            .h(theme.size.switch_height)
            .px(theme.spacing.xxs)
            .rounded(theme.radius.full)
            .bg(fill)
            .when(on, |this| this.justify_end())
            .when(interactive, |this| {
                this.hover(|style| style.bg(hover))
                    .active(|style| style.bg(pressed))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_action(|_: &Confirm, _, _| {})
                    .when_some(self.on_change, |this, on_change| {
                        this.on_click(move |_, window, cx| on_change(&!on, window, cx))
                    })
            })
            .when_some(focus_handle, |this, handle| this.track_focus(&handle))
            .when(focused, |this| {
                this.child(focus_ring(theme.radius.full, theme))
            })
            .child(
                div()
                    .size(theme.size.switch_thumb)
                    .rounded(theme.radius.full)
                    .bg(thumb)
                    .when(!self.disabled, |thumb| {
                        thumb.shadow(theme.shadow.sm.clone())
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext};

    use super::*;
    use crate::ui::test::{click, focus_is_on_root, open, press};

    #[derive(Default)]
    struct Host {
        muted: bool,
        notify: bool,
        changes: usize,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .child(
                    div().debug_selector(|| "locked".into()).child(
                        Checkbox::new("locked", false)
                            .disabled(true)
                            .on_change(cx.listener(|this, _, _, _| this.changes += 100)),
                    ),
                )
                .child(
                    div().debug_selector(|| "muted".into()).child(
                        Checkbox::new("muted", self.muted)
                            .label("Mute")
                            .on_change(cx.listener(|this, muted: &bool, _, cx| {
                                this.muted = *muted;
                                this.changes += 1;
                                cx.notify();
                            })),
                    ),
                )
                .child(div().debug_selector(|| "notify".into()).child(
                    Switch::new("notify", self.notify).on_change(cx.listener(
                        |this, notify: &bool, _, cx| {
                            this.notify = *notify;
                            this.changes += 1;
                            cx.notify();
                        },
                    )),
                ))
        }
    }

    #[gpui::test]
    fn clicking_the_box_or_its_label_toggles_a_checkbox(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());
        let bounds = cx.debug_bounds("muted").unwrap();
        let half = bounds.size.height / 2.;

        cx.simulate_click(bounds.origin + point(half, half), Modifiers::none());
        assert!(
            host.read_with(cx, |host, _| host.muted),
            "the box checks it"
        );

        let label = bounds.origin + point(bounds.size.width - half, half);
        cx.simulate_click(label, Modifiers::none());
        assert!(
            !host.read_with(cx, |host, _| host.muted),
            "the label unchecks it"
        );

        assert_eq!(host.read_with(cx, |host, _| host.changes), 2);
        assert!(focus_is_on_root(cx));
    }

    #[gpui::test]
    fn a_click_flips_a_switch(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        click(cx, "notify");
        assert!(host.read_with(cx, |host, _| host.notify));

        click(cx, "notify");
        assert!(!host.read_with(cx, |host, _| host.notify));
    }

    #[gpui::test]
    fn space_and_enter_toggle_the_focused_control_once(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        press(cx, "tab space");
        assert_eq!(
            host.read_with(cx, |host, _| (host.muted, host.changes)),
            (true, 1),
            "Tab skipped the disabled checkbox and Space checked the next one"
        );

        press(cx, "tab enter");
        assert_eq!(
            host.read_with(cx, |host, _| (host.notify, host.changes)),
            (true, 2)
        );
    }

    #[gpui::test]
    fn disabled_ignores_clicks(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        click(cx, "locked");

        assert_eq!(host.read_with(cx, |host, _| host.changes), 0);
    }
}
