//! A Google Chat that tests fill in and listen to, in place of Google's.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use futures::{
    FutureExt as _,
    channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    future,
};

use super::{
    Answer, Conversation, ConversationId, Directory, Error, Event, Message, MessageKind, Page,
    Service, Showing, Snapshot, Status, User, UserId,
};

/// How many messages a page holds.
const PAGE: usize = 3;

/// Something Peep asked of the service.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Asked {
    Refresh,
    Messages(String, Option<i64>),
    MarkRead(String, i64),
    Statuses(Vec<String>),
    Show(Showing),
    Watch(Vec<String>),
}

/// The service and the handle a test keeps on it are the same thing, shared.
#[derive(Clone, Default)]
pub(crate) struct Fake(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    directory: Directory,
    /// Each conversation's messages by its key, oldest first.
    messages: HashMap<String, Vec<Message>>,
    statuses: HashMap<UserId, Status>,
    /// Whether asking is refused, as when Google has signed the person out.
    signed_out: bool,
    listeners: Vec<UnboundedSender<Event>>,
    asked: Vec<Asked>,
    /// When the newest things happen, if not now.
    now: Option<i64>,
}

impl Fake {
    /// A service for someone called "me", with nobody to talk to yet.
    pub(crate) fn new() -> Self {
        let fake = Self::default();
        fake.0.borrow_mut().directory.me = Some(UserId("me".into()));
        fake
    }

    /// Has everything happen around a set time, in microseconds since the Unix epoch, rather
    /// than now. Call it before adding conversations.
    pub(crate) fn around(self, time: i64) -> Self {
        self.0.borrow_mut().now = Some(time);
        self
    }

    /// Adds a direct message with one person, whose id is their name in lower case. Its
    /// messages are `(sender, text)`, oldest first, a minute apart and ending `minutes_ago`.
    /// Those the person wrote after `read` of them are unread.
    pub(crate) fn conversation(
        self,
        name: &str,
        messages: &[(&str, &str)],
        minutes_ago: i64,
        read: usize,
    ) -> Self {
        let person = UserId(name.to_lowercase().into());
        let id = ConversationId::Dm(format!("{}-dm", person.0).into());
        let newest = self.time(minutes_ago);
        let messages: Vec<Message> = messages
            .iter()
            .enumerate()
            .map(|(index, (sender, text))| {
                let before_newest = (messages.len() - 1 - index) as i64;
                message(
                    &format!("{}-{index}", person.0),
                    sender,
                    text,
                    newest - before_newest * MINUTE,
                )
            })
            .collect();
        let unread = messages
            .iter()
            .skip(read)
            .filter(|message| message.sender == person)
            .count();
        {
            let mut state = self.0.borrow_mut();
            state.directory.users.insert(
                person.clone(),
                User {
                    name: name.into(),
                    email: format!("{}@example.com", person.0).into(),
                    photo: "".into(),
                },
            );
            state.directory.conversations.push(Conversation {
                id: id.clone(),
                title: name.into(),
                members: vec![person],
                name: None,
                unread: unread as u32,
                muted: false,
                sort_time: newest,
            });
            state
                .directory
                .conversations
                .sort_by_key(|conversation| std::cmp::Reverse(conversation.sort_time));
            state.directory.unread += u32::from(unread > 0);
            state.messages.insert(id.key(), messages);
        }
        self
    }

    /// Says how a person is doing, for when Peep asks.
    pub(crate) fn status(self, person: &str, status: Status) -> Self {
        let person = UserId(person.into());
        self.0.borrow_mut().statuses.insert(person, status);
        self
    }

    /// Reports `event` on the live channel.
    pub(crate) fn say(&self, event: Event) {
        self.0
            .borrow_mut()
            .listeners
            .retain(|listener| listener.unbounded_send(event.clone()).is_ok());
    }

    /// Someone posts a message now: it is kept, and reported on the live channel.
    pub(crate) fn post(&self, conversation: &str, sender: &str, text: &str) {
        let (id, message) = {
            let mut state = self.0.borrow_mut();
            let now = state.now.unwrap_or_else(now);
            let kept = state.messages.entry(conversation.to_owned()).or_default();
            let message = message(&format!("posted-{}", kept.len()), sender, text, now);
            kept.push(message.clone());
            let known = state
                .directory
                .conversations
                .iter_mut()
                .find(|known| known.id.has_key(conversation))
                .expect("a message is posted to a conversation the service has");
            known.sort_time = message.time;
            (known.id.clone(), message)
        };
        self.say(Event::Message {
            conversation: id,
            message,
            posted: true,
        });
    }

    /// From now on asking is refused, as when Google has signed the person out.
    pub(crate) fn sign_out(&self) {
        self.0.borrow_mut().signed_out = true;
    }

    /// What Peep has asked since this was last called, in order.
    pub(crate) fn asked(&self) -> Vec<Asked> {
        std::mem::take(&mut self.0.borrow_mut().asked)
    }

    /// The time `minutes_ago`, in microseconds since the Unix epoch.
    fn time(&self, minutes_ago: i64) -> i64 {
        self.0.borrow().now.unwrap_or_else(now) - minutes_ago * MINUTE
    }

    /// Notes what was asked and gives the state to answer from.
    fn answer<T: Send + 'static>(
        &self,
        asked: Asked,
        answer: impl FnOnce(&mut State) -> T,
    ) -> Answer<T> {
        let mut state = self.0.borrow_mut();
        state.asked.push(asked);
        let answer = if state.signed_out {
            Err(Error::SignedOut)
        } else {
            Ok(answer(&mut state))
        };
        future::ready(answer).boxed()
    }
}

impl Service for Fake {
    fn refresh(&self) -> Answer<Snapshot> {
        self.answer(Asked::Refresh, |state| Snapshot {
            directory: state.directory.clone(),
            token: None,
        })
    }

    fn messages(&self, conversation: &ConversationId, before: Option<i64>) -> Answer<Page> {
        let key = conversation.key();
        self.answer(Asked::Messages(key.clone(), before), |state| {
            let all = state.messages.get(&key).map_or(&[][..], Vec::as_slice);
            let end = before.map_or(all.len(), |before| {
                all.partition_point(|message| message.time < before)
            });
            let start = end.saturating_sub(PAGE);
            Page {
                messages: all[start..end].to_vec(),
                older: (start > 0).then(|| all[start].time),
            }
        })
    }

    fn mark_read(&self, conversation: &ConversationId, time: i64) -> Answer<()> {
        let id = conversation.clone();
        let mut count = None;
        let answer = self.answer(Asked::MarkRead(id.key(), time), |state| {
            let unread = state
                .directory
                .conversations
                .iter_mut()
                .find(|known| known.id == id)
                .map(|known| std::mem::take(&mut known.unread));
            if unread.is_some_and(|unread| unread > 0) {
                state.directory.unread -= 1;
                count = Some(state.directory.unread);
            }
        });
        // Every device hears that it was read, and the count that leaves.
        if let Some(count) = count {
            self.say(Event::Read {
                conversation: conversation.clone(),
                unread: false,
            });
            self.say(Event::Unread(count));
        }
        answer
    }

    fn statuses(&self, people: &[UserId]) -> Answer<HashMap<UserId, Status>> {
        let people = people.to_vec();
        let asked = Asked::Statuses(names(&people));
        self.answer(asked, |state| {
            people
                .into_iter()
                .filter_map(|person| {
                    let status = state.statuses.get(&person)?.clone();
                    Some((person, status))
                })
                .collect()
        })
    }

    fn show(&self, showing: Showing) -> Answer<Showing> {
        self.answer(Asked::Show(showing), |state| {
            state.directory.showing = showing;
            showing
        })
    }

    fn events(&self) -> UnboundedReceiver<Event> {
        let (listener, events) = mpsc::unbounded();
        // The channel says it is up as soon as it is listened to.
        listener
            .unbounded_send(Event::Connected { gap: false })
            .ok();
        self.0.borrow_mut().listeners.push(listener);
        events
    }

    fn watch(&self, people: Vec<UserId>) {
        self.0.borrow_mut().asked.push(Asked::Watch(names(&people)));
    }
}

const MINUTE: i64 = 60_000_000;

/// Now, in microseconds since the Unix epoch.
fn now() -> i64 {
    (crate::session::now() * 1_000_000.) as i64
}

fn message(id: &str, sender: &str, text: &str, time: i64) -> Message {
    Message {
        id: id.into(),
        sender: UserId(sender.into()),
        sender_name: "".into(),
        time,
        kind: MessageKind::Sent,
        text: text.into(),
        attachments: Vec::new(),
        edited: false,
    }
}

fn names(people: &[UserId]) -> Vec<String> {
    let mut names: Vec<String> = people.iter().map(|person| person.0.to_string()).collect();
    names.sort();
    names
}
