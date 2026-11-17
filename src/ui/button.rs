use gpui::{
    App, ClickEvent, ElementId, Hsla, IntoElement, MouseButton, RenderOnce, SharedString, Window,
    div, prelude::*, transparent_black,
};

use super::{
    actions::Confirm,
    style::{ControlSize, Preview, focus, focus_ring, text_style},
};
use crate::theme::{ActiveTheme, Colors};

/// How much a button stands out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The one action to notice in a view.
    Primary,
    /// An outlined button on the fill of its surface.
    #[default]
    Secondary,
    /// Shares the fill of its surface until used.
    Ghost,
    /// A destructive action.
    Danger,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A push button. Enter and Space press it while it has keyboard focus.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    size: ControlSize,
    disabled: bool,
    preview: Option<Preview>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::default(),
            size: ControlSize::default(),
            disabled: false,
            preview: None,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub fn preview(mut self, state: Preview) -> Self {
        self.preview = Some(state);
        self
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Rest,
    Hover,
    Pressed,
}

fn background(variant: ButtonVariant, phase: Phase, colors: &Colors) -> Hsla {
    match (variant, phase) {
        (ButtonVariant::Primary, Phase::Rest) => colors.accent,
        (ButtonVariant::Primary, Phase::Hover) => colors.accent_hover,
        (ButtonVariant::Primary, Phase::Pressed) => colors.accent_active,
        (ButtonVariant::Secondary, Phase::Rest) => colors.surface_raised,
        (ButtonVariant::Secondary, Phase::Hover) => colors.element,
        (ButtonVariant::Secondary, Phase::Pressed) => colors.element_hover,
        (ButtonVariant::Ghost, Phase::Rest) => transparent_black(),
        (ButtonVariant::Ghost, Phase::Hover) => colors.ghost_hover,
        (ButtonVariant::Ghost, Phase::Pressed) => colors.ghost_active,
        (ButtonVariant::Danger, Phase::Rest) => colors.danger,
        (ButtonVariant::Danger, Phase::Hover) => colors.danger_hover,
        (ButtonVariant::Danger, Phase::Pressed) => colors.danger_active,
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let interactive = !self.disabled && self.preview.is_none();
        let (focus_handle, focused) = focus(&self.id, interactive, self.preview, window, cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let variant = self.variant;

        let phase = match self.preview {
            Some(Preview::Hover) => Phase::Hover,
            Some(Preview::Pressed) => Phase::Pressed,
            Some(Preview::Focused) | None => Phase::Rest,
        };
        let (fill, text, border) = if self.disabled {
            let (fill, border) = match variant {
                ButtonVariant::Ghost => (transparent_black(), transparent_black()),
                ButtonVariant::Secondary => (colors.surface_raised, colors.border),
                _ => (colors.element, transparent_black()),
            };
            (fill, colors.text_disabled, border)
        } else {
            let text = match variant {
                ButtonVariant::Primary => colors.text_on_accent,
                ButtonVariant::Danger => colors.text_on_danger,
                ButtonVariant::Secondary | ButtonVariant::Ghost => colors.text,
            };
            let border = match variant {
                ButtonVariant::Secondary => colors.border,
                _ => transparent_black(),
            };
            (background(variant, phase, colors), text, border)
        };
        let hover = background(variant, Phase::Hover, colors);
        let pressed = background(variant, Phase::Pressed, colors);
        let mut label = self.size.text(theme);
        label.weight = theme.text.emphasis;

        text_style(div(), label)
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(self.size.height(theme))
            .px(self.size.padding(theme))
            .rounded(theme.radius.md)
            .border(theme.size.border)
            .border_color(border)
            .bg(fill)
            .text_color(text)
            .whitespace_nowrap()
            .when(
                variant == ButtonVariant::Secondary && !self.disabled,
                |this| this.shadow(theme.shadow.sm.clone()),
            )
            .when(interactive, |this| {
                this.hover(|style| style.bg(hover))
                    .active(|style| style.bg(pressed))
                    // A click leaves keyboard focus where it was.
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    // Enter presses the focused button on key-up; keep it from also
                    // confirming whatever contains the button.
                    .on_action(|_: &Confirm, _, _| {})
                    .when_some(self.on_click, |this, on_click| {
                        this.on_click(move |event, window, cx| on_click(event, window, cx))
                    })
            })
            .when_some(focus_handle, |this, handle| this.track_focus(&handle))
            .when(focused, |this| {
                this.child(focus_ring(theme.radius.md, theme))
            })
            .child(self.label)
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;
    use crate::ui::test::{click, focus_is_on_root, open, press};

    #[derive(Default)]
    struct Host {
        saved: usize,
        deleted: usize,
        confirmed: usize,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .on_action(cx.listener(|this, _: &Confirm, _, _| this.confirmed += 1))
                .child(
                    div().debug_selector(|| "off".into()).child(
                        Button::new("off", "Off")
                            .disabled(true)
                            .on_click(cx.listener(|this, _, _, _| this.deleted += 1)),
                    ),
                )
                .child(
                    div().debug_selector(|| "save".into()).child(
                        Button::new("save", "Save")
                            .on_click(cx.listener(|this, _, _, _| this.saved += 1)),
                    ),
                )
        }
    }

    #[gpui::test]
    fn a_click_presses_it_and_leaves_focus_alone(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        click(cx, "save");

        assert_eq!(host.read_with(cx, |host, _| host.saved), 1);
        assert!(focus_is_on_root(cx));
    }

    #[gpui::test]
    fn disabled_ignores_clicks_and_tab(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        click(cx, "off");
        press(cx, "tab enter");

        let (saved, deleted) = host.read_with(cx, |host, _| (host.saved, host.deleted));
        assert_eq!((saved, deleted), (1, 0), "Tab skipped the disabled button");
    }

    #[gpui::test]
    fn enter_and_space_press_the_focused_button_once(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        press(cx, "tab enter");
        assert_eq!(host.read_with(cx, |host, _| host.saved), 1);

        press(cx, "space");
        assert_eq!(host.read_with(cx, |host, _| host.saved), 2);

        assert_eq!(
            host.read_with(cx, |host, _| host.confirmed),
            0,
            "Enter on a button does not also confirm its container"
        );
    }
}
