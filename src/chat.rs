//! Google Chat: the people and direct messages of the signed-in account.

mod client;
mod wire;

use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

pub use client::{Client, Error};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Arc<str>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub name: Arc<str>,
    pub email: Arc<str>,
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
        match self {
            ConversationId::Dm(id) => format!("dm/{id}"),
            ConversationId::Space(id) => format!("space/{id}"),
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
    /// Direct messages, most recently active first.
    pub conversations: Vec<Conversation>,
    pub users: HashMap<UserId, User>,
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
}
