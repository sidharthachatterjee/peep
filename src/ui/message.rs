use gpui::{
    App, ElementId, HighlightStyle, InteractiveText, IntoElement, RenderOnce, SharedString,
    StyledText, UnderlineStyle, Window, div, prelude::*, relative,
};

use super::{
    avatar::Avatar,
    label::Label,
    style::{Color, TextSize},
};
use crate::{
    chat::{Mark, MarkKind},
    theme::ActiveTheme,
};

/// The kinds of address a link in a message may open. Anything else stays text.
const OPENED: [&str; 3] = ["https://", "http://", "mailto:"];

/// One message in a conversation, in a bubble: the signed-in person's on the right, everyone
/// else's on the left with their photo. A run of messages from one person shows who sent
/// it and when once, on the message that starts the run.
#[derive(IntoElement)]
pub struct MessageRow {
    id: ElementId,
    sender: SharedString,
    photo: Option<SharedString>,
    time: SharedString,
    text: SharedString,
    marks: Vec<Mark>,
    quote: Option<(SharedString, SharedString)>,
    attachments: Vec<SharedString>,
    reactions: Vec<(SharedString, u32, bool)>,
    day: Option<SharedString>,
    mine: bool,
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
    /// A message from `sender`, sent at `time`.
    pub fn new(
        id: impl Into<ElementId>,
        sender: impl Into<SharedString>,
        time: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            sender: sender.into(),
            photo: None,
            time: time.into(),
            text: SharedString::default(),
            marks: Vec::new(),
            quote: None,
            attachments: Vec::new(),
            reactions: Vec::new(),
            day: None,
            mine: false,
            continues: false,
            edited: false,
            kind: Kind::Sent,
        }
    }

    /// A line from the service itself, such as someone joining.
    pub fn notice(text: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Notice,
            ..Self::new("notice", "", "").text(text)
        }
    }

    /// Draws it as the signed-in person's own: on the right, without their name or photo.
    pub fn mine(mut self, mine: bool) -> Self {
        self.mine = mine;
        self
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

    /// The parts of the text that are links, which open when clicked, or name someone.
    pub fn marks(mut self, marks: Vec<Mark>) -> Self {
        self.marks = marks;
        self
    }

    /// Shows the message this one answers above it: who sent that, and what it said.
    pub fn quote(mut self, sender: impl Into<SharedString>, text: impl Into<SharedString>) -> Self {
        self.quote = Some((sender.into(), text.into()));
        self
    }

    /// Adds an attached file by name.
    pub fn attachment(mut self, name: impl Into<SharedString>) -> Self {
        self.attachments.push(name.into());
        self
    }

    /// Adds an emoji people answered with, how many did, and whether the signed-in person
    /// is one of them.
    pub fn reaction(mut self, emoji: impl Into<SharedString>, count: u32, mine: bool) -> Self {
        self.reactions.push((emoji.into(), count, mine));
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
        let colors = &theme.colors;
        let caption =
            |text: SharedString| Label::new(text).size(TextSize::Caption).color(Color::Muted);
        let day = self.day.map(|day| {
            div()
                .flex()
                .justify_center()
                .py(theme.spacing.sm)
                .child(caption(day).emphasized())
        });
        // The row fills its list, so the day's heading centres over the conversation rather
        // than over this message.
        let column = div()
            .w_full()
            .flex()
            .flex_col()
            .px(theme.spacing.lg)
            .children(day);

        if let Kind::Notice = self.kind {
            return column.child(
                div()
                    .flex()
                    .justify_center()
                    .py(theme.spacing.xs)
                    .child(caption(self.text)),
            );
        }

        let mine = self.mine;
        let (fill, ink, quiet) = match mine {
            true => (colors.accent, colors.text_on_accent, colors.text_on_accent),
            false => (colors.element, colors.text, colors.text_muted),
        };
        let heading = (!self.continues).then(|| {
            div()
                .flex()
                .items_baseline()
                .gap(theme.spacing.sm)
                .pb(theme.spacing.xxs)
                .px(theme.spacing.xs)
                .when(!mine, |this| {
                    this.child(
                        Label::new(self.sender.clone())
                            .size(TextSize::Small)
                            .emphasized(),
                    )
                })
                .child(caption(self.time))
        });

        let quote = self.quote.map(|(sender, text)| {
            div()
                .flex()
                .flex_col()
                .mb(theme.spacing.xs)
                .pl(theme.spacing.sm)
                .border_l(theme.size.focus_ring)
                .border_color(quiet)
                .text_color(quiet)
                .text_size(theme.text.small.size)
                .line_height(theme.text.small.line_height)
                .child(div().font_weight(theme.text.emphasis).child(sender))
                .child(div().line_clamp(2).child(text))
        });
        let content = match self.kind {
            Kind::Deleted => Some(
                div()
                    .text_color(quiet)
                    .child("This message was deleted.")
                    .into_any_element(),
            ),
            _ if self.text.is_empty() && self.attachments.is_empty() => Some(
                div()
                    .text_color(quiet)
                    .child("This message can't be shown yet.")
                    .into_any_element(),
            ),
            _ if self.text.is_empty() => None,
            _ => Some(text(self.id, self.text, self.marks, mine, cx).into_any_element()),
        };
        let attachments = self.attachments.into_iter().map(|name| {
            div().flex().pt(theme.spacing.xxs).child(
                div()
                    .max_w_full()
                    .px(theme.spacing.sm)
                    .py(theme.spacing.xs)
                    .rounded(theme.radius.md)
                    .border(theme.size.border)
                    .border_color(quiet)
                    .text_size(theme.text.small.size)
                    .line_height(theme.text.small.line_height)
                    .truncate()
                    .child(name),
            )
        });
        let bubble = div()
            .debug_selector(|| "bubble".into())
            .max_w_full()
            .flex()
            .flex_col()
            .px(theme.spacing.md)
            .py(theme.spacing.sm)
            .rounded(theme.radius.lg)
            .bg(fill)
            .text_color(ink)
            .children(quote)
            .children(content)
            .children(attachments)
            .children(self.edited.then(|| {
                div()
                    .text_size(theme.text.caption.size)
                    .line_height(theme.text.caption.line_height)
                    .text_color(quiet)
                    .child("Edited")
            }));

        let reactions = (!self.reactions.is_empty()).then(|| {
            let chips = self.reactions.into_iter().map(|(emoji, count, mine)| {
                div()
                    .flex()
                    .items_center()
                    .gap(theme.spacing.xs)
                    .px(theme.spacing.sm)
                    .rounded(theme.radius.full)
                    .border(theme.size.border)
                    .border_color(match mine {
                        true => colors.border_control,
                        false => colors.border,
                    })
                    .bg(match mine {
                        true => colors.element_active,
                        false => colors.surface_raised,
                    })
                    .text_size(theme.text.small.size)
                    .line_height(theme.text.body.line_height)
                    .child(emoji)
                    .when(count > 1, |this| {
                        this.child(
                            div()
                                .text_size(theme.text.caption.size)
                                .text_color(colors.text_muted)
                                .child(count.to_string()),
                        )
                    })
            });
            div()
                .flex()
                .flex_wrap()
                .gap(theme.spacing.xs)
                .pt(theme.spacing.xxs)
                .children(chips)
        });

        let stack = div()
            .flex()
            .flex_col()
            .min_w_0()
            .max_w(relative(theme.size.bubble_share))
            .map(|this| match mine {
                true => this.items_end(),
                false => this.items_start(),
            })
            .children(heading)
            .child(bubble)
            .children(reactions);
        let gutter = match (mine, self.continues) {
            (true, _) => None,
            (false, true) => Some(div().flex_none().w(theme.size.avatar_md).into_any_element()),
            (false, false) => Some(
                Avatar::new(self.sender)
                    .photo(self.photo)
                    .into_any_element(),
            ),
        };

        column.child(
            div()
                .flex()
                .gap(theme.spacing.sm)
                .pt(if self.continues {
                    theme.spacing.xxs
                } else {
                    theme.spacing.md
                })
                .when(mine, |this| this.justify_end())
                .children(gutter)
                .child(stack),
        )
    }
}

/// A message's text, with its links underlined and opening when clicked, and the names of
/// people in it standing out.
fn text(
    id: ElementId,
    text: SharedString,
    marks: Vec<Mark>,
    mine: bool,
    cx: &App,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    // On the dark bubble of one's own messages the accent colour would not show.
    let accent = match mine {
        true => theme.colors.text_on_accent,
        false => theme.colors.text_accent,
    };
    let within = |mark: &&Mark| {
        mark.range.end <= text.len()
            && text.is_char_boundary(mark.range.start)
            && text.is_char_boundary(mark.range.end)
    };
    let highlights = marks.iter().filter(within).map(|mark| {
        let style = match mark.kind {
            MarkKind::Link(_) => HighlightStyle {
                color: Some(accent),
                underline: Some(UnderlineStyle {
                    thickness: theme.size.border,
                    color: Some(accent),
                    wavy: false,
                }),
                ..Default::default()
            },
            MarkKind::Mention => HighlightStyle {
                color: Some(accent),
                font_weight: Some(theme.text.emphasis),
                ..Default::default()
            },
        };
        (mark.range.clone(), style)
    });
    let styled = StyledText::new(text.clone()).with_highlights(highlights);

    let links: Vec<(std::ops::Range<usize>, SharedString)> = marks
        .iter()
        .filter(within)
        .filter_map(|mark| match &mark.kind {
            MarkKind::Link(address) if OPENED.iter().any(|kind| address.starts_with(kind)) => {
                Some((mark.range.clone(), SharedString::from(address.clone())))
            }
            _ => None,
        })
        .collect();
    let ranges = links.iter().map(|(range, _)| range.clone()).collect();
    InteractiveText::new(id, styled).on_click(ranges, move |index, _, cx| {
        if let Some((_, address)) = links.get(index) {
            cx.open_url(address);
        }
    })
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Render, TestAppContext};

    use super::*;
    use crate::ui::test::{click, open};

    /// A message that is one mark from end to end.
    struct Host(MarkKind);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let text = "example.com/engine";
            MessageRow::new("m", "Ada", "09:12")
                .text(text)
                .marks(vec![Mark {
                    range: 0..text.len(),
                    kind: self.0.clone(),
                }])
        }
    }

    #[gpui::test]
    fn a_link_opens_when_clicked_and_only_a_web_or_mail_address(cx: &mut TestAppContext) {
        let opened = |kind: MarkKind, cx: &mut TestAppContext| {
            let cx = &mut cx.new_app();
            let (_, cx) = open(cx, |_, _| Host(kind));
            click(cx, "bubble");
            cx.opened_url()
        };
        let link = |address: &str| MarkKind::Link(address.into());

        let address = "https://example.com/engine";
        assert_eq!(opened(link(address), cx).as_deref(), Some(address));
        assert!(opened(link("mailto:ada@example.com"), cx).is_some());
        assert_eq!(opened(link("file:///etc/passwd"), cx), None);
        assert_eq!(opened(link("javascript:alert(1)"), cx), None);
        assert_eq!(opened(MarkKind::Mention, cx), None, "a name goes nowhere");
    }
}
