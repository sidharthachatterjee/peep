use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*};

use super::style::{ControlSize, text_style};
use crate::theme::ActiveTheme;

/// A person or space, shown as initials in a circle.
#[derive(IntoElement)]
pub struct Avatar {
    name: SharedString,
    size: ControlSize,
}

impl Avatar {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            size: ControlSize::default(),
        }
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Avatar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (diameter, mut text) = match self.size {
            ControlSize::Small => (theme.size.avatar_sm, theme.text.caption),
            ControlSize::Medium => (theme.size.avatar_md, theme.text.small),
            ControlSize::Large => (theme.size.avatar_lg, theme.text.body),
        };
        text.weight = theme.text.emphasis;
        let initials: String = self
            .name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(2)
            .flat_map(char::to_uppercase)
            .collect();

        text_style(div(), text)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(diameter)
            .rounded(theme.radius.full)
            .bg(theme.colors.element_active)
            .text_color(theme.colors.text)
            .child(initials)
    }
}
