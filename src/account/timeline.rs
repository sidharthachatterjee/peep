//! The messages of one conversation as loaded so far, ready to draw.

use std::time::{Duration, Instant};

use gpui::SharedString;

use crate::{
    chat::{Message, MessageKind, Page},
    time::Local,
};

/// Messages from one person this close together are drawn as one run.
const RUN_WINDOW: i64 = 5 * 60 * 1_000_000;
/// How long the newest messages count as current before they are asked for again.
const FRESH_FOR: Duration = Duration::from_secs(5);

/// One message, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: SharedString,
    pub sender: SharedString,
    /// The time of day it was sent.
    pub time: SharedString,
    pub kind: MessageKind,
    pub text: SharedString,
    /// The names of attached files.
    pub attachments: Vec<SharedString>,
    pub edited: bool,
    /// The day's heading, on the first message of each day.
    pub day: Option<SharedString>,
    /// Whether it starts a run of messages from one person, and so shows who sent it.
    pub leads: bool,
}

/// How a timeline's entries changed, so a list showing them can keep its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Entries were added at the end.
    Appended(usize),
    /// Entries were added at the start. The entry that was first may now draw differently.
    Prepended(usize),
    /// Anything else.
    Replaced,
}

#[derive(Debug, Default)]
pub struct Timeline {
    /// Oldest first.
    messages: Vec<Message>,
    entries: Vec<Entry>,
    /// Where the page before the oldest message starts. `None` at the conversation's start.
    older: Option<i64>,
    /// When the newest messages last arrived.
    loaded: Option<Instant>,
    pub(super) loading: bool,
    pub(super) loading_older: bool,
    /// Why the newest messages could not be loaded.
    pub(super) failed: Option<SharedString>,
}

impl Timeline {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether the newest messages have arrived at least once.
    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn is_loading_older(&self) -> bool {
        self.loading_older
    }

    pub fn failed(&self) -> Option<&SharedString> {
        self.failed.as_ref()
    }

    /// Where to load older messages from, if there are any and none are on their way.
    pub(super) fn older(&self) -> Option<i64> {
        self.older.filter(|_| !self.loading_older)
    }

    /// Whether the newest messages should be asked for.
    pub(super) fn is_stale(&self) -> bool {
        !self.loading
            && self
                .loaded
                .is_none_or(|loaded| loaded.elapsed() > FRESH_FOR)
    }

    /// When the newest message was sent.
    pub(super) fn newest_time(&self) -> Option<i64> {
        self.messages.last().map(|message| message.time)
    }

    /// Takes in the newest page. What was loaded before is kept in front of it when the two
    /// meet, and dropped when they do not: a gap would hide messages.
    pub(super) fn merge_newest(
        &mut self,
        page: Page,
        name: impl Fn(&Message) -> SharedString,
    ) -> Option<Change> {
        self.loading = false;
        self.failed = None;
        self.loaded = Some(Instant::now());

        let first = page.messages.first().map(|message| message.time);
        let kept = match first {
            Some(first) => self
                .messages
                .iter()
                .position(|message| message.time >= first)
                .unwrap_or(self.messages.len()),
            None => 0,
        };
        let meets = kept < self.messages.len() || page.older.is_none();
        let mut messages = if meets {
            self.messages[..kept].to_vec()
        } else {
            Vec::new()
        };
        if !meets || messages.is_empty() {
            self.older = page.older;
        }
        messages.extend(page.messages);

        if messages == self.messages {
            return None;
        }
        let before = self.messages.len();
        let change = if before > 0 && messages.len() > before && messages[..before] == self.messages
        {
            Change::Appended(messages.len() - before)
        } else {
            Change::Replaced
        };
        self.messages = messages;
        self.rebuild(name);
        Some(change)
    }

    /// Takes in the page before the oldest message.
    pub(super) fn merge_older(
        &mut self,
        page: Page,
        name: impl Fn(&Message) -> SharedString,
    ) -> Option<Change> {
        self.loading_older = false;
        self.older = page.older;

        let oldest = self.messages.first().map(|message| message.time);
        let mut older: Vec<Message> = page
            .messages
            .into_iter()
            .filter(|message| {
                oldest.is_none_or(|oldest| message.time <= oldest)
                    && !self.messages.iter().any(|known| known.id == message.id)
            })
            .collect();
        if older.is_empty() {
            return None;
        }
        let added = older.len();
        older.append(&mut self.messages);
        self.messages = older;
        self.rebuild(name);
        Some(Change::Prepended(added))
    }

    fn rebuild(&mut self, name: impl Fn(&Message) -> SharedString) {
        let today = Local::now();
        let mut previous: Option<(&Message, Local)> = None;
        self.entries = self
            .messages
            .iter()
            .map(|message| {
                let local = Local::at(message.time);
                let new_day = previous.is_none_or(|(_, before)| !before.same_day(&local));
                let leads = new_day
                    || previous.is_none_or(|(before, _)| {
                        before.sender != message.sender
                            || before.kind != MessageKind::Sent
                            || message.time - before.time > RUN_WINDOW
                    });
                previous = Some((message, local));
                Entry {
                    id: SharedString::from(message.id.clone()),
                    sender: name(message),
                    time: local.clock().into(),
                    kind: message.kind,
                    text: SharedString::from(message.text.clone()),
                    attachments: message
                        .attachments
                        .iter()
                        .map(|name| SharedString::from(name.clone()))
                        .collect(),
                    edited: message.edited,
                    day: new_day.then(|| local.day_label(&today).into()),
                    leads,
                }
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::UserId;

    const MINUTE: i64 = 60 * 1_000_000;
    /// Midday on 15 November 2023, UTC: far from midnight in most time zones.
    const NOON: i64 = 1_700_049_600_000_000;

    fn message(id: &str, sender: &str, time: i64) -> Message {
        Message {
            id: id.into(),
            sender: UserId(sender.into()),
            sender_name: "".into(),
            time,
            kind: MessageKind::Sent,
            text: format!("text {id}").into(),
            attachments: Vec::new(),
            edited: false,
        }
    }

    fn page(messages: Vec<Message>, older: Option<i64>) -> Page {
        Page { messages, older }
    }

    fn name(message: &Message) -> SharedString {
        SharedString::from(message.sender.0.clone())
    }

    fn ids(timeline: &Timeline) -> Vec<&str> {
        timeline
            .entries()
            .iter()
            .map(|entry| entry.id.as_ref())
            .collect()
    }

    #[test]
    fn a_run_from_one_person_shows_who_sent_it_once() {
        let mut timeline = Timeline::default();
        let mut notice = message("e", "ada", NOON + 4 * MINUTE);
        notice.kind = MessageKind::Notice;
        let messages = vec![
            message("a", "ada", NOON),
            message("b", "ada", NOON + MINUTE),
            message("c", "grace", NOON + 2 * MINUTE),
            message("d", "grace", NOON + 3 * MINUTE),
            notice,
            message("f", "grace", NOON + 5 * MINUTE),
            message("g", "grace", NOON + 30 * MINUTE),
        ];

        timeline.merge_newest(page(messages, None), name);

        let leads: Vec<bool> = timeline.entries().iter().map(|entry| entry.leads).collect();
        assert_eq!(leads, [true, false, true, false, true, true, true]);
        assert!(timeline.entries()[0].day.is_some());
        assert!(
            timeline.entries()[1..]
                .iter()
                .all(|entry| entry.day.is_none())
        );
        assert_eq!(timeline.entries()[2].sender.as_ref(), "grace");
    }

    #[test]
    fn new_messages_are_appended_and_the_same_page_changes_nothing() {
        let mut timeline = Timeline::default();
        let first = vec![
            message("a", "ada", NOON),
            message("b", "ada", NOON + MINUTE),
        ];
        assert_eq!(
            timeline.merge_newest(page(first.clone(), Some(NOON)), name),
            Some(Change::Replaced)
        );
        assert_eq!(timeline.merge_newest(page(first, Some(NOON)), name), None);

        let next = vec![
            message("b", "ada", NOON + MINUTE),
            message("c", "ada", NOON + 2 * MINUTE),
        ];
        assert_eq!(
            timeline.merge_newest(page(next, Some(NOON + MINUTE)), name),
            Some(Change::Appended(1))
        );
        assert_eq!(ids(&timeline), ["a", "b", "c"]);
        assert_eq!(timeline.older(), Some(NOON), "the older cursor is kept");
    }

    #[test]
    fn an_edit_replaces_and_a_gap_drops_what_came_before() {
        let mut timeline = Timeline::default();
        timeline.merge_newest(page(vec![message("a", "ada", NOON)], Some(NOON)), name);

        let mut edited = message("a", "ada", NOON);
        edited.edited = true;
        assert_eq!(
            timeline.merge_newest(page(vec![edited], Some(NOON)), name),
            Some(Change::Replaced)
        );

        let later = vec![message("x", "ada", NOON + 60 * MINUTE)];
        assert_eq!(
            timeline.merge_newest(page(later, Some(NOON + 60 * MINUTE)), name),
            Some(Change::Replaced)
        );
        assert_eq!(ids(&timeline), ["x"], "messages may be missing in between");
        assert_eq!(timeline.older(), Some(NOON + 60 * MINUTE));
    }

    #[test]
    fn older_messages_go_in_front_without_repeats() {
        let mut timeline = Timeline::default();
        timeline.merge_newest(
            page(vec![message("c", "ada", NOON + 2 * MINUTE)], Some(NOON)),
            name,
        );
        timeline.loading_older = true;
        assert_eq!(timeline.older(), None, "one page at a time");

        let older = vec![
            message("a", "ada", NOON),
            message("b", "ada", NOON + MINUTE),
            message("c", "ada", NOON + 2 * MINUTE),
        ];
        assert_eq!(
            timeline.merge_older(page(older, None), name),
            Some(Change::Prepended(2))
        );
        assert_eq!(ids(&timeline), ["a", "b", "c"]);
        assert!(!timeline.entries()[2].leads, "it now continues a run");
        assert_eq!(timeline.older(), None, "the start was reached");
    }
}
