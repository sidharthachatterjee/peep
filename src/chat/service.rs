//! What Peep asks of Google Chat, apart from who answers.

use std::collections::HashMap;

use futures::{FutureExt as _, channel::mpsc::UnboundedReceiver, future::BoxFuture};

use super::{
    Client, ConversationId, Draft, Error, Event, Message, Page, Showing, Snapshot, Status, User,
    UserId,
};

/// What comes back from asking, once it has.
pub type Answer<T> = BoxFuture<'static, Result<T, Error>>;

/// Google Chat, as Peep uses it. [`Client`] asks Google; tests answer in its place.
pub trait Service {
    /// The direct messages and the people in them, with what to keep for the next launch.
    fn refresh(&self) -> Answer<Snapshot>;

    /// A page of a conversation's messages: the newest, or those before `before`, which is
    /// the `older` of the page after it.
    fn messages(&self, conversation: &ConversationId, before: Option<i64>) -> Answer<Page>;

    /// Says the person has read `conversation` up to `time`, the time of its newest message.
    fn mark_read(&self, conversation: &ConversationId, time: i64) -> Answer<()>;

    /// How these people are doing right now. People nothing is known about are left out.
    fn statuses(&self, people: &[UserId]) -> Answer<HashMap<UserId, Status>>;

    /// Changes what the signed-in person shows others. Gives what was saved.
    fn show(&self, showing: Showing) -> Answer<Showing>;

    /// Sends a message. `id` is from [`new_id`](super::new_id); sending the same one again
    /// sends nothing more. Gives the message as it was kept.
    fn send(&self, conversation: &ConversationId, draft: &Draft, id: &str) -> Answer<Message>;

    /// Says the signed-in person is typing in `conversation`. It wears off by itself.
    fn typing(&self, conversation: &ConversationId) -> Answer<()>;

    /// The people in the organisation whose name or address starts with `typed`, best first.
    fn people(&self, typed: &str) -> Answer<Vec<(UserId, User)>>;

    /// What happens from now on, as it happens, until the receiver is dropped.
    fn events(&self) -> UnboundedReceiver<Event>;

    /// Asks for [`Event::Availability`] about these people and [`Event::Typing`] in these
    /// conversations from now on, in place of what was asked for before.
    fn watch(&self, people: Vec<UserId>, typing_in: Vec<ConversationId>);
}

impl Service for Client {
    fn refresh(&self) -> Answer<Snapshot> {
        Client::refresh(self).boxed()
    }

    fn messages(&self, conversation: &ConversationId, before: Option<i64>) -> Answer<Page> {
        Client::messages(self, conversation, before).boxed()
    }

    fn mark_read(&self, conversation: &ConversationId, time: i64) -> Answer<()> {
        Client::mark_read(self, conversation, time).boxed()
    }

    fn statuses(&self, people: &[UserId]) -> Answer<HashMap<UserId, Status>> {
        Client::statuses(self, people).boxed()
    }

    fn show(&self, showing: Showing) -> Answer<Showing> {
        Client::show(self, showing).boxed()
    }

    fn send(&self, conversation: &ConversationId, draft: &Draft, id: &str) -> Answer<Message> {
        Client::send(self, conversation, draft, id).boxed()
    }

    fn typing(&self, conversation: &ConversationId) -> Answer<()> {
        Client::typing(self, conversation).boxed()
    }

    fn people(&self, typed: &str) -> Answer<Vec<(UserId, User)>> {
        Client::people(self, typed).boxed()
    }

    fn events(&self) -> UnboundedReceiver<Event> {
        Client::events(self)
    }

    fn watch(&self, people: Vec<UserId>, typing_in: Vec<ConversationId>) {
        Client::watch(self, people, typing_in);
    }
}
