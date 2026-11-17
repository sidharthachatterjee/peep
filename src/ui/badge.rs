use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*};

use super::style::text_style;
use crate::theme::ActiveTheme;

/// A short count or status that draws the eye, such as unread messages.
#[derive(IntoElement)]
pub struct Badge {
    text: SharedString,
}

impl Badge {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut text = theme.text.caption;
        text.weight = theme.text.emphasis;

        text_style(div(), text)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(theme.size.badge)
            .min_w(theme.size.badge)
            .px(theme.spacing.xs)
            .rounded(theme.radius.full)
            .bg(theme.colors.accent)
            .text_color(theme.colors.text_on_accent)
            .child(self.text)
    }
}
