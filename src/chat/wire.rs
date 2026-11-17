//! Google Chat's wire messages, declaring only the fields Peep reads or writes.
//! Timestamps are microseconds since the Unix epoch.

use prost::Message;

/// The `RequestHeader` every request carries, already encoded.
///
/// It declares the web client and every capability the web client has. Declaring a
/// partial list makes the server leave out whatever the list omits.
pub fn request_header() -> Vec<u8> {
    const WEB_CLIENT: u8 = 3;
    const FULLY_SUPPORTED: u8 = 2;
    let fields = [5, 6, 8, 9, 10, 11]
        .into_iter()
        .chain(16..=29)
        .chain([32, 33, 37, 38])
        .chain(43..=46)
        .chain([48, 51])
        .chain(53..=56)
        .chain([58])
        .chain(60..=70)
        .chain([72, 73]);

    let mut capabilities = Vec::new();
    for field in fields {
        prost::encoding::encode_varint(field << 3, &mut capabilities);
        capabilities.push(FULLY_SUPPORTED);
    }

    // trace_id = 0, client_type = WEB, client_feature_capabilities = { ... }
    let mut header = vec![0x08, 0x00, 0x10, WEB_CLIENT, 0x2a];
    prost::encoding::encode_varint(capabilities.len() as u64, &mut header);
    header.extend(capabilities);
    header
}

#[derive(Clone, PartialEq, Message)]
pub struct UserId {
    #[prost(string, tag = "1")]
    pub id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct User {
    #[prost(message, optional, tag = "1")]
    pub user_id: Option<UserId>,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub avatar_url: String,
    #[prost(string, tag = "4")]
    pub email: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct DmId {
    #[prost(string, tag = "1")]
    pub dm_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct SpaceId {
    #[prost(string, tag = "1")]
    pub space_id: String,
}

/// A conversation's id: one or the other. A conversation between two people has a DM id;
/// a group conversation is a space.
#[derive(Clone, PartialEq, Message)]
pub struct GroupId {
    #[prost(message, optional, tag = "1")]
    pub space_id: Option<SpaceId>,
    #[prost(message, optional, tag = "3")]
    pub dm_id: Option<DmId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetSelfUserStatusRequest {
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetSelfUserStatusResponse {
    #[prost(message, optional, tag = "1")]
    pub user_status: Option<UserStatus>,
}

#[derive(Clone, PartialEq, Message)]
pub struct UserStatus {
    #[prost(message, optional, tag = "1")]
    pub user_id: Option<UserId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct PaginatedWorldRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub request_header: Vec<u8>,
    #[prost(message, repeated, tag = "2")]
    pub world_section_requests: Vec<WorldSectionRequest>,
    #[prost(bool, tag = "5")]
    pub fetch_from_user_spaces: bool,
    #[prost(bool, tag = "7")]
    pub fetch_snippets_for_unnamed_rooms: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct WorldSectionRequest {
    #[prost(int32, tag = "1")]
    pub page_size: i32,
    #[prost(string, tag = "6")]
    pub pagination_token: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct PaginatedWorldResponse {
    #[prost(message, repeated, tag = "1")]
    pub world_section_responses: Vec<WorldSectionResponse>,
    #[prost(message, repeated, tag = "4")]
    pub world_items: Vec<WorldItem>,
}

#[derive(Clone, PartialEq, Message)]
pub struct WorldSectionResponse {
    #[prost(message, repeated, tag = "2")]
    pub world_items: Vec<WorldItem>,
    #[prost(bool, tag = "5")]
    pub more_items: bool,
    #[prost(string, tag = "6")]
    pub pagination_token: String,
}

/// One conversation in the sidebar.
#[derive(Clone, PartialEq, Message)]
pub struct WorldItem {
    #[prost(message, optional, tag = "1")]
    pub group_id: Option<GroupId>,
    #[prost(int64, tag = "3")]
    pub sort_timestamp: i64,
    #[prost(message, optional, tag = "4")]
    pub read_state: Option<GroupReadState>,
    #[prost(string, tag = "5")]
    pub room_name: String,
    #[prost(message, optional, tag = "6")]
    pub dm_members: Option<Users>,
    /// 6 is a person, 2 an app, 4 a group, 11 Gemini.
    #[prost(int32, tag = "19")]
    pub group_type: i32,
    #[prost(message, optional, tag = "20")]
    pub name_users: Option<NameUsers>,
    #[prost(message, repeated, tag = "36")]
    pub attributes: Vec<Attribute>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Attribute {
    /// `GROUP_DM` marks a space that is a group conversation; `SPACE` a named space.
    #[prost(string, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Users {
    #[prost(message, repeated, tag = "1")]
    pub members: Vec<UserId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct NameUsers {
    #[prost(message, repeated, tag = "1")]
    pub users: Vec<UserId>,
    #[prost(string, tag = "3")]
    pub group_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct GroupReadState {
    #[prost(int64, tag = "2")]
    pub last_read_time: i64,
    #[prost(int64, tag = "4")]
    pub unread_message_count: i64,
    #[prost(int64, tag = "7")]
    pub unread_subscribed_topic_count: i64,
    #[prost(int64, tag = "9")]
    pub hide_timestamp: i64,
    #[prost(message, optional, tag = "12")]
    pub notification_settings: Option<NotificationSettings>,
    #[prost(int64, tag = "18")]
    pub mark_as_unread_timestamp: i64,
    #[prost(int64, tag = "29")]
    pub last_head_message_create_time: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct NotificationSettings {
    #[prost(message, optional, tag = "3")]
    pub mute_settings: Option<MuteSettings>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MuteSettings {
    /// 2 is muted.
    #[prost(int32, tag = "1")]
    pub state: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetMembersRequest {
    #[prost(message, repeated, tag = "2")]
    pub membership_ids: Vec<MembershipId>,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MembershipId {
    #[prost(message, optional, tag = "1")]
    pub member_id: Option<MemberId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MemberId {
    #[prost(message, optional, tag = "1")]
    pub user_id: Option<UserId>,
}

/// The server has answered in either field over time, so Peep reads both.
#[derive(Clone, PartialEq, Message)]
pub struct GetMembersResponse {
    #[prost(message, repeated, tag = "1")]
    pub members: Vec<Member>,
    #[prost(message, repeated, tag = "2")]
    pub member_profiles: Vec<MemberProfile>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MemberProfile {
    #[prost(message, optional, tag = "2")]
    pub member: Option<Member>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Member {
    #[prost(message, optional, tag = "1")]
    pub user: Option<User>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, Message)]
    struct Header {
        #[prost(int64, tag = "1")]
        trace_id: i64,
        #[prost(int32, tag = "2")]
        client_type: i32,
        #[prost(bytes = "vec", tag = "5")]
        capabilities: Vec<u8>,
    }

    #[test]
    fn the_request_header_declares_the_web_client_and_its_capabilities() {
        let header = Header::decode(request_header().as_slice()).unwrap();
        assert_eq!((header.trace_id, header.client_type), (0, 3));

        let mut fields = Vec::new();
        let mut rest = header.capabilities.as_slice();
        while !rest.is_empty() {
            let key = prost::encoding::decode_varint(&mut rest).unwrap();
            let value = prost::encoding::decode_varint(&mut rest).unwrap();
            assert_eq!(
                (key & 7, value),
                (0, 2),
                "a varint field set to fully supported"
            );
            fields.push(key >> 3);
        }
        assert_eq!(fields.len(), 48);
        assert_eq!((fields[0], fields[47]), (5, 73));
        assert!(fields.contains(&17), "17 opens the history of some DMs");
        assert!(!fields.contains(&7) && !fields.contains(&71));
    }
}
