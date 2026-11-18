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
    #[prost(message, optional, tag = "2")]
    pub dnd_settings: Option<DndSettings>,
    /// False when the person set themselves as away.
    #[prost(bool, optional, tag = "5")]
    pub presence_shared: Option<bool>,
    #[prost(message, optional, tag = "6")]
    pub custom_status: Option<CustomStatus>,
}

#[derive(Clone, PartialEq, Message)]
pub struct DndSettings {
    /// 2 is do not disturb.
    #[prost(int32, tag = "1")]
    pub dnd_state: i32,
    /// When do not disturb ends, or 0 when it is off.
    #[prost(int64, tag = "2")]
    pub dnd_expiry_time_usec: i64,
}

/// Turns do not disturb on for a while.
#[derive(Clone, PartialEq, Message)]
pub struct SetDndDurationRequest {
    #[prost(int64, tag = "1")]
    pub duration_usec: i64,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

/// Shares whether the person is around, or shows them as away. Sharing also ends do not
/// disturb.
#[derive(Clone, PartialEq, Message)]
pub struct SetPresenceSharedRequest {
    #[prost(bool, optional, tag = "1")]
    pub presence_shared: Option<bool>,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

/// The status as saved.
#[derive(Clone, PartialEq, Message)]
pub struct SetUserStatusResponse {
    #[prost(message, optional, tag = "1")]
    pub user_status: Option<UserStatus>,
}

/// What a person says they are doing, such as "In a meeting".
#[derive(Clone, PartialEq, Message)]
pub struct CustomStatus {
    #[prost(string, tag = "1")]
    pub status_text: String,
    /// When it stops applying, or 0 for no end.
    #[prost(int64, tag = "3")]
    pub state_expiry_timestamp_usec: i64,
    #[prost(message, optional, tag = "4")]
    pub emoji: Option<Emoji>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Emoji {
    #[prost(string, tag = "1")]
    pub unicode: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetUserPresenceRequest {
    #[prost(message, repeated, tag = "1")]
    pub user_ids: Vec<UserId>,
    #[prost(bool, tag = "3")]
    pub include_user_status: bool,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct GetUserPresenceResponse {
    #[prost(message, repeated, tag = "1")]
    pub user_presences: Vec<UserPresence>,
}

#[derive(Clone, PartialEq, Message)]
pub struct UserPresence {
    #[prost(message, optional, tag = "1")]
    pub user_id: Option<UserId>,
    /// 1 is active, 3 idle. Anything else is away.
    #[prost(int32, tag = "2")]
    pub presence: i32,
    /// 2 is do not disturb.
    #[prost(int32, tag = "3")]
    pub dnd_state: i32,
    #[prost(message, optional, tag = "5")]
    pub user_status: Option<UserStatus>,
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

#[derive(Clone, PartialEq, Message)]
pub struct ListTopicsRequest {
    #[prost(int32, tag = "2")]
    pub page_size_for_topics: i32,
    #[prost(message, optional, tag = "4")]
    pub filter: Option<ListTopicsFilter>,
    /// The web client sends these one by one, not packed.
    #[prost(int32, repeated, packed = "false", tag = "5")]
    pub fetch_options: Vec<i32>,
    #[prost(int32, tag = "6")]
    pub page_size_for_unread_replies: i32,
    #[prost(int32, tag = "7")]
    pub page_size_for_read_replies: i32,
    #[prost(message, optional, tag = "8")]
    pub group_id: Option<GroupId>,
    /// The web client always sends 2. Its meaning is not known.
    #[prost(int32, tag = "11")]
    pub sort_option: i32,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ListTopicsFilter {
    /// Only topics sorted before this time.
    #[prost(int64, optional, tag = "1")]
    pub older_than: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ListTopicsResponse {
    #[prost(message, repeated, tag = "1")]
    pub topics: Vec<Topic>,
    /// Nothing older exists.
    #[prost(bool, tag = "4")]
    pub contains_first_topic: bool,
}

/// A message at the top level of a conversation, with any replies in its thread.
#[derive(Clone, PartialEq, Message)]
pub struct Topic {
    #[prost(int64, tag = "2")]
    pub sort_time: i64,
    #[prost(message, repeated, tag = "7")]
    pub replies: Vec<ChatMessage>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ChatMessage {
    #[prost(message, optional, tag = "1")]
    pub id: Option<MessageId>,
    #[prost(message, optional, tag = "2")]
    pub creator: Option<User>,
    #[prost(int64, tag = "3")]
    pub create_time: i64,
    /// Above zero once the message is deleted.
    #[prost(int64, tag = "8")]
    pub delete_time: i64,
    #[prost(string, tag = "10")]
    pub text_body: String,
    #[prost(message, repeated, tag = "11")]
    pub annotations: Vec<Annotation>,
    /// Above zero once the message is edited.
    #[prost(int64, tag = "17")]
    pub last_edit_time: i64,
    /// 2 is a message from Google Chat itself, such as someone joining.
    #[prost(int32, tag = "28")]
    pub message_type: i32,
    #[prost(message, optional, tag = "31")]
    pub tombstone: Option<Tombstone>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MessageId {
    #[prost(message, optional, tag = "1")]
    pub parent_id: Option<MessageParentId>,
    #[prost(string, tag = "2")]
    pub message_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct MessageParentId {
    #[prost(message, optional, tag = "4")]
    pub topic_id: Option<TopicId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct TopicId {
    #[prost(message, optional, tag = "3")]
    pub group_id: Option<GroupId>,
}

/// Sends a message at the top level of a conversation.
#[derive(Clone, PartialEq, Message)]
pub struct CreateTopicRequest {
    #[prost(string, tag = "2")]
    pub text_body: String,
    #[prost(message, repeated, tag = "3")]
    pub annotations: Vec<Annotation>,
    #[prost(message, optional, tag = "5")]
    pub group_id: Option<GroupId>,
    /// Made up by the sender. Sending the same one twice sends one message.
    #[prost(string, tag = "7")]
    pub topic_and_message_id: String,
    /// The web client always sends true.
    #[prost(bool, tag = "8")]
    pub history_v2: bool,
    #[prost(message, optional, tag = "9")]
    pub message_info: Option<MessageInfo>,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MessageInfo {
    #[prost(bool, tag = "1")]
    pub accept_format_annotations: bool,
}

#[derive(Clone, PartialEq, Message)]
pub struct CreateTopicResponse {
    #[prost(message, optional, tag = "1")]
    pub topic: Option<Topic>,
}

/// Where someone is typing: a conversation's own box, or a thread's.
#[derive(Clone, PartialEq, Message)]
pub struct TypingContext {
    #[prost(message, optional, tag = "1")]
    pub group_id: Option<GroupId>,
    #[prost(message, optional, tag = "2")]
    pub topic_id: Option<TopicId>,
}

/// Says the signed-in person is typing. Nothing says they stopped: it wears off.
#[derive(Clone, PartialEq, Message)]
pub struct SetTypingStateRequest {
    /// 1 is typing.
    #[prost(int32, tag = "1")]
    pub state: i32,
    #[prost(message, optional, tag = "2")]
    pub context: Option<TypingContext>,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SetTypingStateResponse {}

#[derive(Clone, PartialEq, Message)]
pub struct TypingStateChanged {
    /// 1 is typing, 2 stopped.
    #[prost(int32, tag = "1")]
    pub state: i32,
    #[prost(message, optional, tag = "2")]
    pub user_id: Option<UserId>,
    #[prost(message, optional, tag = "3")]
    pub context: Option<TypingContext>,
}

/// Stands where a deleted message was.
#[derive(Clone, PartialEq, Message)]
pub struct Tombstone {
    #[prost(bool, tag = "1")]
    pub is_tombstone: bool,
}

/// Something attached to a message or marked within its text.
#[derive(Clone, PartialEq, Message)]
pub struct Annotation {
    /// 6 is a mention of someone, 11 a video call.
    #[prost(int32, tag = "1")]
    pub kind: i32,
    /// Where in the text it is, in UTF-16 code units.
    #[prost(int32, tag = "2")]
    pub start_index: i32,
    #[prost(int32, tag = "3")]
    pub length: i32,
    #[prost(message, optional, tag = "4")]
    pub drive: Option<DriveFile>,
    #[prost(message, optional, tag = "5")]
    pub user_mention: Option<UserMention>,
    #[prost(message, optional, tag = "7")]
    pub link: Option<Link>,
    #[prost(message, optional, tag = "10")]
    pub upload: Option<Upload>,
}

#[derive(Clone, PartialEq, Message)]
pub struct UserMention {
    #[prost(message, optional, tag = "1")]
    pub id: Option<UserId>,
    /// 3 is a plain mention.
    #[prost(int32, tag = "2")]
    pub kind: i32,
    #[prost(message, optional, tag = "3")]
    pub invitee_info: Option<InviteeInfo>,
}

#[derive(Clone, PartialEq, Message)]
pub struct InviteeInfo {
    #[prost(message, optional, tag = "1")]
    pub user_id: Option<UserId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct DriveFile {
    #[prost(string, tag = "2")]
    pub title: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Link {
    #[prost(string, tag = "1")]
    pub title: String,
    #[prost(message, optional, tag = "7")]
    pub url: Option<SafeUrl>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SafeUrl {
    #[prost(string, tag = "3")]
    pub url: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Upload {
    #[prost(string, tag = "3")]
    pub content_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct MarkGroupReadstateRequest {
    #[prost(message, optional, tag = "1")]
    pub id: Option<GroupId>,
    #[prost(int64, tag = "2")]
    pub last_read_time: i64,
    #[prost(bytes = "vec", tag = "100")]
    pub request_header: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MarkGroupReadstateResponse {}

/// One thing that happened, as the live channel reports it.
#[derive(Clone, PartialEq, Message)]
pub struct Event {
    #[prost(message, optional, tag = "1")]
    pub group_id: Option<GroupId>,
    /// What `body` is about. `bodies` each say their own.
    #[prost(int32, tag = "3")]
    pub kind: i32,
    #[prost(message, optional, tag = "4")]
    pub body: Option<EventBody>,
    #[prost(message, repeated, tag = "8")]
    pub bodies: Vec<EventBody>,
}

#[derive(Clone, PartialEq, Message)]
pub struct EventBody {
    #[prost(message, optional, tag = "3")]
    pub group_viewed: Option<GroupViewed>,
    #[prost(message, optional, tag = "6")]
    pub message: Option<MessageEvent>,
    #[prost(int32, tag = "12")]
    pub kind: i32,
    #[prost(message, optional, tag = "18")]
    pub message_deleted: Option<MessageDeleted>,
    #[prost(message, optional, tag = "23")]
    pub user_status: Option<UserStatusUpdated>,
    #[prost(message, optional, tag = "26")]
    pub typing: Option<TypingStateChanged>,
    #[prost(message, optional, tag = "32")]
    pub marked_unread: Option<MarkedUnread>,
    #[prost(message, optional, tag = "38")]
    pub availability: Option<Availability>,
    #[prost(message, optional, tag = "65")]
    pub read_state: Option<ReadStateUpdated>,
}

#[derive(Clone, PartialEq, Message)]
pub struct UserStatusUpdated {
    #[prost(message, optional, tag = "1")]
    pub user_status: Option<UserStatus>,
}

/// Whether a person is around changed.
#[derive(Clone, PartialEq, Message)]
pub struct Availability {
    #[prost(message, optional, tag = "1")]
    pub user_id: Option<UserId>,
    /// 1 is active, 2 away, 3 idle.
    #[prost(int32, tag = "3")]
    pub state: i32,
}

/// A message posted or edited.
#[derive(Clone, PartialEq, Message)]
pub struct MessageEvent {
    #[prost(message, optional, tag = "1")]
    pub message: Option<ChatMessage>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MessageDeleted {
    #[prost(message, optional, tag = "1")]
    pub message_id: Option<MessageId>,
}

/// The person opened a conversation, here or on another device.
#[derive(Clone, PartialEq, Message)]
pub struct GroupViewed {
    #[prost(message, optional, tag = "1")]
    pub group_id: Option<GroupId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MarkedUnread {
    #[prost(message, optional, tag = "1")]
    pub group_id: Option<GroupId>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReadStateUpdated {
    #[prost(message, optional, tag = "1")]
    pub group_id: Option<GroupId>,
    #[prost(int64, tag = "2")]
    pub last_head_message_create_time: i64,
    #[prost(int64, tag = "3")]
    pub most_recent_read_time: i64,
    #[prost(int64, tag = "4")]
    pub mark_as_unread_time: i64,
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

    #[test]
    fn fetch_options_are_sent_one_by_one() {
        let request = ListTopicsRequest {
            fetch_options: vec![4, 3],
            ..Default::default()
        };
        assert_eq!(request.encode_to_vec(), [0x28, 4, 0x28, 3]);
    }
}
