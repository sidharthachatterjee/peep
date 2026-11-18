//! Google Chat: the people, direct messages and messages of the signed-in account.

mod client;
#[cfg(test)]
pub(crate) mod fake;
mod live;
mod service;
mod wire;

use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

pub use client::{Client, Error};
pub use service::{Answer, Service};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Arc<str>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub name: Arc<str>,
    pub email: Arc<str>,
    /// Where their photo is, as Google gives it. It may be empty.
    pub photo: Arc<str>,
}

/// Whether a person is around.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Availability {
    /// Using Google Chat or Gmail now.
    Active,
    /// Has it open but has not used it for a while.
    Idle,
    #[default]
    Away,
}

/// What Google Chat shows beside a person's name: whether they are around, unless they
/// asked not to be disturbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Active,
    Idle,
    Away,
    DoNotDisturb,
}

/// What the signed-in person chose to show others. The three exclude each other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Showing {
    /// Whether they are around, as it is.
    #[default]
    Automatic,
    /// Not to be disturbed until `until`, in microseconds since the Unix epoch.
    DoNotDisturb { until: i64 },
    /// Away, whether or not they are around.
    Away,
}

impl Showing {
    /// What is in effect at `now`: do not disturb ends by itself.
    pub fn at(self, now: i64) -> Self {
        match self {
            Showing::DoNotDisturb { until } if until <= now => Showing::Automatic,
            showing => showing,
        }
    }

    fn from_wire(status: &wire::UserStatus) -> Self {
        let quiet = status
            .dnd_settings
            .as_ref()
            .filter(|settings| settings.dnd_state == DO_NOT_DISTURB);
        match quiet {
            Some(settings) => Showing::DoNotDisturb {
                until: settings.dnd_expiry_time_usec,
            },
            None if status.presence_shared == Some(false) => Showing::Away,
            None => Showing::Automatic,
        }
    }
}

/// How a person is doing right now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub availability: Availability,
    /// Whether they asked not to be disturbed.
    pub quiet: bool,
    /// What they say they are doing, such as "In a meeting", with its emoji in front. Empty
    /// when they say nothing.
    pub note: Arc<str>,
}

impl Status {
    pub fn presence(&self) -> Presence {
        match (self.quiet, self.availability) {
            (true, _) => Presence::DoNotDisturb,
            (false, Availability::Active) => Presence::Active,
            (false, Availability::Idle) => Presence::Idle,
            (false, Availability::Away) => Presence::Away,
        }
    }
}

/// Google Chat keeps a conversation between two people as a DM and a group conversation
/// as a space, each with its own kind of id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConversationId {
    Dm(Arc<str>),
    Space(Arc<str>),
}

impl ConversationId {
    /// A string that names this conversation among all of them.
    pub fn key(&self) -> String {
        let (kind, id) = self.parts();
        format!("{kind}/{id}")
    }

    /// Whether `key` is this conversation's [`key`](Self::key).
    pub fn has_key(&self, key: &str) -> bool {
        let (kind, id) = self.parts();
        key.strip_prefix(kind)
            .and_then(|rest| rest.strip_prefix('/'))
            == Some(id)
    }

    fn parts(&self) -> (&'static str, &str) {
        match self {
            ConversationId::Dm(id) => ("dm", id),
            ConversationId::Space(id) => ("space", id),
        }
    }
}

/// A direct message: a conversation with one person or a fixed group of people.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    /// What to call it: its given name, or the names of the people in it.
    pub title: Arc<str>,
    /// The people it is named after, without the signed-in person.
    pub members: Vec<UserId>,
    /// A name the group was given, if any.
    pub name: Option<Arc<str>>,
    /// Messages not yet read. When the server gives no count, 1 stands for "some".
    pub unread: u32,
    pub muted: bool,
    /// When it last had activity, in microseconds since the Unix epoch.
    pub sort_time: i64,
}

/// Everything the conversation list shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Directory {
    /// The signed-in person.
    pub me: Option<UserId>,
    /// What they chose to show others.
    pub showing: Showing,
    /// Direct messages, most recently active first.
    pub conversations: Vec<Conversation>,
    pub users: HashMap<UserId, User>,
}

/// One message in a conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub id: Arc<str>,
    pub sender: UserId,
    /// The sender's name as the message carries it, which may be empty.
    pub sender_name: Arc<str>,
    /// When it was sent, in microseconds since the Unix epoch.
    pub time: i64,
    pub kind: MessageKind,
    pub text: Arc<str>,
    /// The names of attached files.
    pub attachments: Vec<Arc<str>>,
    pub edited: bool,
    /// The parts of the text that are links or name someone, in order.
    pub marks: Vec<Mark>,
    /// The emoji people answered it with.
    pub reactions: Vec<Reaction>,
    /// The message it answers, if it quotes one.
    pub quote: Option<Quote>,
}

/// A part of a message's text that is more than text.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// Where it is in the text, in bytes.
    pub range: std::ops::Range<usize>,
    pub kind: MarkKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MarkKind {
    /// Goes to this address when clicked.
    Link(Arc<str>),
    /// Names someone.
    Mention,
}

/// An emoji and how many people answered a message with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Reaction {
    /// The emoji itself, or the `:name:` of one of the organisation's own.
    pub emoji: Arc<str>,
    pub count: u32,
    /// Whether the signed-in person is one of them.
    pub mine: bool,
}

/// A message as it was when another quoted it.
#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub sender: UserId,
    /// The sender's name as the quote carries it, which may be empty.
    pub sender_name: Arc<str>,
    pub text: Arc<str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// Written by a person or an app.
    Sent,
    /// From Google Chat itself, such as someone joining.
    Notice,
    /// Deleted, with a placeholder left where it was.
    Deleted,
}

/// A message as written, before it is sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub text: String,
    /// The people named in it, each where their name is in the text.
    pub mentions: Vec<Mention>,
}

/// Someone named in a message, so that they are told about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Mention {
    pub person: UserId,
    /// Where their name is in the text, `@` included, in bytes.
    pub range: std::ops::Range<usize>,
}

/// An id for a message about to be sent, made the way Google Chat's own client makes them:
/// eleven characters that no other message will have.
pub fn new_id() -> String {
    use base64::Engine as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    static MADE: AtomicU64 = AtomicU64::new(0);
    let mut hash = sha1_smol::Sha1::new();
    hash.update(&crate::session::now().to_bits().to_le_bytes());
    hash.update(&std::process::id().to_le_bytes());
    hash.update(&MADE.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&hash.digest().bytes()[..8])
}

/// A run of a conversation's messages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    /// Oldest first.
    pub messages: Vec<Message>,
    /// What to pass as `before` for the page before this one. `None` at the conversation's
    /// start.
    pub older: Option<i64>,
}

/// Something the live channel reported.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The channel is up. With `gap` it could not pick up where it left off, so what
    /// happened in between is not known.
    Connected { gap: bool },
    /// The channel dropped. It reconnects by itself.
    Disconnected,
    /// Google no longer accepts the session.
    SignedOut,
    /// A message was posted, or one already posted changed: edited, or given a preview of
    /// a link in it.
    Message {
        conversation: ConversationId,
        message: Message,
        /// Whether it is new, rather than a change to one already posted.
        posted: bool,
    },
    /// A message was deleted, leaving nothing in its place.
    Removed {
        conversation: ConversationId,
        message: Arc<str>,
    },
    /// A conversation was read or marked unread, here or on another device.
    Read {
        conversation: ConversationId,
        unread: bool,
    },
    /// A person changed their status settings. Whether they are around is not said.
    Status {
        person: UserId,
        /// What they chose to show, which matters when they are the signed-in person.
        showing: Showing,
        /// Whether they asked not to be disturbed.
        quiet: bool,
        /// What they say they are doing, or nothing.
        note: Arc<str>,
    },
    /// Whether a person is around changed. Only people the channel was asked to
    /// [watch](Client::watch) are reported.
    Availability {
        person: UserId,
        availability: Availability,
    },
    /// The reactions to a message changed. These are all it has now.
    Reactions {
        conversation: ConversationId,
        message: Arc<str>,
        reactions: Vec<Reaction>,
    },
    /// Someone started typing in a conversation, or stopped. Google says they are typing
    /// every few seconds for as long as they are, and often does not say they stopped. Only
    /// conversations the channel was asked to [watch](Client::watch) are reported.
    Typing {
        conversation: ConversationId,
        person: UserId,
        typing: bool,
    },
    /// The conversations themselves changed: one was added, left, renamed, muted or hidden.
    ListChanged,
    /// Anything else, by Google's number for it.
    Other(i32),
}

/// What Peep keeps between launches so the list shows at once and the first request
/// needs no preparation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub directory: Directory,
    pub token: Option<Token>,
}

/// The token that proves requests come from the signed-in session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub value: String,
    /// When Google issued it, in seconds since the Unix epoch.
    pub issued: f64,
}

impl Directory {
    /// Gives every conversation its title, now that the people in it are known.
    fn name_conversations(&mut self) {
        for conversation in &mut self.conversations {
            let names: Vec<&str> = conversation
                .members
                .iter()
                .filter_map(|id| self.users.get(id))
                .map(|user| {
                    if user.name.is_empty() {
                        &*user.email
                    } else {
                        &*user.name
                    }
                })
                .filter(|name| !name.is_empty())
                .collect();
            conversation.title = match &conversation.name {
                Some(name) => name.clone(),
                None if names.is_empty() => UNNAMED.into(),
                None => names.join(", ").into(),
            };
        }
    }
}

/// The title of a conversation whose people the server did not name.
const UNNAMED: &str = "Direct message";
const APP: i32 = 2;
const GEMINI: i32 = 11;
const MUTED: i32 = 2;
const GROUP_CONVERSATION: &str = "GROUP_DM";

/// Maps one sidebar item to a direct message. Named spaces, apps and hidden
/// conversations give `None`.
fn conversation(item: wire::WorldItem, me: &UserId) -> Option<Conversation> {
    let group = item.group_id?;
    let is_group = item
        .attributes
        .iter()
        .any(|attribute| attribute.value == GROUP_CONVERSATION);
    let id = match (group.dm_id, group.space_id) {
        (Some(dm), _) if ![APP, GEMINI].contains(&item.group_type) => {
            ConversationId::Dm(dm.dm_id.into())
        }
        (None, Some(space)) if is_group => ConversationId::Space(space.space_id.into()),
        _ => return None,
    };
    let read = item.read_state.unwrap_or_default();
    if read.hide_timestamp > 0 {
        return None;
    }

    let name_users = item.name_users.unwrap_or_default();
    let mut members: Vec<UserId> = Vec::new();
    for user in item
        .dm_members
        .unwrap_or_default()
        .members
        .into_iter()
        .chain(name_users.users)
    {
        let user = UserId(user.id.into());
        if !user.0.is_empty() && user != *me && !members.contains(&user) {
            members.push(user);
        }
    }

    let name = [item.room_name, name_users.group_name]
        .into_iter()
        .find(|name| !name.is_empty())
        .map(Arc::from);

    let unread = if read.unread_message_count > 0 {
        read.unread_message_count
    } else if read.last_head_message_create_time > read.last_read_time
        || read.mark_as_unread_timestamp > 0
    {
        read.unread_subscribed_topic_count.max(1)
    } else {
        0
    };

    let muted = read
        .notification_settings
        .and_then(|settings| settings.mute_settings)
        .is_some_and(|mute| mute.state == MUTED);

    Some(Conversation {
        id,
        title: UNNAMED.into(),
        members,
        name,
        unread: u32::try_from(unread).unwrap_or(u32::MAX),
        muted,
        sort_time: item.sort_timestamp,
    })
}

/// Google's numbers for the kinds of event Peep acts on.
mod event {
    pub const GROUP_VIEWED: i32 = 3;
    pub const MESSAGE_POSTED: i32 = 6;
    pub const MESSAGE_UPDATED: i32 = 7;
    pub const MESSAGE_DELETED: i32 = 8;
    pub const STATUS_UPDATED: i32 = 25;
    pub const TYPING_CHANGED: i32 = 29;
    pub const REACTIONS_UPDATED: i32 = 58;
    pub const AVAILABILITY_UPDATED: i32 = 45;
    pub const MARKED_UNREAD: i32 = 37;
    pub const READ_STATE_UPDATED: i32 = 83;
    /// Added to or removed from a conversation, a conversation updated, a membership
    /// changed, hidden or shown, notifications changed, deleted, the whole list refreshed.
    pub const LIST: [i32; 8] = [1, 2, 5, 15, 16, 18, 30, 76];
}

impl ConversationId {
    fn from_wire(id: wire::GroupId) -> Option<Self> {
        match (id.dm_id, id.space_id) {
            (Some(dm), _) if !dm.dm_id.is_empty() => Some(Self::Dm(dm.dm_id.into())),
            (_, Some(space)) if !space.space_id.is_empty() => {
                Some(Self::Space(space.space_id.into()))
            }
            _ => None,
        }
    }
}

/// What one report from the live channel means. A report can carry several events.
fn events(report: wire::Event) -> Vec<Event> {
    let group = report.group_id.and_then(ConversationId::from_wire);
    let first = report.body.map(|mut body| {
        if body.kind == 0 {
            body.kind = report.kind;
        }
        body
    });
    first
        .into_iter()
        .chain(report.bodies)
        .map(|body| heard(body, group.clone()))
        .collect()
}

fn heard(body: wire::EventBody, group: Option<ConversationId>) -> Event {
    let within =
        |id: Option<wire::GroupId>| id.and_then(ConversationId::from_wire).or(group.clone());
    let event =
        match body.kind {
            event::MESSAGE_POSTED | event::MESSAGE_UPDATED => body
                .message
                .and_then(|event| event.message)
                .and_then(|message| {
                    let id = message.id.clone()?;
                    let conversation = within(
                        id.parent_id
                            .and_then(|parent| parent.topic_id)
                            .and_then(|topic| topic.group_id),
                    )?;
                    Some(match self::message(message) {
                        Some(message) => Event::Message {
                            conversation,
                            message,
                            posted: body.kind == event::MESSAGE_POSTED,
                        },
                        None => Event::Removed {
                            conversation,
                            message: id.message_id.into(),
                        },
                    })
                }),
            event::MESSAGE_DELETED => body
                .message_deleted
                .and_then(|deleted| deleted.message_id)
                .and_then(|id| {
                    let conversation = within(
                        id.parent_id
                            .and_then(|parent| parent.topic_id)
                            .and_then(|topic| topic.group_id),
                    )?;
                    Some(Event::Removed {
                        conversation,
                        message: id.message_id.into(),
                    })
                }),
            event::GROUP_VIEWED => within(body.group_viewed.and_then(|viewed| viewed.group_id))
                .map(|conversation| Event::Read {
                    conversation,
                    unread: false,
                }),
            event::MARKED_UNREAD => within(body.marked_unread.and_then(|marked| marked.group_id))
                .map(|conversation| Event::Read {
                    conversation,
                    unread: true,
                }),
            event::READ_STATE_UPDATED => body.read_state.and_then(|state| {
                let marked = state.mark_as_unread_time > 0;
                // A message newer than the last one read is reported as a message too,
                // which is what counts it.
                if !marked && state.last_head_message_create_time > state.most_recent_read_time {
                    return None;
                }
                Some(Event::Read {
                    conversation: within(state.group_id)?,
                    unread: marked,
                })
            }),
            event::STATUS_UPDATED => body
                .user_status
                .and_then(|updated| updated.user_status)
                .and_then(|status| {
                    let person = status.user_id.clone().filter(|id| !id.id.is_empty())?;
                    let now = (crate::session::now() * 1_000_000.) as i64;
                    let showing = Showing::from_wire(&status).at(now);
                    let (quiet, note) = settings(Some(status), now);
                    Some(Event::Status {
                        person: UserId(person.id.into()),
                        showing,
                        quiet,
                        note,
                    })
                }),
            event::AVAILABILITY_UPDATED => body.availability.and_then(|changed| {
                let person = changed.user_id.filter(|id| !id.id.is_empty())?;
                Some(Event::Availability {
                    person: UserId(person.id.into()),
                    availability: availability(changed.state)?,
                })
            }),
            event::REACTIONS_UPDATED => body.reactions.and_then(|updated| {
                let id = updated.message_id?;
                let conversation = within(
                    id.parent_id
                        .and_then(|parent| parent.topic_id)
                        .and_then(|topic| topic.group_id),
                )?;
                let reactions = updated.reaction_summaries.into_iter();
                Some(Event::Reactions {
                    conversation,
                    message: id.message_id.into(),
                    reactions: reactions
                        .filter_map(|one| {
                            let count = u32::try_from(one.count).unwrap_or(0);
                            reaction(one.emoji, count, one.current_user_reacted)
                        })
                        .collect(),
                })
            }),
            event::TYPING_CHANGED => body.typing.and_then(|changed| {
                let person = changed.user_id.filter(|id| !id.id.is_empty())?;
                let context = changed.context?;
                // Typing in a thread's own box is not typing in the conversation's.
                let conversation = within(context.group_id.filter(|_| context.topic_id.is_none()))?;
                Some(Event::Typing {
                    conversation,
                    person: UserId(person.id.into()),
                    typing: changed.state == TYPING,
                })
            }),
            kind if event::LIST.contains(&kind) => Some(Event::ListChanged),
            _ => None,
        };
    event.unwrap_or(Event::Other(body.kind))
}

const DO_NOT_DISTURB: i32 = 2;
/// Google's number for someone typing, as opposed to having stopped.
const TYPING: i32 = 1;
/// Google's numbers for an annotation that mentions someone, and for a plain mention.
const USER_MENTION: i32 = 6;
const MENTION: i32 = 3;

/// How `mention` goes to Google: where the name is, counted in UTF-16 code units.
fn mention(text: &str, mention: &Mention) -> wire::Annotation {
    let units = |bytes: &str| i32::try_from(bytes.encode_utf16().count()).unwrap_or(i32::MAX);
    let person = wire::UserId {
        id: mention.person.0.to_string(),
    };
    wire::Annotation {
        kind: USER_MENTION,
        start_index: units(&text[..mention.range.start]),
        length: units(&text[mention.range.clone()]),
        user_mention: Some(wire::UserMention {
            id: Some(person.clone()),
            kind: MENTION,
            invitee_info: Some(wire::InviteeInfo {
                user_id: Some(person),
            }),
        }),
        ..Default::default()
    }
}
/// Whether a person is around, from Google's number for it. `None` when Google does not
/// say.
fn availability(state: i32) -> Option<Availability> {
    match state {
        1 => Some(Availability::Active),
        2 => Some(Availability::Away),
        3 => Some(Availability::Idle),
        _ => None,
    }
}

/// What a person's status settings say: whether they are not to be disturbed, and their note.
fn settings(status: Option<wire::UserStatus>, now: i64) -> (bool, Arc<str>) {
    let status = status.unwrap_or_default();
    let quiet = status
        .dnd_settings
        .is_some_and(|settings| settings.dnd_state == DO_NOT_DISTURB);
    let note = status
        .custom_status
        .filter(|custom| {
            custom.state_expiry_timestamp_usec == 0 || custom.state_expiry_timestamp_usec > now
        })
        .map(|custom| {
            let emoji = custom.emoji.unwrap_or_default().unicode;
            [emoji.as_str(), custom.status_text.trim()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    (quiet, note.into())
}

/// One person's status as `get_user_presence` reports it, at `now` in microseconds.
fn status(presence: wire::UserPresence, now: i64) -> Option<(UserId, Status)> {
    let id = presence.user_id.filter(|id| !id.id.is_empty())?;
    let (quiet, note) = settings(presence.user_status, now);
    let status = Status {
        availability: availability(presence.presence).unwrap_or_default(),
        quiet: quiet || presence.dnd_state == DO_NOT_DISTURB,
        note,
    };
    Some((UserId(id.id.into()), status))
}

const NOTICE: i32 = 2;
const VIDEO_CALL: i32 = 11;

/// The messages of one reply to `list_topics`, asked for before `before`.
fn page(response: wire::ListTopicsResponse, before: Option<i64>) -> Page {
    let oldest = response
        .topics
        .iter()
        .map(|topic| topic.sort_time)
        .filter(|time| *time > 0)
        .min();
    let mut messages: Vec<Message> = response
        .topics
        .into_iter()
        .flat_map(|topic| topic.replies)
        .filter_map(message)
        .collect();
    messages.sort_by(|a, b| a.time.cmp(&b.time).then_with(|| a.id.cmp(&b.id)));
    messages.dedup_by(|a, b| a.id == b.id);

    // A page that does not reach further back than what was asked for would repeat forever.
    let older = oldest.filter(|oldest| {
        !response.contains_first_topic && before.is_none_or(|before| *oldest < before)
    });
    Page { messages, older }
}

/// One message as shown, or `None` for one that leaves no trace: deleted without a
/// placeholder, or a notice with nothing to say.
fn message(message: wire::ChatMessage) -> Option<Message> {
    let id = message
        .id
        .filter(|id| !id.message_id.is_empty())?
        .message_id;
    let deleted = message
        .tombstone
        .is_some_and(|tombstone| tombstone.is_tombstone);
    let kind = if deleted {
        MessageKind::Deleted
    } else if message.delete_time > 0 {
        return None;
    } else if message.message_type == NOTICE {
        MessageKind::Notice
    } else {
        MessageKind::Sent
    };
    if kind == MessageKind::Notice && message.text_body.trim().is_empty() {
        return None;
    }
    let has_text = !message.text_body.is_empty();
    let marks = marks(&message.text_body, &message.annotations);
    let reactions = message.reactions.into_iter();
    let reactions = reactions
        .filter_map(|one| {
            let count = u32::try_from(one.count).unwrap_or(0);
            reaction(one.emoji, count, one.current_user_reacted)
        })
        .collect();
    let quote = message.quoted.map(|quoted| {
        let creator = quoted.creator.unwrap_or_default();
        Quote {
            sender: UserId(creator.user_id.unwrap_or_default().id.into()),
            sender_name: creator.name.into(),
            text: quoted.text_body.into(),
        }
    });
    let attachments = message
        .annotations
        .into_iter()
        .filter_map(|annotation| {
            if let Some(upload) = annotation.upload {
                Some(upload.content_name)
            } else if let Some(drive) = annotation.drive {
                Some(drive.title)
            } else if annotation.kind == VIDEO_CALL {
                Some("Video call".to_owned())
            } else if has_text {
                // The text already shows its links.
                None
            } else {
                let link = annotation.link?;
                Some(if link.title.is_empty() {
                    link.url.unwrap_or_default().url
                } else {
                    link.title
                })
            }
        })
        .filter(|name| !name.is_empty())
        .map(Arc::from)
        .collect();
    let creator = message.creator.unwrap_or_default();
    Some(Message {
        id: id.into(),
        sender: UserId(creator.user_id.unwrap_or_default().id.into()),
        sender_name: creator.name.into(),
        time: message.create_time,
        kind,
        text: message.text_body.into(),
        attachments,
        edited: message.last_edit_time > 0,
        marks,
        reactions,
        quote,
    })
}

/// One reaction as shown, or `None` for one no one holds any more or with no emoji to show.
fn reaction(emoji: Option<wire::Emoji>, count: u32, mine: bool) -> Option<Reaction> {
    let emoji = emoji?;
    let emoji = match emoji.custom {
        _ if !emoji.unicode.is_empty() => emoji.unicode,
        Some(custom) if !custom.shortcode.is_empty() => format!(":{}:", custom.shortcode),
        _ => return None,
    };
    (count > 0).then(|| Reaction {
        emoji: emoji.into(),
        count,
        mine,
    })
}

/// The links and the names of people in a message's text: those Google marked, then any
/// web address it did not. Marks are in order and do not overlap.
fn marks(text: &str, annotations: &[wire::Annotation]) -> Vec<Mark> {
    let mut marks: Vec<Mark> = annotations
        .iter()
        .filter_map(|annotation| {
            let range = bytes(text, annotation.start_index, annotation.length)?;
            let kind = if annotation.user_mention.is_some() {
                MarkKind::Mention
            } else {
                let link = annotation.link.as_ref()?;
                let address = link.url.as_ref().map(|url| url.url.as_str());
                // A link without an address of its own goes where its text says.
                let address = address.filter(|address| !address.is_empty());
                MarkKind::Link(address.unwrap_or(&text[range.clone()]).into())
            };
            Some(Mark { range, kind })
        })
        .collect();
    for (start, word) in words(text) {
        let address = word.trim_end_matches(|last: char| ".,;:!?)]}'\"".contains(last));
        let range = start..start + address.len();
        let is_address = ["https://", "http://"]
            .iter()
            .any(|scheme| address.len() > scheme.len() && address.starts_with(scheme));
        let marked = |mark: &Mark| mark.range.start < range.end && range.start < mark.range.end;
        if is_address && !marks.iter().any(marked) {
            marks.push(Mark {
                range,
                kind: MarkKind::Link(address.into()),
            });
        }
    }
    marks.sort_by_key(|mark| mark.range.start);
    // Google sometimes marks the same text twice.
    marks.dedup_by(|later, earlier| later.range.start < earlier.range.end);
    marks
}

/// Each run of text between spaces and line breaks, with where it starts.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_whitespace().map(move |word| {
        // `word` is a slice of `text`, so its address says where it starts.
        (word.as_ptr() as usize - text.as_ptr() as usize, word)
    })
}

/// The bytes of `text` that a run of UTF-16 code units covers, as Google counts them.
/// `None` when it is empty or does not fall on whole characters inside the text.
fn bytes(text: &str, start: i32, length: i32) -> Option<std::ops::Range<usize>> {
    let (start, length) = (usize::try_from(start).ok()?, usize::try_from(length).ok()?);
    if length == 0 {
        return None;
    }
    let mut units = 0;
    let mut range = None..None;
    for (index, character) in text.char_indices().chain([(text.len(), ' ')]) {
        if units == start {
            range.start = Some(index);
        }
        if units == start + length {
            range.end = Some(index);
        }
        units += character.len_utf16();
    }
    Some(range.start?..range.end?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_id(id: &str) -> wire::UserId {
        wire::UserId { id: id.into() }
    }

    fn dm(id: &str, members: &[&str]) -> wire::WorldItem {
        wire::WorldItem {
            group_id: Some(wire::GroupId {
                dm_id: Some(wire::DmId { dm_id: id.into() }),
                space_id: None,
            }),
            dm_members: Some(wire::Users {
                members: members.iter().map(|id| user_id(id)).collect(),
            }),
            group_type: 6,
            ..Default::default()
        }
    }

    fn me() -> UserId {
        UserId("me".into())
    }

    #[test]
    fn a_dm_lists_everyone_but_the_signed_in_person() {
        let mut item = dm("dm-1", &["me", "ada"]);
        item.name_users = Some(wire::NameUsers {
            users: vec![user_id("ada"), user_id("grace")],
            group_name: String::new(),
        });
        item.sort_timestamp = 42;

        let conversation = conversation(item, &me()).unwrap();

        assert_eq!(conversation.id, ConversationId::Dm("dm-1".into()));
        assert_eq!(
            conversation.members,
            [UserId("ada".into()), UserId("grace".into())]
        );
        assert_eq!((conversation.name, conversation.sort_time), (None, 42));
    }

    fn space(id: &str, attribute: &str) -> wire::WorldItem {
        wire::WorldItem {
            group_id: Some(wire::GroupId {
                space_id: Some(wire::SpaceId {
                    space_id: id.into(),
                }),
                dm_id: None,
            }),
            attributes: vec![wire::Attribute {
                value: attribute.into(),
            }],
            group_type: 4,
            ..Default::default()
        }
    }

    #[test]
    fn a_group_conversation_is_a_space_marked_as_one() {
        let mut group = space("space-1", "GROUP_DM");
        group.name_users = Some(wire::NameUsers {
            users: vec![user_id("ada"), user_id("me"), user_id("grace")],
            group_name: String::new(),
        });

        let conversation = conversation(group, &me()).unwrap();

        assert_eq!(conversation.id, ConversationId::Space("space-1".into()));
        assert_eq!(
            conversation.members,
            [UserId("ada".into()), UserId("grace".into())]
        );
    }

    #[test]
    fn spaces_apps_and_hidden_conversations_are_left_out() {
        assert_eq!(conversation(wire::WorldItem::default(), &me()), None);
        assert_eq!(conversation(space("space-1", "SPACE"), &me()), None);

        for group_type in [APP, GEMINI] {
            let mut app = dm("dm-app", &["me", "bot"]);
            app.group_type = group_type;
            assert_eq!(conversation(app, &me()), None);
        }

        let mut hidden = dm("dm-hidden", &["me", "ada"]);
        hidden.read_state = Some(wire::GroupReadState {
            hide_timestamp: 1,
            ..Default::default()
        });
        assert_eq!(conversation(hidden, &me()), None);
    }

    #[test]
    fn unread_uses_the_count_or_falls_back_to_timestamps() {
        let unread = |read_state: wire::GroupReadState| {
            let mut item = dm("dm-1", &["me", "ada"]);
            item.read_state = Some(read_state);
            conversation(item, &me()).unwrap().unread
        };
        let state = wire::GroupReadState::default;

        assert_eq!(unread(state()), 0);
        assert_eq!(
            unread(wire::GroupReadState {
                unread_message_count: 3,
                ..state()
            }),
            3
        );
        assert_eq!(
            unread(wire::GroupReadState {
                last_read_time: 10,
                last_head_message_create_time: 11,
                ..state()
            }),
            1,
            "a newer message than the last read one, with no count"
        );
        assert_eq!(
            unread(wire::GroupReadState {
                last_read_time: 11,
                last_head_message_create_time: 11,
                ..state()
            }),
            0
        );
        assert_eq!(
            unread(wire::GroupReadState {
                mark_as_unread_timestamp: 5,
                unread_subscribed_topic_count: 2,
                ..state()
            }),
            2
        );
    }

    #[test]
    fn a_muted_group_keeps_its_given_name() {
        let mut item = dm("dm-1", &["me", "ada", "grace"]);
        item.name_users = Some(wire::NameUsers {
            users: Vec::new(),
            group_name: "Lunch".into(),
        });
        item.read_state = Some(wire::GroupReadState {
            notification_settings: Some(wire::NotificationSettings {
                mute_settings: Some(wire::MuteSettings { state: MUTED }),
            }),
            ..Default::default()
        });

        let conversation = conversation(item, &me()).unwrap();

        assert_eq!(conversation.name.as_deref(), Some("Lunch"));
        assert!(conversation.muted);
    }

    #[test]
    fn a_title_is_the_given_name_or_the_members_names() {
        let people = [
            ("ada", "Ada Lovelace", ""),
            ("grace", "", "grace@example.com"),
        ];
        let mut directory = Directory::default();
        for (id, name, email) in people {
            let id = UserId(id.into());
            directory.users.insert(
                id,
                User {
                    name: name.into(),
                    email: email.into(),
                    photo: "".into(),
                },
            );
        }
        let members = |ids: &[&str]| ids.iter().map(|id| UserId((*id).into())).collect();
        let conversation = |name: Option<&str>, ids: &[&str]| Conversation {
            id: ConversationId::Dm("dm".into()),
            title: "".into(),
            members: members(ids),
            name: name.map(Arc::from),
            unread: 0,
            muted: false,
            sort_time: 0,
        };
        directory.conversations = vec![
            conversation(None, &["ada", "grace", "unknown"]),
            conversation(Some("Lunch"), &["ada"]),
            conversation(None, &["unknown"]),
        ];

        directory.name_conversations();

        let titles: Vec<&str> = directory
            .conversations
            .iter()
            .map(|conversation| &*conversation.title)
            .collect();
        assert_eq!(
            titles,
            ["Ada Lovelace, grace@example.com", "Lunch", "Direct message"]
        );
    }

    #[test]
    fn a_conversation_knows_its_own_key() {
        let dm = ConversationId::Dm("abc".into());
        assert_eq!(dm.key(), "dm/abc");
        assert!(dm.has_key("dm/abc"));
        assert!(!dm.has_key("space/abc") && !dm.has_key("dm/abcd") && !dm.has_key("dmabc"));
        assert!(ConversationId::Space("abc".into()).has_key("space/abc"));
    }

    #[test]
    fn a_snapshot_survives_being_stored() {
        let ada = UserId("ada".into());
        let mut snapshot = Snapshot {
            token: Some(Token {
                value: "token".into(),
                issued: 12.5,
            }),
            ..Default::default()
        };
        snapshot.directory.users.insert(
            ada.clone(),
            User {
                name: "Ada".into(),
                email: "ada@example.com".into(),
                photo: "//lh3.googleusercontent.com/a/photo".into(),
            },
        );
        snapshot.directory.conversations.push(Conversation {
            id: ConversationId::Space("space-1".into()),
            title: "Ada".into(),
            members: vec![ada],
            name: None,
            unread: 2,
            muted: true,
            sort_time: 7,
        });

        let stored = serde_json::to_vec(&snapshot).unwrap();

        assert_eq!(
            serde_json::from_slice::<Snapshot>(&stored).unwrap(),
            snapshot
        );
    }

    fn sent(id: &str, time: i64, text: &str) -> wire::ChatMessage {
        wire::ChatMessage {
            id: Some(wire::MessageId {
                message_id: id.into(),
                parent_id: None,
            }),
            creator: Some(wire::User {
                user_id: Some(user_id("ada")),
                name: "Ada".into(),
                ..Default::default()
            }),
            create_time: time,
            text_body: text.into(),
            ..Default::default()
        }
    }

    fn topic(sort_time: i64, replies: Vec<wire::ChatMessage>) -> wire::Topic {
        wire::Topic { sort_time, replies }
    }

    #[test]
    fn a_page_is_oldest_first_with_thread_replies_in_line() {
        let response = wire::ListTopicsResponse {
            topics: vec![
                topic(30, vec![sent("c", 30, "third")]),
                topic(10, vec![sent("a", 10, "first"), sent("d", 40, "a reply")]),
                topic(20, vec![sent("b", 20, "second"), sent("b", 20, "second")]),
            ],
            contains_first_topic: false,
        };

        let page = page(response, None);

        let texts: Vec<&str> = page.messages.iter().map(|message| &*message.text).collect();
        assert_eq!(texts, ["first", "second", "third", "a reply"]);
        assert_eq!(page.older, Some(10));
        assert_eq!(
            (&*page.messages[0].sender.0, &*page.messages[0].sender_name),
            ("ada", "Ada")
        );
    }

    #[test]
    fn paging_back_stops_at_the_start_or_when_nothing_older_came() {
        let reply = |first| wire::ListTopicsResponse {
            topics: vec![topic(10, vec![sent("a", 10, "first")])],
            contains_first_topic: first,
        };

        assert_eq!(page(reply(true), None).older, None);
        assert_eq!(page(reply(false), Some(11)).older, Some(10));
        assert_eq!(
            page(reply(false), Some(10)).older,
            None,
            "no further back than what was asked for"
        );
        assert_eq!(page(wire::ListTopicsResponse::default(), None).older, None);
    }

    #[test]
    fn deleted_edited_and_attached_messages_are_told_apart() {
        let mut deleted = sent("a", 1, "");
        deleted.delete_time = 5;
        assert_eq!(message(deleted.clone()), None, "gone without a trace");
        deleted.tombstone = Some(wire::Tombstone { is_tombstone: true });
        assert_eq!(message(deleted).unwrap().kind, MessageKind::Deleted);

        let mut notice = sent("b", 2, " ");
        notice.message_type = NOTICE;
        assert_eq!(
            message(notice.clone()),
            None,
            "a notice with nothing to say"
        );
        notice.text_body = "Ada joined".into();
        assert_eq!(message(notice).unwrap().kind, MessageKind::Notice);

        let mut edited = sent("c", 3, "see attached");
        edited.last_edit_time = 4;
        let link = |title: &str| wire::Annotation {
            link: Some(wire::Link {
                title: title.into(),
                url: Some(wire::SafeUrl {
                    url: "https://example.com".into(),
                }),
            }),
            ..Default::default()
        };
        edited.annotations = vec![
            wire::Annotation {
                upload: Some(wire::Upload {
                    content_name: "plan.pdf".into(),
                }),
                ..Default::default()
            },
            wire::Annotation {
                drive: Some(wire::DriveFile {
                    title: "Budget".into(),
                }),
                ..Default::default()
            },
            wire::Annotation {
                kind: VIDEO_CALL,
                ..Default::default()
            },
            link("Shown by the text"),
            wire::Annotation::default(),
        ];
        let edited = message(edited).unwrap();
        assert!(edited.edited);
        let names: Vec<&str> = edited.attachments.iter().map(|name| &**name).collect();
        assert_eq!(names, ["plan.pdf", "Budget", "Video call"]);

        let mut bare = sent("d", 5, "");
        bare.annotations = vec![link("A page"), link("")];
        let names = message(bare).unwrap().attachments;
        let names: Vec<&str> = names.iter().map(|name| &**name).collect();
        assert_eq!(
            names,
            ["A page", "https://example.com"],
            "a link with no text"
        );

        assert_eq!(message(wire::ChatMessage::default()), None, "no id");
    }

    fn group(dm: &str) -> wire::GroupId {
        wire::GroupId {
            dm_id: Some(wire::DmId { dm_id: dm.into() }),
            space_id: None,
        }
    }

    fn body(kind: i32) -> wire::EventBody {
        wire::EventBody {
            kind,
            ..Default::default()
        }
    }

    #[test]
    fn a_report_carries_posted_edited_and_deleted_messages() {
        let dm = ConversationId::Dm("dm-1".into());
        let mut inside = sent("b", 2, "in its own conversation");
        inside.id.as_mut().unwrap().parent_id = Some(wire::MessageParentId {
            topic_id: Some(wire::TopicId {
                group_id: Some(group("dm-2")),
            }),
        });
        let mut gone = sent("c", 3, "");
        gone.delete_time = 4;
        let message = |message| wire::EventBody {
            message: Some(wire::MessageEvent {
                message: Some(message),
            }),
            ..body(event::MESSAGE_POSTED)
        };
        let report = wire::Event {
            group_id: Some(group("dm-1")),
            kind: event::MESSAGE_UPDATED,
            body: Some(wire::EventBody {
                kind: 0,
                ..message(sent("a", 1, "edited"))
            }),
            bodies: vec![
                message(inside),
                message(gone),
                wire::EventBody {
                    message_deleted: Some(wire::MessageDeleted {
                        message_id: Some(wire::MessageId {
                            message_id: "d".into(),
                            parent_id: None,
                        }),
                    }),
                    ..body(event::MESSAGE_DELETED)
                },
            ],
        };

        let heard = events(report);

        assert!(
            matches!(&heard[0], Event::Message { conversation, message, posted: false }
                if *conversation == dm && &*message.text == "edited"),
            "the first body takes the report's kind"
        );
        assert!(
            matches!(&heard[1], Event::Message { conversation, posted: true, .. }
                if *conversation == ConversationId::Dm("dm-2".into())),
            "a message names its own conversation"
        );
        let removed = |id: &str| Event::Removed {
            conversation: dm.clone(),
            message: id.into(),
        };
        assert_eq!(heard[2..], [removed("c"), removed("d")]);
    }

    #[test]
    fn a_report_tells_reading_from_marking_unread_and_from_list_changes() {
        let dm = ConversationId::Dm("dm-1".into());
        let read_state = |head, read, marked| wire::EventBody {
            read_state: Some(wire::ReadStateUpdated {
                group_id: Some(group("dm-1")),
                last_head_message_create_time: head,
                most_recent_read_time: read,
                mark_as_unread_time: marked,
            }),
            ..body(event::READ_STATE_UPDATED)
        };
        let report = wire::Event {
            group_id: Some(group("dm-1")),
            bodies: vec![
                body(event::GROUP_VIEWED),
                body(event::MARKED_UNREAD),
                read_state(5, 5, 0),
                read_state(6, 5, 0),
                read_state(5, 5, 1),
                body(15),
                body(29),
                body(event::MESSAGE_POSTED),
            ],
            ..Default::default()
        };

        let read = |unread| Event::Read {
            conversation: dm.clone(),
            unread,
        };
        assert_eq!(
            events(report),
            [
                read(false),
                read(true),
                read(false),
                Event::Other(event::READ_STATE_UPDATED),
                read(true),
                Event::ListChanged,
                Event::Other(29),
                Event::Other(event::MESSAGE_POSTED),
            ]
        );
    }

    fn custom(text: &str, emoji: &str, expires: i64) -> wire::UserStatus {
        wire::UserStatus {
            user_id: Some(user_id("ada")),
            dnd_settings: None,
            presence_shared: None,
            custom_status: Some(wire::CustomStatus {
                status_text: text.into(),
                state_expiry_timestamp_usec: expires,
                emoji: Some(wire::Emoji {
                    unicode: emoji.into(),
                    custom: None,
                }),
            }),
        }
    }

    #[test]
    fn a_status_says_whether_someone_is_around_or_not_to_be_disturbed() {
        let presence = |presence, dnd_state, user_status| wire::UserPresence {
            user_id: Some(user_id("ada")),
            presence,
            dnd_state,
            user_status,
        };
        let read = |presence| status(presence, 100).unwrap().1;

        let shown = |raw| read(presence(raw, 1, None)).presence();
        assert_eq!(shown(1), Presence::Active);
        assert_eq!(shown(3), Presence::Idle);
        for away in [0, 2, 4] {
            assert_eq!(shown(away), Presence::Away);
        }
        assert_eq!(
            read(presence(1, DO_NOT_DISTURB, None)).presence(),
            Presence::DoNotDisturb
        );
        let mut quiet = custom("", "", 0);
        quiet.dnd_settings = Some(wire::DndSettings {
            dnd_state: DO_NOT_DISTURB,
            dnd_expiry_time_usec: 0,
        });
        let quiet = read(presence(1, 0, Some(quiet)));
        assert_eq!(
            (quiet.presence(), quiet.availability),
            (Presence::DoNotDisturb, Availability::Active),
            "their settings say so too, and they are still around"
        );

        let note = |status| read(presence(1, 1, Some(status))).note;
        assert_eq!(&*note(custom(" In a meeting ", "📅", 0)), "📅 In a meeting");
        assert_eq!(&*note(custom("Lunch", "", 101)), "Lunch");
        assert_eq!(&*note(custom("Lunch", "🍜", 99)), "", "it has run out");

        let mut nameless = presence(1, 1, None);
        nameless.user_id = None;
        assert_eq!(status(nameless, 100), None);
    }

    #[test]
    fn a_report_says_who_came_or_went() {
        let changed = |state| wire::EventBody {
            availability: Some(wire::Availability {
                user_id: Some(user_id("ada")),
                state,
            }),
            ..body(event::AVAILABILITY_UPDATED)
        };
        let report = wire::Event {
            bodies: vec![changed(1), changed(2), changed(3), changed(0)],
            ..Default::default()
        };

        let came_or_went = |availability| Event::Availability {
            person: UserId("ada".into()),
            availability,
        };
        assert_eq!(
            events(report),
            [
                came_or_went(Availability::Active),
                came_or_went(Availability::Away),
                came_or_went(Availability::Idle),
                Event::Other(event::AVAILABILITY_UPDATED),
            ]
        );
    }

    #[test]
    fn a_report_carries_a_persons_changed_status() {
        let mut settings = custom("Focusing", "", 0);
        settings.dnd_settings = Some(wire::DndSettings {
            dnd_state: DO_NOT_DISTURB,
            dnd_expiry_time_usec: i64::MAX,
        });
        let report = wire::Event {
            bodies: vec![
                wire::EventBody {
                    user_status: Some(wire::UserStatusUpdated {
                        user_status: Some(settings),
                    }),
                    ..body(event::STATUS_UPDATED)
                },
                body(event::STATUS_UPDATED),
            ],
            ..Default::default()
        };

        assert_eq!(
            events(report),
            [
                Event::Status {
                    person: UserId("ada".into()),
                    showing: Showing::DoNotDisturb { until: i64::MAX },
                    quiet: true,
                    note: "Focusing".into(),
                },
                Event::Other(event::STATUS_UPDATED),
            ]
        );
    }

    #[test]
    fn a_message_carries_its_links_the_people_it_names_its_reactions_and_what_it_quotes() {
        let text = "😀 see docs and https://example.com/a, @Ada (www.example.org)";
        let units = |part: &str| {
            let start = text.find(part).unwrap();
            let before = text[..start].encode_utf16().count() as i32;
            (before, part.encode_utf16().count() as i32)
        };
        let annotation = |part: &str| {
            let (start_index, length) = units(part);
            wire::Annotation {
                start_index,
                length,
                ..Default::default()
            }
        };
        let emoji = |unicode: &str, shortcode: &str| {
            Some(wire::Emoji {
                unicode: unicode.into(),
                custom: (!shortcode.is_empty()).then(|| wire::CustomEmoji {
                    shortcode: shortcode.into(),
                }),
            })
        };
        let reaction = |emoji, count, current_user_reacted| wire::Reaction {
            emoji,
            count,
            current_user_reacted,
        };
        let sent = wire::ChatMessage {
            text_body: text.into(),
            annotations: vec![
                wire::Annotation {
                    link: Some(wire::Link {
                        title: "Docs".into(),
                        url: Some(wire::SafeUrl {
                            url: "https://docs.example.com".into(),
                        }),
                    }),
                    ..annotation("docs")
                },
                wire::Annotation {
                    user_mention: Some(wire::UserMention::default()),
                    ..annotation("@Ada")
                },
                // A preview of a link covers none of the text.
                wire::Annotation {
                    link: Some(wire::Link::default()),
                    ..Default::default()
                },
            ],
            reactions: vec![
                reaction(emoji("👍", ""), 2, true),
                reaction(emoji("", "engine"), 1, false),
                reaction(emoji("🎉", ""), 0, false),
                reaction(None, 4, false),
            ],
            quoted: Some(wire::QuotedMessage {
                creator: Some(wire::User {
                    user_id: Some(wire::UserId { id: "me".into() }),
                    ..Default::default()
                }),
                text_body: "Where are the docs?".into(),
            }),
            ..sent("m1", 5, "")
        };
        let message = message(sent).unwrap();

        let marked: Vec<(&str, &MarkKind)> = message
            .marks
            .iter()
            .map(|mark| (&text[mark.range.clone()], &mark.kind))
            .collect();
        assert_eq!(
            marked,
            [
                ("docs", &MarkKind::Link("https://docs.example.com".into())),
                (
                    "https://example.com/a",
                    &MarkKind::Link("https://example.com/a".into())
                ),
                ("@Ada", &MarkKind::Mention),
            ],
            "a web address Google did not mark is still a link, without the comma after it"
        );
        let reactions: Vec<(&str, u32, bool)> = message
            .reactions
            .iter()
            .map(|one| (&*one.emoji, one.count, one.mine))
            .collect();
        assert_eq!(reactions, [("👍", 2, true), (":engine:", 1, false)]);
        let quote = message.quote.unwrap();
        assert_eq!(
            (&*quote.sender.0, &*quote.text),
            ("me", "Where are the docs?")
        );
    }

    #[test]
    fn a_report_carries_all_the_reactions_a_message_has_now() {
        let updated = wire::EventBody {
            reactions: Some(wire::ReactionsUpdated {
                message_id: Some(wire::MessageId {
                    message_id: "m1".into(),
                    parent_id: Some(wire::MessageParentId {
                        topic_id: Some(wire::TopicId {
                            group_id: Some(wire::GroupId {
                                dm_id: Some(wire::DmId { dm_id: "d".into() }),
                                space_id: None,
                            }),
                        }),
                    }),
                }),
                reaction_summaries: vec![wire::ReactionSummary {
                    emoji: Some(wire::Emoji {
                        unicode: "👍".into(),
                        custom: None,
                    }),
                    count: 3,
                    current_user_reacted: true,
                }],
            }),
            ..body(event::REACTIONS_UPDATED)
        };
        let report = wire::Event {
            bodies: vec![updated],
            ..Default::default()
        };

        assert_eq!(
            events(report),
            [Event::Reactions {
                conversation: ConversationId::Dm("d".into()),
                message: "m1".into(),
                reactions: vec![Reaction {
                    emoji: "👍".into(),
                    count: 3,
                    mine: true,
                }],
            }]
        );
    }

    #[test]
    fn a_report_says_who_is_typing_in_a_conversation() {
        let typing = |state: i32, in_thread: bool| wire::EventBody {
            typing: Some(wire::TypingStateChanged {
                state,
                user_id: Some(wire::UserId { id: "ada".into() }),
                context: Some(wire::TypingContext {
                    group_id: Some(wire::GroupId {
                        dm_id: Some(wire::DmId { dm_id: "d".into() }),
                        space_id: None,
                    }),
                    topic_id: in_thread.then(wire::TopicId::default),
                }),
            }),
            ..body(event::TYPING_CHANGED)
        };
        let report = wire::Event {
            bodies: vec![typing(1, false), typing(2, false), typing(1, true)],
            ..Default::default()
        };
        let heard = |typing| Event::Typing {
            conversation: ConversationId::Dm("d".into()),
            person: UserId("ada".into()),
            typing,
        };

        assert_eq!(
            events(report),
            [
                heard(true),
                heard(false),
                Event::Other(event::TYPING_CHANGED)
            ],
            "typing a reply in a thread is not typing in the conversation"
        );
    }

    #[test]
    fn a_mention_says_where_the_name_is_as_google_counts() {
        let text = "😀 hi @Ada!";
        let start = text.find('@').unwrap();
        let mention = mention(
            text,
            &Mention {
                person: UserId("ada".into()),
                range: start..start + 4,
            },
        );

        assert_eq!(
            (mention.kind, mention.start_index, mention.length),
            (6, 6, 4)
        );
        let named = mention.user_mention.unwrap();
        assert_eq!(named.kind, 3);
        assert_eq!(named.id.unwrap().id, "ada");
        assert_eq!(named.invitee_info.unwrap().user_id.unwrap().id, "ada");
    }

    #[test]
    fn new_message_ids_look_like_googles_and_differ() {
        let ids: std::collections::HashSet<String> = (0..100).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 100);
        let url_safe = |id: &String| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        };
        assert!(ids.iter().all(url_safe));
    }

    #[test]
    fn what_someone_shows_is_automatic_do_not_disturb_or_away() {
        let status = |dnd: Option<(i32, i64)>, shared: Option<bool>| wire::UserStatus {
            dnd_settings: dnd.map(|(dnd_state, dnd_expiry_time_usec)| wire::DndSettings {
                dnd_state,
                dnd_expiry_time_usec,
            }),
            presence_shared: shared,
            ..Default::default()
        };
        let showing = |status| Showing::from_wire(&status);

        assert_eq!(showing(status(None, None)), Showing::Automatic);
        assert_eq!(
            showing(status(Some((1, 0)), Some(true))),
            Showing::Automatic
        );
        assert_eq!(showing(status(None, Some(false))), Showing::Away);
        assert_eq!(
            showing(status(Some((DO_NOT_DISTURB, 500)), Some(false))),
            Showing::DoNotDisturb { until: 500 }
        );

        let quiet = Showing::DoNotDisturb { until: 500 };
        assert_eq!(quiet.at(499), quiet);
        assert_eq!(quiet.at(500), Showing::Automatic, "it ends by itself");
        assert_eq!(Showing::Away.at(i64::MAX), Showing::Away);
    }
}
