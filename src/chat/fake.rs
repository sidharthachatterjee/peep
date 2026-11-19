//! A Google Chat that tests fill in and listen to, in place of Google's.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use futures::{
    FutureExt as _,
    channel::mpsc::{self, UnboundedReceiver, UnboundedSender},
    future,
};

use super::{
    Answer, Conversation, ConversationId, Directory, Draft, Error, Event, Message, MessageKind,
    Page, Service, Showing, Snapshot, Status, User, UserId,
};

/// How many messages a page holds, unless a test says otherwise.
const PAGE: usize = 3;

/// Something Peep asked of the service.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Asked {
    Refresh,
    RenewPeople,
    Messages(String, Option<i64>),
    MarkRead(String, i64),
    Statuses(Vec<String>),
    Show(Showing),
    /// The people watched, then the conversations whose typing is.
    Watch(Vec<String>, Vec<String>),
    Send(String, Draft),
    Typing(String),
    People(String),
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
    /// Everyone in the organisation who is not in a conversation, by name.
    colleagues: Vec<String>,
    /// Whether asking is refused, as when Google has signed the person out.
    signed_out: bool,
    /// Whether messages are refused.
    refusing_messages: bool,
    /// A person's new name, found when everyone is looked up again.
    renamed: Option<(UserId, String)>,
    listeners: Vec<UnboundedSender<Event>>,
    asked: Vec<Asked>,
    /// When the newest things happen, if not now.
    now: Option<i64>,
    /// How many messages a page holds, if not [`PAGE`].
    page: Option<usize>,
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
            state.messages.insert(id.key(), messages);
        }
        self
    }

    /// Mutes the conversation with this key.
    pub(crate) fn mute(self, conversation: &str) -> Self {
        let mut state = self.0.borrow_mut();
        let known = state.directory.conversations.iter_mut();
        for known in known.filter(|known| known.id.has_key(conversation)) {
            known.muted = true;
        }
        drop(state);
        self
    }

    /// Adds someone to the organisation who is in no conversation. Their id is their name
    /// in lower case.
    pub(crate) fn colleague(self, name: &str) -> Self {
        self.0.borrow_mut().colleagues.push(name.to_owned());
        self
    }

    /// Gives messages out this many at a time.
    pub(crate) fn pages_of(self, messages: usize) -> Self {
        self.0.borrow_mut().page = Some(messages);
        self
    }

    /// Changes one message of a conversation, counted from its oldest.
    pub(crate) fn with_message(
        self,
        conversation: &str,
        index: usize,
        change: impl FnOnce(&mut Message),
    ) -> Self {
        let mut state = self.0.borrow_mut();
        let messages = state.messages.get_mut(conversation);
        change(&mut messages.expect("a conversation the service has")[index]);
        drop(state);
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

    /// Gives a person another name, which shows once everyone is looked up again.
    pub(crate) fn renamed(self, person: &str, name: &str) -> Self {
        self.0.borrow_mut().renamed = Some((UserId(person.into()), name.to_owned()));
        self
    }

    /// From now on messages are not taken.
    pub(crate) fn refuse_messages(&self) {
        self.0.borrow_mut().refusing_messages = true;
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

    fn renew_people(&self) -> Answer<bool> {
        self.answer(Asked::RenewPeople, |state| {
            let Some((person, name)) = state.renamed.take() else {
                return false;
            };
            if let Some(user) = state.directory.users.get_mut(&person) {
                user.name = name.as_str().into();
            }
            let known = state.directory.conversations.iter_mut();
            for conversation in known.filter(|known| known.members == [person.clone()]) {
                conversation.title = name.as_str().into();
            }
            true
        })
    }

    fn messages(&self, conversation: &ConversationId, before: Option<i64>) -> Answer<Page> {
        let key = conversation.key();
        self.answer(Asked::Messages(key.clone(), before), |state| {
            let all = state.messages.get(&key).map_or(&[][..], Vec::as_slice);
            let end = before.map_or(all.len(), |before| {
                all.partition_point(|message| message.time < before)
            });
            let start = end.saturating_sub(state.page.unwrap_or(PAGE));
            Page {
                messages: all[start..end].to_vec(),
                older: (start > 0).then(|| all[start].time),
            }
        })
    }

    fn mark_read(&self, conversation: &ConversationId, time: i64) -> Answer<()> {
        let id = conversation.clone();
        let mut was_unread = false;
        let answer = self.answer(Asked::MarkRead(id.key(), time), |state| {
            let known = state
                .directory
                .conversations
                .iter_mut()
                .find(|known| known.id == id);
            was_unread = known.is_some_and(|known| std::mem::take(&mut known.unread) > 0);
        });
        // Every device hears that it was read.
        if was_unread {
            self.say(Event::Read {
                conversation: conversation.clone(),
                unread: false,
            });
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

    fn send(&self, conversation: &ConversationId, draft: &Draft, id: &str) -> Answer<Message> {
        let key = conversation.key();
        if self.0.borrow().refusing_messages {
            let asked = Asked::Send(key, draft.clone());
            self.0.borrow_mut().asked.push(asked);
            return future::ready(Err(Error::Refused(403))).boxed();
        }
        let mut sent = None;
        let answer = self.answer(Asked::Send(key.clone(), draft.clone()), |state| {
            let now = state.now.unwrap_or_else(now);
            let kept = state.messages.entry(key).or_default();
            // The same id sends the same message once.
            match kept.iter().find(|message| &*message.id == id) {
                Some(message) => message.clone(),
                None => {
                    let message = message(id, "me", &draft.text, now);
                    kept.push(message.clone());
                    sent = Some(message.clone());
                    message
                }
            }
        });
        // Every device hears of it, this one too.
        if let Some(message) = sent {
            self.say(Event::Message {
                conversation: conversation.clone(),
                message,
                posted: true,
            });
        }
        answer
    }

    fn typing(&self, conversation: &ConversationId) -> Answer<()> {
        self.answer(Asked::Typing(conversation.key()), |_| ())
    }

    fn people(&self, typed: &str) -> Answer<Vec<(UserId, User)>> {
        let typed = typed.to_lowercase();
        self.answer(Asked::People(typed.clone()), |state| {
            let known = state.directory.users.iter();
            let known = known.map(|(id, user)| (id.clone(), user.clone()));
            let others = state.colleagues.iter().map(|name| {
                let id = name.to_lowercase();
                let user = User {
                    name: name.as_str().into(),
                    email: format!("{id}@example.com").into(),
                    photo: "".into(),
                };
                (UserId(id.into()), user)
            });
            let mut found: Vec<(UserId, User)> = known
                .chain(others)
                .filter(|(_, user)| user.name.to_lowercase().starts_with(&typed))
                .collect();
            found.sort_by(|one, other| one.1.name.cmp(&other.1.name));
            found
        })
    }

    fn watch(&self, people: Vec<UserId>, typing_in: Vec<ConversationId>) {
        let mut conversations: Vec<String> = typing_in.iter().map(ConversationId::key).collect();
        conversations.sort();
        let asked = Asked::Watch(names(&people), conversations);
        self.0.borrow_mut().asked.push(asked);
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
        marks: super::marks(text, &[]),
        reactions: Vec::new(),
        quote: None,
    }
}

fn names(people: &[UserId]) -> Vec<String> {
    let mut names: Vec<String> = people.iter().map(|person| person.0.to_string()).collect();
    names.sort();
    names
}
