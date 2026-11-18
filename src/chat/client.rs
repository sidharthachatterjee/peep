//! Requests to Google Chat with the signed-in session. All network work runs on Peep's own
//! threads, never on the one that draws.

use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use base64::Engine;
use futures::channel::mpsc::{self, UnboundedReceiver};
use prost::Message;
use reqwest::{Url, header};
use tokio::{
    runtime::Runtime,
    sync::{Mutex, Notify},
};

use super::{
    Conversation, ConversationId, Directory, Event, Page, Showing, Snapshot, Status, Token, User,
    UserId, conversation, live::Registration, page, status, wire,
};
use crate::{
    network,
    session::{self, Session},
};

pub(super) const ORIGIN: &str = "https://chat.google.com";
const API_PATH: &str = "/api/";
const BOOTSTRAP_PATH: &str = "/mole/world";
const PROTOBUF: &str = "application/x-protobuf";
const TIMEOUT: Duration = Duration::from_secs(30);

/// Conversations asked for per request: more than most accounts have, so one request
/// usually brings them all. One large reply arrives much sooner than several small ones.
const PAGE_SIZE: i32 = 2000;
/// Messages asked for at a time: a few screens' worth.
const MESSAGES_PER_PAGE: i32 = 40;
/// Replies asked for per thread. A direct message rarely has threads.
const REPLIES_PER_TOPIC: i32 = 20;
/// Asks for what the web client asks for: each topic's read state, and read receipts.
const TOPIC_METADATA: i32 = 4;
const READ_RECEIPTS: i32 = 3;
/// People looked up per request. Several small requests at once answer sooner than one
/// large one.
const PEOPLE_PER_REQUEST: usize = 100;
/// People whose status is asked for per request. Google answers a larger request in the
/// same time by leaving people out: asked about 500 at once, it reported a fifth of those
/// who were active.
const STATUSES_PER_REQUEST: usize = 50;
/// A guard against a server that never stops paging.
const MAX_PAGES: usize = 50;
/// How long a request token is used before a fresh one is fetched, in seconds. Google
/// refusing one sooner also fetches a fresh one.
const TOKEN_LIFETIME: f64 = 24. * 60. * 60.;

/// Describes the page that carries the request token. Google's web client sends this value.
const SHELL: &str = r#"["h_hs",null,null,[1,0],null,null,"gmail.pinto-server_20230730.06_p0",1,null,[15,38,36,35,26,30,41,18,24,11,21,14,6],null,null,"3Mu86PSulM4.en..es5",0,null,null,[0]]"#;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    /// Google no longer accepts the session; the person has to sign in again.
    #[error("Google has signed you out")]
    SignedOut,
    #[error("Google Chat refused the request (HTTP {0})")]
    Refused(u16),
    #[error("Google Chat answered with HTTP {0}")]
    Http(u16),
    #[error("Could not reach Google Chat: {0}")]
    Network(String),
    #[error("Google Chat's reply to {0} was not understood")]
    Unreadable(&'static str),
    #[error("The request was interrupted")]
    Interrupted,
}

#[derive(Clone)]
pub struct Client(Arc<Inner>);

pub(super) struct Inner {
    http: reqwest::Client,
    /// For the live channel, whose requests stay open: it sets no limit on how long one
    /// takes.
    pub(super) streaming: reqwest::Client,
    runtime: &'static Runtime,
    pub(super) session: Session,
    /// Fetched on first use, and again when it is old or Google refuses it.
    token: Mutex<Option<Token>>,
    /// Where the live channel is, read from the page that carries the token.
    registration: Mutex<Option<Registration>>,
    /// The people whose comings and goings the live channel is asked to report.
    pub(super) watched: std::sync::Mutex<Vec<UserId>>,
    /// Tells the live channel that `watched` changed.
    pub(super) watched_changed: Notify,
    /// Everyone who has been looked up. What an earlier launch kept is looked up once
    /// more, since names and photos change; after that no one is looked up twice.
    users: Mutex<HashMap<UserId, User>>,
    /// Whether what an earlier launch kept has been looked up again.
    users_current: Mutex<bool>,
}

impl Client {
    /// A client for `session`, starting from what an earlier launch kept.
    pub fn new(session: Session, snapshot: Snapshot) -> Result<Self, Error> {
        let runtime = network::runtime().map_err(|error| Error::Network(error.to_string()))?;
        let builder = || {
            reqwest::Client::builder()
                .user_agent(session.user_agent())
                // A redirect means Google wants a sign-in; following it would hide that.
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(TIMEOUT)
        };
        let build = |builder: reqwest::ClientBuilder| {
            builder
                .build()
                .map_err(|error| Error::Network(error.to_string()))
        };
        Ok(Self(Arc::new(Inner {
            http: build(builder().timeout(TIMEOUT))?,
            streaming: build(builder())?,
            runtime,
            session,
            token: Mutex::new(snapshot.token),
            registration: Mutex::new(None),
            watched: std::sync::Mutex::new(Vec::new()),
            watched_changed: Notify::new(),
            users: Mutex::new(snapshot.directory.users),
            users_current: Mutex::new(false),
        })))
    }

    /// Loads the direct messages and the people in them afresh, with what to keep for the
    /// next launch.
    pub fn refresh(&self) -> impl Future<Output = Result<Snapshot, Error>> + use<> {
        self.spawn(move |inner| async move { inner.refresh().await })
    }

    /// A page of a conversation's messages: the newest, or those before `before`, which is
    /// the `older` of the page after it.
    pub fn messages(
        &self,
        conversation: &ConversationId,
        before: Option<i64>,
    ) -> impl Future<Output = Result<Page, Error>> + use<> {
        let request = wire::ListTopicsRequest {
            page_size_for_topics: MESSAGES_PER_PAGE,
            filter: before.map(|before| wire::ListTopicsFilter {
                older_than: Some(before),
            }),
            fetch_options: match before {
                None => vec![TOPIC_METADATA, READ_RECEIPTS],
                Some(_) => vec![TOPIC_METADATA],
            },
            page_size_for_unread_replies: REPLIES_PER_TOPIC,
            page_size_for_read_replies: REPLIES_PER_TOPIC,
            group_id: Some(group_id(conversation)),
            sort_option: 2,
            request_header: wire::request_header(),
        };
        self.spawn(move |inner| async move {
            let response: wire::ListTopicsResponse = inner.call("list_topics", &request).await?;
            Ok(page(response, before))
        })
    }

    /// Tells Google the person has read `conversation` up to `time`, the time of its newest
    /// message.
    pub fn mark_read(
        &self,
        conversation: &ConversationId,
        time: i64,
    ) -> impl Future<Output = Result<(), Error>> + use<> {
        let request = wire::MarkGroupReadstateRequest {
            id: Some(group_id(conversation)),
            last_read_time: time,
            request_header: wire::request_header(),
        };
        self.spawn(move |inner| async move {
            inner
                .call::<_, wire::MarkGroupReadstateResponse>("mark_group_readstate", &request)
                .await
                .map(|_| ())
        })
    }

    /// How these people are doing right now. People Google says nothing about are left out.
    pub fn statuses(
        &self,
        people: &[UserId],
    ) -> impl Future<Output = Result<HashMap<UserId, Status>, Error>> + use<> {
        let requests: Vec<wire::GetUserPresenceRequest> = people
            .chunks(STATUSES_PER_REQUEST)
            .map(|people| wire::GetUserPresenceRequest {
                user_ids: people
                    .iter()
                    .map(|id| wire::UserId {
                        id: id.0.to_string(),
                    })
                    .collect(),
                include_user_status: true,
                request_header: wire::request_header(),
            })
            .collect();
        self.spawn(move |inner| async move {
            let mut statuses = HashMap::new();
            // One after another: asked for all at once, Google leaves some people out.
            for request in requests {
                let response: wire::GetUserPresenceResponse =
                    inner.call("get_user_presence", &request).await?;
                let now = (session::now() * 1_000_000.) as i64;
                statuses.extend(
                    response
                        .user_presences
                        .into_iter()
                        .filter_map(|presence| status(presence, now)),
                );
            }
            Ok(statuses)
        })
    }

    /// Changes what the signed-in person shows others, on every device. `DoNotDisturb`
    /// lasts until its time, and the other two until changed. Gives what Google saved.
    pub fn show(&self, showing: Showing) -> impl Future<Output = Result<Showing, Error>> + use<> {
        self.spawn(move |inner| async move {
            let now = (session::now() * 1_000_000.) as i64;
            let saved: wire::SetUserStatusResponse = match showing {
                Showing::DoNotDisturb { until } => {
                    let request = wire::SetDndDurationRequest {
                        duration_usec: (until - now).max(0),
                        request_header: wire::request_header(),
                    };
                    inner.call("set_dnd_duration", &request).await?
                }
                Showing::Automatic | Showing::Away => {
                    let request = wire::SetPresenceSharedRequest {
                        presence_shared: Some(showing == Showing::Automatic),
                        request_header: wire::request_header(),
                    };
                    inner.call("set_presence_shared", &request).await?
                }
            };
            let saved = saved
                .user_status
                .ok_or(Error::Unreadable("the saved status"))?;
            Ok(Showing::from_wire(&saved).at((session::now() * 1_000_000.) as i64))
        })
    }

    /// What happens from now on, as it happens. The channel behind it reconnects by itself
    /// and stops when the receiver is dropped.
    pub fn events(&self) -> UnboundedReceiver<Event> {
        let (listener, events) = mpsc::unbounded();
        self.0.runtime.spawn(self.0.clone().listen(listener));
        events
    }

    /// Asks the live channel to report, from now on, whether these people are around, in
    /// place of whoever it was asked about before. It reports each person's state at once
    /// and then whenever it changes.
    pub fn watch(&self, people: Vec<UserId>) {
        // A poisoned lock only means another thread panicked mid-swap; the list is whole.
        let mut watched = self
            .0
            .watched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *watched != people {
            *watched = people;
            self.0.watched_changed.notify_one();
        }
    }

    /// Runs `work` on the network threads.
    fn spawn<T, Work, Start>(
        &self,
        work: Start,
    ) -> impl Future<Output = Result<T, Error>> + use<T, Work, Start>
    where
        T: Send + 'static,
        Work: Future<Output = Result<T, Error>> + Send + 'static,
        Start: FnOnce(Arc<Inner>) -> Work,
    {
        let task = self.0.runtime.spawn(work(self.0.clone()));
        async move { task.await.unwrap_or(Err(Error::Interrupted)) }
    }
}

fn group_id(conversation: &ConversationId) -> wire::GroupId {
    match conversation {
        ConversationId::Dm(id) => wire::GroupId {
            dm_id: Some(wire::DmId {
                dm_id: id.to_string(),
            }),
            space_id: None,
        },
        ConversationId::Space(id) => wire::GroupId {
            space_id: Some(wire::SpaceId {
                space_id: id.to_string(),
            }),
            dm_id: None,
        },
    }
}

impl Inner {
    async fn refresh(&self) -> Result<Snapshot, Error> {
        let ((me, showing), items) = futures::try_join!(self.me(), self.world())?;

        let mut conversations: Vec<Conversation> = Vec::new();
        for item in items {
            if let Some(conversation) = conversation(item, &me)
                && !conversations
                    .iter()
                    .any(|known| known.id == conversation.id)
            {
                conversations.push(conversation);
            }
        }
        conversations.sort_by_key(|conversation| std::cmp::Reverse(conversation.sort_time));

        let mut members: Vec<&UserId> = conversations
            .iter()
            .flat_map(|conversation| &conversation.members)
            .collect();
        // The signed-in person too, for the photo beside their own messages.
        members.push(&me);
        members.sort_by(|a, b| a.0.cmp(&b.0));
        members.dedup();

        let mut users = self.users.lock().await;
        let mut current = self.users_current.lock().await;
        let unknown: Vec<&UserId> = members
            .iter()
            .copied()
            .filter(|id| !*current || !users.contains_key(id))
            .collect();
        users.extend(self.users(&unknown).await?);
        *current = true;
        users.retain(|id, _| {
            members
                .binary_search_by(|member| member.0.cmp(&id.0))
                .is_ok()
        });

        let mut directory = Directory {
            me: Some(me),
            showing,
            users: users.clone(),
            conversations,
        };
        directory.name_conversations();
        Ok(Snapshot {
            directory,
            token: self.token.lock().await.clone(),
        })
    }

    /// The signed-in person, and what they chose to show others.
    async fn me(&self) -> Result<(UserId, Showing), Error> {
        const METHOD: &str = "get_self_user_status";
        let request = wire::GetSelfUserStatusRequest {
            request_header: wire::request_header(),
        };
        let response: wire::GetSelfUserStatusResponse = self.call(METHOD, &request).await?;
        let status = response.user_status.ok_or(Error::Unreadable(METHOD))?;
        let showing = Showing::from_wire(&status).at((session::now() * 1_000_000.) as i64);
        status
            .user_id
            .filter(|id| !id.id.is_empty())
            .map(|id| (UserId(id.id.into()), showing))
            .ok_or(Error::Unreadable(METHOD))
    }

    /// Every conversation in the sidebar, spaces included.
    async fn world(&self) -> Result<Vec<wire::WorldItem>, Error> {
        let mut items = Vec::new();
        let mut page = String::new();
        for _ in 0..MAX_PAGES {
            let request = wire::PaginatedWorldRequest {
                request_header: wire::request_header(),
                world_section_requests: vec![wire::WorldSectionRequest {
                    page_size: PAGE_SIZE,
                    pagination_token: page.clone(),
                }],
                fetch_from_user_spaces: true,
                fetch_snippets_for_unnamed_rooms: true,
            };
            let response: wire::PaginatedWorldResponse =
                self.call("paginated_world", &request).await?;

            items.extend(response.world_items);
            let mut next = None;
            for section in response.world_section_responses {
                items.extend(section.world_items);
                if section.more_items && !section.pagination_token.is_empty() {
                    next.get_or_insert(section.pagination_token);
                }
            }
            match next {
                Some(next) if next != page => page = next,
                _ => break,
            }
        }
        Ok(items)
    }

    async fn users(&self, ids: &[&UserId]) -> Result<HashMap<UserId, User>, Error> {
        let looked_up = ids
            .chunks(PEOPLE_PER_REQUEST)
            .map(|ids| self.some_users(ids));
        Ok(futures::future::try_join_all(looked_up)
            .await?
            .into_iter()
            .flatten()
            .collect())
    }

    async fn some_users(&self, ids: &[&UserId]) -> Result<HashMap<UserId, User>, Error> {
        let request = wire::GetMembersRequest {
            membership_ids: ids
                .iter()
                .map(|id| wire::MembershipId {
                    member_id: Some(wire::MemberId {
                        user_id: Some(wire::UserId {
                            id: id.0.to_string(),
                        }),
                    }),
                })
                .collect(),
            request_header: wire::request_header(),
        };
        let response: wire::GetMembersResponse = self.call("get_members", &request).await?;

        let profiles = response
            .member_profiles
            .into_iter()
            .filter_map(|profile| profile.member);
        Ok(response
            .members
            .into_iter()
            .chain(profiles)
            .filter_map(|member| member.user)
            .filter_map(|user| {
                let id = UserId(user.user_id?.id.into());
                let user = User {
                    name: user.name.into(),
                    email: user.email.into(),
                    photo: user.avatar_url.into(),
                };
                Some((id, user))
            })
            .collect())
    }

    async fn call<Request: Message, Response: Message + Default>(
        &self,
        method: &'static str,
        request: &Request,
    ) -> Result<Response, Error> {
        let body = request.encode_to_vec();
        let path = format!("{API_PATH}{method}");
        let mut retried = false;
        loop {
            let token = self.token().await?;
            let response = self
                .http
                .post(format!("{ORIGIN}{path}?rt=b"))
                .header(
                    header::COOKIE,
                    self.session.cookie_header(&path, session::now()),
                )
                .header(header::CONTENT_TYPE, PROTOBUF)
                .header(header::ACCEPT, PROTOBUF)
                .header(header::ORIGIN, ORIGIN)
                .header(header::REFERER, format!("{ORIGIN}/"))
                .header("X-Framework-XSRF-Token", token)
                .body(body.clone())
                .send()
                .await
                .map_err(network)?;

            let status = response.status().as_u16();
            let header = |name: header::HeaderName| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            };
            let location = header(header::LOCATION);
            let content_type = header(header::CONTENT_TYPE).unwrap_or_default();
            let base64 = response
                .headers()
                .get("X-Goog-Safety-Encoding")
                .is_some_and(|value| value == "base64");
            let bytes = response.bytes().await.map_err(network)?;

            if status == 200 {
                let payload =
                    payload(&content_type, base64, &bytes).ok_or(Error::Unreadable(method))?;
                return Response::decode(payload.as_slice()).map_err(|_| Error::Unreadable(method));
            }
            if matches!(status, 401 | 403) && !retried {
                // The token may have expired: fetch a fresh one and try once more.
                retried = true;
                *self.token.lock().await = None;
                continue;
            }
            return Err(refusal(
                status,
                location.as_deref(),
                &String::from_utf8_lossy(&bytes),
            ));
        }
    }

    /// Where the live channel is. `fresh` reads the page again, which also renews the
    /// request token.
    pub(super) async fn registration(&self, fresh: bool) -> Result<Registration, Error> {
        let known = self.registration.lock().await.clone();
        if let (false, Some(registration)) = (fresh, known) {
            return Ok(registration);
        }
        *self.token.lock().await = None;
        self.token().await?;
        self.registration
            .lock()
            .await
            .clone()
            .ok_or(Error::Unreadable("the live channel's registration"))
    }

    pub(super) async fn token(&self) -> Result<String, Error> {
        let mut token = self.token.lock().await;
        if let Some(token) = &*token
            && session::now() - token.issued < TOKEN_LIFETIME
        {
            return Ok(token.value.clone());
        }

        let response = self
            .http
            .get(format!("{ORIGIN}{BOOTSTRAP_PATH}"))
            .query(&[
                ("hl", "en"),
                ("hs", SHELL),
                ("origin", "https://mail.google.com"),
                ("shell", "9"),
                ("wfi", "gtn-roster-iframe-id"),
            ])
            .header(
                header::COOKIE,
                self.session.cookie_header(BOOTSTRAP_PATH, session::now()),
            )
            .header(header::REFERER, "https://mail.google.com/")
            .send()
            .await
            .map_err(network)?;

        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let page = response.text().await.map_err(network)?;
        if status != 200 {
            return Err(refusal(status, location.as_deref(), &page));
        }
        if is_sign_in_page(&page) {
            return Err(Error::SignedOut);
        }
        let fresh = request_token(&page).ok_or(Error::Unreadable("the request token page"))?;
        *token = Some(Token {
            value: fresh.clone(),
            issued: session::now(),
        });
        *self.registration.lock().await = Registration::in_page(&page);
        Ok(fresh)
    }
}

pub(super) fn network(error: reqwest::Error) -> Error {
    Error::Network(error.without_url().to_string())
}

/// What a reply other than 200 means. Only Google's sign-in page ends the session.
pub(super) fn refusal(status: u16, location: Option<&str>, body: &str) -> Error {
    let to_sign_in = location
        .and_then(|location| Url::parse(location).ok())
        .is_some_and(|url| url.host_str() == Some("accounts.google.com"));
    match status {
        300..=399 if to_sign_in => Error::SignedOut,
        401 | 403 if to_sign_in || is_sign_in_page(body) => Error::SignedOut,
        401 | 403 => Error::Refused(status),
        _ => Error::Http(status),
    }
}

/// Google's pages name the app they belong to; its sign-in app is `AccountsSignInUi`.
fn is_sign_in_page(page: &str) -> bool {
    value_after(page, "qwAQke").is_some_and(|app| app == "AccountsSignInUi")
}

/// The request token Google embeds in the page it serves to a signed-in session.
fn request_token(page: &str) -> Option<String> {
    value_after(page, "SMqcke").filter(|token| !token.is_empty() && !token.contains(['\r', '\n']))
}

/// The string value of `"key": "value"` in a page's embedded JSON. The JSON may itself
/// sit inside a string, with its quotes escaped.
fn value_after(page: &str, key: &str) -> Option<String> {
    let rest = &page[page.find(key)? + key.len()..];
    let rest = rest.trim_start_matches(['\\', '"']).trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if let Some(escaped) = rest.strip_prefix("\\\"") {
        return Some(escaped[..escaped.find("\\\"")?].to_owned());
    }
    let end = rest
        .char_indices()
        .skip(1)
        .find(|(index, c)| *c == '"' && !rest[..*index].ends_with('\\'))
        .map(|(index, _)| index + 1)?;
    serde_json::from_str(&rest[..end]).ok()
}

/// The protobuf bytes of a reply, or `None` if it is not one.
fn payload(content_type: &str, base64: bool, body: &[u8]) -> Option<Vec<u8>> {
    if !content_type.contains("protobuf") && !content_type.contains("octet-stream") {
        return None;
    }
    let body = match body.strip_prefix(b")]}'") {
        Some(rest) => rest.trim_ascii_start(),
        None => body,
    };
    if base64 {
        let text: Vec<u8> = body
            .iter()
            .copied()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        return base64::engine::general_purpose::STANDARD.decode(text).ok();
    }
    Some(body.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_token_is_read_from_the_page() {
        let page = r#"<script>window.WIZ_global_data = {"qwAQke":"DynamiteWebUi","SMqcke" : "AB\u003dc:123","x":1};</script>"#;
        assert_eq!(request_token(page).as_deref(), Some("AB=c:123"));
        assert!(!is_sign_in_page(page));

        assert_eq!(request_token(r#"{"SMqcke":""}"#), None);
        assert_eq!(request_token(r#"{"other":"value"}"#), None);
    }

    #[test]
    fn the_sign_in_page_is_recognised_plain_or_escaped() {
        assert!(is_sign_in_page(r#"{"qwAQke":"AccountsSignInUi"}"#));
        assert!(is_sign_in_page(
            r#"x = "{\"qwAQke\":\"AccountsSignInUi\"}""#
        ));
        assert!(!is_sign_in_page("AccountsSignInUi appears in prose"));
    }

    #[test]
    fn only_googles_sign_in_page_ends_the_session() {
        let sign_in = Some("https://accounts.google.com/ServiceLogin?continue=x");
        assert_eq!(refusal(302, sign_in, ""), Error::SignedOut);
        assert_eq!(refusal(401, sign_in, ""), Error::SignedOut);
        assert_eq!(
            refusal(403, None, r#"{"qwAQke":"AccountsSignInUi"}"#),
            Error::SignedOut
        );

        assert_eq!(
            refusal(302, Some("https://chat.google.com/"), ""),
            Error::Http(302)
        );
        assert_eq!(refusal(403, None, "forbidden"), Error::Refused(403));
        assert_eq!(refusal(500, None, ""), Error::Http(500));
    }

    #[test]
    fn a_reply_is_unwrapped_to_its_protobuf_bytes() {
        assert_eq!(payload(PROTOBUF, false, b"\x08\x01"), Some(vec![8, 1]));
        assert_eq!(
            payload("application/octet-stream", false, b")]}'\n\x08\x01"),
            Some(vec![8, 1]),
            "the guard prefix is dropped"
        );
        assert_eq!(
            payload(PROTOBUF, true, b"CA\nE=\n"),
            Some(vec![8, 1]),
            "base64 with line breaks"
        );
        assert_eq!(payload("text/html", false, b"<html>"), None);
        assert_eq!(payload(PROTOBUF, true, b"not base64!"), None);
    }
}
