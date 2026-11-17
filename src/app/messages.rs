use gpui::{
    App, Context, Entity, IntoElement, ListAlignment, ListScrollEvent, ListState, Render,
    SharedString, Subscription, Window, div, list, prelude::*,
};

use crate::{
    account::{Account, Change, Entry, TimelineChanged},
    chat::MessageKind,
    theme::ActiveTheme,
    ui::{Button, Color, Divider, Label, MessageRow, TextSize},
};

/// How near the oldest loaded message scrolling comes before older ones are asked for.
const LOAD_OLDER_WITHIN: usize = 10;

/// The messages of the open conversation, newest at the bottom.
pub struct Messages {
    account: Entity<Account>,
    conversation: Option<SharedString>,
    /// One row per entry of the open conversation's timeline.
    list: ListState,
    _subscriptions: [Subscription; 3],
}

impl Messages {
    pub fn new(account: Entity<Account>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = ListState::new(0, ListAlignment::Bottom, cx.theme().size.list_overdraw);
        list.set_scroll_handler(cx.listener(|this, event: &ListScrollEvent, _, cx| {
            if event.visible_range.start < LOAD_OLDER_WITHIN
                && let Some(conversation) = &this.conversation
            {
                this.account
                    .update(cx, |account, cx| account.load_older(conversation, cx));
            }
        }));
        Self {
            _subscriptions: [
                cx.subscribe(&account, |this, _, event: &TimelineChanged, cx| {
                    this.timeline_changed(event, cx)
                }),
                cx.observe_in(&account, window, |this, _, window, cx| {
                    this.attend(window, cx);
                    cx.notify();
                }),
                cx.observe_window_activation(window, |this, window, cx| this.attend(window, cx)),
            ],
            account,
            conversation: None,
            list,
        }
    }

    /// Shows a conversation, or none.
    pub fn show(
        &mut self,
        conversation: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.conversation == conversation {
            return;
        }
        self.conversation = conversation;
        self.list.reset(self.entry_count(cx));
        self.attend(window, cx);
        cx.notify();
    }

    /// While the window is in front, keeps the open conversation's messages current and
    /// counts them as read.
    fn attend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conversation) = &self.conversation else {
            return;
        };
        if window.is_window_active() {
            self.account.update(cx, |account, cx| {
                account.open(conversation, cx);
                account.mark_read(conversation, cx);
            });
        }
    }

    fn timeline_changed(&mut self, event: &TimelineChanged, cx: &mut Context<Self>) {
        if self.conversation.as_ref() != Some(&event.conversation) {
            return;
        }
        let shown = self.list.item_count();
        let count = self.entry_count(cx);
        match event.change {
            Change::Appended(added) if shown + added == count => {
                self.list.splice(shown..shown, added)
            }
            // The row that was first is laid out again: it may no longer start a run.
            Change::Prepended(added) if shown > 0 && shown + added == count => {
                self.list.splice(0..1, added + 1)
            }
            Change::Updated(index) if shown == count && index < count => {
                self.list.splice(index..index + 1, 1)
            }
            // The row after it is laid out again, for the same reason.
            Change::Removed(index) if shown == count + 1 && index < shown => {
                let after = usize::from(index < count);
                self.list.splice(index..index + 1 + after, after)
            }
            _ => self.list.reset(count),
        }
        cx.notify();
    }

    fn entry_count(&self, cx: &App) -> usize {
        self.conversation
            .as_ref()
            .and_then(|conversation| self.account.read(cx).timeline(conversation))
            .map_or(0, |timeline| timeline.entries().len())
    }
}

impl Render for Messages {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let note = |text: &'static str| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(theme.spacing.sm)
                .child(Label::new(text).color(Color::Muted))
        };
        let Some(conversation) = self.conversation.clone() else {
            return note("Select a conversation.").into_any_element();
        };
        let account = self.account.read(cx);
        let title = account
            .rows()
            .iter()
            .find(|row| row.id == conversation)
            .map(|row| row.title.clone())
            .unwrap_or_default();
        let timeline = account.timeline(&conversation);
        let loading_older = timeline.is_some_and(|timeline| timeline.is_loading_older());

        let body = match timeline {
            Some(timeline) if !timeline.entries().is_empty() => {
                let account = self.account.clone();
                let conversation = conversation.clone();
                list(self.list.clone(), move |index, _, cx| {
                    let entry = account
                        .read(cx)
                        .timeline(&conversation)
                        .and_then(|timeline| timeline.entries().get(index));
                    match entry {
                        Some(entry) => row(entry).into_any_element(),
                        None => div().into_any_element(),
                    }
                })
                .size_full()
                .py(theme.spacing.md)
                .into_any_element()
            }
            Some(timeline) if timeline.failed().is_some() => note("")
                .children(
                    timeline
                        .failed()
                        .map(|error| Label::new(error.clone()).color(Color::Danger)),
                )
                .child(
                    Button::new("retry-messages", "Try again").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(conversation) = &this.conversation {
                                this.account
                                    .update(cx, |account, cx| account.open(conversation, cx));
                            }
                        },
                    )),
                )
                .into_any_element(),
            Some(timeline) if timeline.is_loaded() => note("No messages yet.").into_any_element(),
            _ => note("Loading messages…").into_any_element(),
        };

        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(theme.spacing.md)
            .px(theme.spacing.lg)
            .pb(theme.spacing.md)
            .child(
                div().flex_1().overflow_hidden().child(
                    Label::new(title)
                        .size(TextSize::Large)
                        .emphasized()
                        .truncate(),
                ),
            )
            .children(loading_older.then(|| {
                Label::new("Loading earlier messages…")
                    .size(TextSize::Caption)
                    .color(Color::Muted)
            }));

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(Divider::horizontal())
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

fn row(entry: &Entry) -> MessageRow {
    if entry.kind == MessageKind::Notice {
        let notice = MessageRow::notice(entry.text.clone());
        return match &entry.day {
            Some(day) => notice.day(day.clone()),
            None => notice,
        };
    }
    let mut row = MessageRow::new(entry.sender.clone(), entry.time.clone())
        .text(entry.text.clone())
        .continues(!entry.leads)
        .edited(entry.edited);
    if entry.kind == MessageKind::Deleted {
        row = row.deleted();
    }
    if let Some(day) = &entry.day {
        row = row.day(day.clone());
    }
    for name in &entry.attachments {
        row = row.attachment(name.clone());
    }
    row
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::{
        account::{Row, Status},
        chat::{Message, Page, UserId},
        ui::Root,
    };

    const MINUTE: i64 = 60 * 1_000_000;
    const NOON: i64 = 1_700_049_600_000_000;

    fn row(id: &str, unread: Option<&str>) -> Row {
        Row {
            id: id.to_owned().into(),
            title: format!("Person {id}").into(),
            unread: unread.map(|unread| unread.to_owned().into()),
            muted: false,
        }
    }

    /// `count` messages a minute apart, the first numbered `from`.
    fn page(from: usize, count: usize, older: Option<i64>) -> Page {
        let messages = (from..from + count)
            .map(|number| Message {
                id: format!("m{number}").into(),
                sender: UserId(if number % 3 == 0 { "ada" } else { "grace" }.into()),
                sender_name: "".into(),
                time: NOON + number as i64 * MINUTE,
                kind: MessageKind::Sent,
                text: format!("Message number {number}").into(),
                attachments: Vec::new(),
                edited: false,
            })
            .collect();
        Page { messages, older }
    }

    fn open(
        cx: &mut TestAppContext,
        rows: Vec<Row>,
    ) -> (Entity<Account>, Entity<Messages>, &mut VisualTestContext) {
        cx.update(crate::ui::init);
        let account = cx.new(|_| Account::with(Status::Ready, rows));
        let mut messages = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Messages::new(account.clone(), window, cx));
            messages = Some(view.clone());
            Root::new(view, window, cx)
        });
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        (account, messages.unwrap(), cx)
    }

    fn show(messages: &Entity<Messages>, id: &str, cx: &mut VisualTestContext) {
        let id = SharedString::from(id.to_owned());
        cx.update(|window, cx| {
            messages.update(cx, |messages, cx| messages.show(Some(id), window, cx))
        });
        cx.run_until_parked();
    }

    fn arrive(account: &Entity<Account>, id: &str, page: Page, cx: &mut VisualTestContext) {
        let id = SharedString::from(id.to_owned());
        account.update(cx, |account, cx| account.newest_arrived(&id, Ok(page), cx));
        cx.run_until_parked();
    }

    fn count(messages: &Entity<Messages>, cx: &mut VisualTestContext) -> usize {
        messages.read_with(cx, |messages, _| messages.list.item_count())
    }

    /// Whether the list is scrolled to its newest row, where it stays as rows are added.
    fn at_newest(messages: &Entity<Messages>, cx: &mut VisualTestContext) -> bool {
        messages.read_with(cx, |messages, _| {
            messages.list.logical_scroll_top().item_ix == messages.list.item_count()
        })
    }

    #[gpui::test]
    fn only_the_rows_near_the_newest_message_are_laid_out(cx: &mut TestAppContext) {
        let (account, messages, cx) = open(cx, vec![row("a", None)]);
        show(&messages, "a", cx);
        assert_eq!(count(&messages, cx), 0, "nothing has arrived yet");

        arrive(&account, "a", page(0, 5000, None), cx);
        assert_eq!(count(&messages, cx), 5000);
        assert!(at_newest(&messages, cx));

        // A list resting at its end does not say where its rows are; one a hair above does.
        let hair = cx.update(|_, cx| cx.theme().size.border);
        let laid_out: Vec<usize> = messages.read_with(cx, |messages, _| {
            messages.list.scroll_by(-hair);
            (0..5000)
                .filter(|index| messages.list.bounds_for_item(*index).is_some())
                .collect()
        });
        assert_eq!(laid_out.last(), Some(&4999), "the newest message shows");
        assert!(
            (1..200).contains(&laid_out.len()),
            "{} rows laid out for one screen",
            laid_out.len()
        );
    }

    #[gpui::test]
    fn new_and_older_messages_join_without_starting_over(cx: &mut TestAppContext) {
        let (account, messages, cx) = open(cx, vec![row("a", None)]);
        show(&messages, "a", cx);
        arrive(&account, "a", page(100, 40, Some(NOON)), cx);
        assert_eq!(count(&messages, cx), 40);

        arrive(&account, "a", page(130, 20, Some(NOON)), cx);
        assert_eq!(count(&messages, cx), 50, "ten of the twenty were new");
        assert!(at_newest(&messages, cx));

        let id = SharedString::from("a");
        account.update(cx, |account, cx| {
            account.older_arrived(&id, Ok(page(60, 40, None)), cx)
        });
        cx.run_until_parked();
        assert_eq!(count(&messages, cx), 90);
        assert!(
            at_newest(&messages, cx),
            "older messages do not move the view"
        );
    }

    #[gpui::test]
    fn what_the_live_channel_reports_shows_at_once(cx: &mut TestAppContext) {
        use crate::chat::{ConversationId, Event};

        let (account, messages, cx) = open(cx, vec![row("dm/a", None), row("dm/b", None)]);
        account.update(cx, |account, _| account.know(&["a", "b"]));
        show(&messages, "dm/a", cx);
        arrive(&account, "dm/a", page(0, 30, None), cx);
        let hear = |event: Event, cx: &mut VisualTestContext| {
            account.update(cx, |account, cx| account.heard(event, cx));
            cx.run_until_parked();
        };
        let message = |conversation: &str, number: usize, posted: bool| Event::Message {
            conversation: ConversationId::Dm(conversation.into()),
            message: page(number, 1, None).messages.remove(0),
            posted,
        };

        hear(message("a", 30, true), cx);
        assert_eq!(count(&messages, cx), 31);
        assert!(at_newest(&messages, cx));

        let mut changed = page(10, 1, None).messages.remove(0);
        changed.text = "Changed".into();
        hear(
            Event::Message {
                conversation: ConversationId::Dm("a".into()),
                message: changed,
                posted: false,
            },
            cx,
        );
        hear(
            Event::Removed {
                conversation: ConversationId::Dm("a".into()),
                message: "m5".into(),
            },
            cx,
        );
        assert_eq!(count(&messages, cx), 30);
        let texts = account.read_with(cx, |account, _| {
            let entries = account.timeline(&"dm/a".into()).unwrap().entries();
            (entries[5].id.to_string(), entries[9].text.to_string())
        });
        assert_eq!(texts, ("m6".to_owned(), "Changed".to_owned()));

        hear(message("b", 40, true), cx);
        assert_eq!(count(&messages, cx), 30, "another conversation's message");
        let rows = account.read_with(cx, |account, _| {
            account
                .rows()
                .iter()
                .map(|row| (row.id.to_string(), row.unread.is_some()))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            rows,
            [("dm/b".to_owned(), true), ("dm/a".to_owned(), false)],
            "it moves its conversation to the top, unread; the open one stays read"
        );
    }

    #[gpui::test]
    fn each_conversation_shows_its_own_messages(cx: &mut TestAppContext) {
        let (account, messages, cx) = open(cx, vec![row("a", None), row("b", None)]);
        arrive(&account, "a", page(0, 12, None), cx);
        arrive(&account, "b", page(0, 3, None), cx);
        assert_eq!(count(&messages, cx), 0, "no conversation is open");

        show(&messages, "a", cx);
        assert_eq!(count(&messages, cx), 12);
        show(&messages, "b", cx);
        assert_eq!(count(&messages, cx), 3);

        arrive(&account, "a", page(0, 13, None), cx);
        assert_eq!(count(&messages, cx), 3, "another conversation's news");
    }

    #[gpui::test]
    fn an_open_conversation_counts_as_read_once_its_messages_show(cx: &mut TestAppContext) {
        let (account, messages, cx) = open(cx, vec![row("a", Some("2")), row("b", Some("1"))]);
        let unread = |cx: &mut VisualTestContext| {
            account.read_with(cx, |account, _| {
                account
                    .rows()
                    .iter()
                    .map(|row| row.unread.is_some())
                    .collect::<Vec<_>>()
            })
        };

        show(&messages, "a", cx);
        assert_eq!(unread(cx), [true, true], "nothing to have read yet");

        arrive(&account, "a", page(0, 3, None), cx);
        assert_eq!(unread(cx), [false, true]);
    }
}
