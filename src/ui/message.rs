use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*};

use super::{
    avatar::Avatar,
    label::Label,
    style::{Color, TextSize},
};
use crate::theme::ActiveTheme;

/// One message in a conversation. A run of messages from one person shows who sent it once,
/// on the message that starts the run.
#[derive(IntoElement)]
pub struct MessageRow {
    sender: SharedString,
    photo: Option<SharedString>,
    time: SharedString,
    text: SharedString,
    attachments: Vec<SharedString>,
    day: Option<SharedString>,
    continues: bool,
    edited: bool,
    kind: Kind,
}

enum Kind {
    Sent,
    Notice,
    Deleted,
}

impl MessageRow {
    /// A message from `sender`, sent at `time` of day.
    pub fn new(sender: impl Into<SharedString>, time: impl Into<SharedString>) -> Self {
        Self {
            sender: sender.into(),
            photo: None,
            time: time.into(),
            text: SharedString::default(),
            attachments: Vec::new(),
            day: None,
            continues: false,
            edited: false,
            kind: Kind::Sent,
        }
    }

    /// A line from the service itself, such as someone joining.
    pub fn notice(text: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Notice,
            ..Self::new("", "").text(text)
        }
    }

    /// Where the sender's photo is, as Google gives it.
    pub fn photo(mut self, photo: Option<SharedString>) -> Self {
        self.photo = photo;
        self
    }

    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.text = text.into();
        self
    }

    /// Adds an attached file by name.
    pub fn attachment(mut self, name: impl Into<SharedString>) -> Self {
        self.attachments.push(name.into());
        self
    }

    /// Heads the row with the day it was sent on, for the first message of a day.
    pub fn day(mut self, day: impl Into<SharedString>) -> Self {
        self.day = Some(day.into());
        self
    }

    /// Continues the run of the message above it, so it does not repeat who sent it.
    pub fn continues(mut self, continues: bool) -> Self {
        self.continues = continues;
        self
    }

    pub fn edited(mut self, edited: bool) -> Self {
        self.edited = edited;
        self
    }

    /// Shows that the message was deleted, in place of what it said.
    pub fn deleted(mut self) -> Self {
        self.kind = Kind::Deleted;
        self
    }
}

impl RenderOnce for MessageRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let caption =
            |text: SharedString| Label::new(text).size(TextSize::Caption).color(Color::Muted);
        let day = self.day.map(|day| {
            div()
                .flex()
                .justify_center()
                .py(theme.spacing.sm)
                .child(caption(day).emphasized())
        });
        let column = div().flex().flex_col().px(theme.spacing.lg).children(day);

        if let Kind::Notice = self.kind {
            return column.child(
                div()
                    .flex()
                    .justify_center()
                    .py(theme.spacing.xs)
                    .child(caption(self.text)),
            );
        }

        let gutter = if self.continues {
            div().flex_none().w(theme.size.avatar_md).into_any_element()
        } else {
            Avatar::new(self.sender.clone())
                .photo(self.photo)
                .into_any_element()
        };
        let heading = (!self.continues).then(|| {
            div()
                .flex()
                .items_baseline()
                .gap(theme.spacing.sm)
                .child(Label::new(self.sender).emphasized())
                .child(caption(self.time))
        });
        let content = match self.kind {
            Kind::Deleted => Some(Label::new("This message was deleted.").color(Color::Muted)),
            _ if self.text.is_empty() && self.attachments.is_empty() => {
                Some(Label::new("This message can't be shown yet.").color(Color::Muted))
            }
            _ if self.text.is_empty() => None,
            _ => Some(Label::new(self.text)),
        };
        let attachments = self.attachments.into_iter().map(|name| {
            div().flex().pt(theme.spacing.xxs).child(
                div()
                    .max_w_full()
                    .px(theme.spacing.sm)
                    .py(theme.spacing.xs)
                    .rounded(theme.radius.md)
                    .border(theme.size.border)
                    .border_color(theme.colors.border)
                    .child(Label::new(name).size(TextSize::Small).truncate()),
            )
        });

        column.child(
            div()
                .flex()
                .gap(theme.spacing.sm)
                .pt(if self.continues {
                    theme.spacing.xxs
                } else {
                    theme.spacing.sm
                })
                .child(gutter)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .children(heading)
                        .children(content)
                        .children(attachments),
                )
                .children(self.edited.then(|| caption("Edited".into()))),
        )
    }
}
