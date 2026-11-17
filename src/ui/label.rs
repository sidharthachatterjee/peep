use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*};

use super::style::{Color, TextSize, text_style};
use crate::theme::ActiveTheme;

/// A run of text in one step of the type scale.
#[derive(IntoElement)]
pub struct Label {
    text: SharedString,
    size: TextSize,
    color: Color,
    emphasized: bool,
    truncate: bool,
}

impl Label {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            size: TextSize::default(),
            color: Color::default(),
            emphasized: false,
            truncate: false,
        }
    }

    pub fn size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Uses the heavier weight of the type scale.
    pub fn emphasized(mut self) -> Self {
        self.emphasized = true;
        self
    }

    /// Keeps the text on one line and ends it with an ellipsis when it does not fit.
    pub fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut token = self.size.token(cx);
        let emphasis = cx.theme().text.emphasis;
        if self.emphasized && token.weight.0 < emphasis.0 {
            token.weight = emphasis;
        }
        text_style(div(), token)
            .text_color(self.color.resolve(cx))
            .when(self.truncate, |this| this.truncate())
            .child(self.text)
    }
}
