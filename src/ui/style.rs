use gpui::{App, Div, ElementId, FocusHandle, Global, Hsla, Pixels, Styled, Window, div};

use crate::theme::{ActiveTheme, TextToken, Theme};

/// Colour roles for text and glyphs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Default,
    Muted,
    Disabled,
    Accent,
    OnAccent,
    Danger,
}

impl Color {
    pub fn resolve(self, cx: &App) -> Hsla {
        let colors = &cx.theme().colors;
        match self {
            Color::Default => colors.text,
            Color::Muted => colors.text_muted,
            Color::Disabled => colors.text_disabled,
            Color::Accent => colors.text_accent,
            Color::OnAccent => colors.text_on_accent,
            Color::Danger => colors.text_danger,
        }
    }
}

/// Steps of the type scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSize {
    Caption,
    Small,
    #[default]
    Body,
    Large,
    Title,
    Display,
}

impl TextSize {
    pub fn token(self, cx: &App) -> TextToken {
        let text = &cx.theme().text;
        match self {
            TextSize::Caption => text.caption,
            TextSize::Small => text.small,
            TextSize::Body => text.body,
            TextSize::Large => text.large,
            TextSize::Title => text.title,
            TextSize::Display => text.display,
        }
    }
}

/// Sizes a control comes in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ControlSize {
    pub(super) fn height(self, theme: &Theme) -> Pixels {
        match self {
            ControlSize::Small => theme.size.control_sm,
            ControlSize::Medium => theme.size.control_md,
            ControlSize::Large => theme.size.control_lg,
        }
    }

    pub(super) fn text(self, theme: &Theme) -> TextToken {
        match self {
            ControlSize::Small => theme.text.small,
            ControlSize::Medium => theme.text.body,
            ControlSize::Large => theme.text.large,
        }
    }

    pub(super) fn padding(self, theme: &Theme) -> Pixels {
        match self {
            ControlSize::Small => theme.spacing.sm,
            ControlSize::Medium => theme.spacing.md,
            ControlSize::Large => theme.spacing.lg,
        }
    }
}

/// Draws a control in one state regardless of the pointer and focus, for the gallery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    Hover,
    Pressed,
    Focused,
}

pub(super) fn text_style<E: Styled>(element: E, token: TextToken) -> E {
    element
        .text_size(token.size)
        .line_height(token.line_height)
        .font_weight(token.weight)
}

/// Keyboard focus for a stateless control: a handle kept between frames under the control's
/// id, and whether to draw the focus ring. A control that cannot be used gets no handle.
pub(super) fn focus(
    id: &ElementId,
    interactive: bool,
    preview: Option<Preview>,
    window: &mut Window,
    cx: &mut App,
) -> (Option<FocusHandle>, bool) {
    if !interactive {
        return (None, preview == Some(Preview::Focused));
    }
    let handle = window
        .use_keyed_state(id.clone(), cx, |_, cx| {
            cx.focus_handle().tab_index(0).tab_stop(true)
        })
        .read(cx)
        .clone();
    let ring = focus_visible(cx) && handle.is_focused(window);
    (Some(handle), ring)
}

/// The ring around the control that has keyboard focus. It sits outside the control,
/// one ring-width away, and follows the control's corners.
pub(super) fn focus_ring(radius: Pixels, theme: &Theme) -> Div {
    let width = theme.size.focus_ring;
    let outset = theme.size.border + width * 2.;
    div()
        .absolute()
        .top(-outset)
        .right(-outset)
        .bottom(-outset)
        .left(-outset)
        .rounded(radius + width * 2.)
        .border(width)
        .border_color(theme.colors.focus_ring)
}

struct KeyboardFocus(bool);

impl Global for KeyboardFocus {}

/// Whether focus last moved by keyboard. Controls show their focus ring only then.
pub fn focus_visible(cx: &App) -> bool {
    cx.try_global::<KeyboardFocus>()
        .is_some_and(|keyboard| keyboard.0)
}

pub(super) fn set_focus_visible(visible: bool, cx: &mut App) {
    if focus_visible(cx) != visible {
        cx.set_global(KeyboardFocus(visible));
        cx.refresh_windows();
    }
}
