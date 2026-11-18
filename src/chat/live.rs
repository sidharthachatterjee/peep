//! Google Chat's live channel, which reports what happens as it happens.
//!
//! The page that carries the request token also says where the signed-in person's stream of
//! events lives. Peep asks Google which server to use, opens a channel there with one
//! "watch" on that stream, and reads it with a request the server holds open. The server
//! sends a watermark every few seconds; reconnecting from the last one resumes the stream
//! without missing anything.

mod frames;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use base64::Engine;
use futures::channel::mpsc::UnboundedSender;
use prost::Message as _;
use reqwest::{Method, RequestBuilder, header};
use serde_json::{Value, json};

use self::frames::{Frame, Payload, Reader};
use super::{
    ConversationId, Event, UserId,
    client::{Error, Inner, ORIGIN, network, refusal},
    events, wire,
};
use crate::session;

/// The watch on the signed-in person's events. Watches on other people's availability are
/// numbered after it.
const WATCH: u64 = 1;
/// Watches asked for in one request.
const WATCHES_PER_REQUEST: usize = 50;
/// Sent with every watch, as the web client sends it.
const WATCH_VERSION: [u8; 2] = [9, 5];
/// The channel's credentials are renewed this often, as the web client does.
const RENEW_CREDENTIALS: Duration = Duration::from_secs(260);
/// The shortest and longest wait before reconnecting after a failure.
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_LONGEST: Duration = Duration::from_secs(60);
/// A channel that lasted this long counts as having worked.
const SETTLED: Duration = Duration::from_secs(30);
/// Google sends each change twice; this many recent ones are remembered to drop repeats.
const REMEMBERED: usize = 32;
/// How long a request that should answer at once may take.
const TIMEOUT: Duration = Duration::from_secs(30);
const JSON_PROTOBUF: &str = "application/json+protobuf";

/// Where the signed-in person's events live, as the bootstrap page says.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Registration {
    /// The address of the service, such as `https://chat.google.com/punctual/prod-02-us`.
    base: String,
    /// The moment the page describes, in microseconds: where a fresh stream starts.
    start: i64,
    /// How long the registration lasts.
    lease: Duration,
    /// Names the person's stream.
    target: String,
    key: String,
}

impl Registration {
    /// Reads the registration out of the bootstrap page, where it sits in an array that
    /// starts with `"dfe.cr.rr"`.
    pub(super) fn in_page(page: &str) -> Option<Self> {
        let tag = page.find(r#""dfe.cr.rr""#)?;
        let start = page[..tag].rfind('[')?;
        let block: Value = serde_json::Deserializer::from_str(&page[start..])
            .into_iter()
            .next()?
            .ok()?;
        let config = block.get(2)?;

        // "/punctual/prod-dynamite-prod-02-us/…" is served under "/punctual/prod-02-us".
        let environment = config.get(1)?.get(1)?.as_str()?.split('/').nth(2)?;
        let environment = environment.strip_prefix("prod-dynamite-")?;
        if !environment.starts_with("prod-")
            || !environment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return None;
        }
        let moment = config.get(2)?;
        let start = integer(moment.get(0)?)? * 1_000_000
            + moment.get(1).and_then(integer).unwrap_or(0) / 1000;
        let lease = config
            .get(3)
            .and_then(|lease| lease.get(0))
            .and_then(integer)
            .and_then(|seconds| u64::try_from(seconds).ok())
            .unwrap_or(3600);
        let text = |index| {
            config
                .get(index)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Some(Self {
            base: format!("{ORIGIN}/punctual/{environment}"),
            start,
            // Never so short that renewing it becomes a loop.
            lease: Duration::from_secs(lease.max(60)),
            target: text(4)?,
            key: text(5)?,
        })
    }

    /// The path of `base`, for choosing cookies.
    fn path(&self) -> &str {
        &self.base[ORIGIN.len()..]
    }

    /// Names the stream to watch.
    fn stream(&self) -> Value {
        json!([
            ["user-targeted-changes"],
            [null, 1],
            [[["userTargeted"], [self.target], ["events"]]]
        ])
    }

    /// Asks which server will carry the channel.
    fn choose_server(&self) -> String {
        json!([
            [null, null, null, WATCH_VERSION, null, self.stream()],
            null,
            null,
            0,
            0
        ])
        .to_string()
    }

    /// Starts the watch from `from`. `resumed` says `from` is a watermark of an earlier
    /// channel rather than the registration's own start.
    fn watch(&self, from: i64, resumed: bool, reconnecting: bool, stream_type: i64) -> String {
        let reason = if reconnecting { 3 } else { 1 };
        let spec = json!([
            from,
            null,
            null,
            WATCH_VERSION,
            null,
            self.stream(),
            null,
            u8::from(resumed),
            reason
        ]);
        json!([[[WATCH, spec, null, stream_type]]]).to_string()
    }
}

/// Something the channel reports on as it changes, when asked to watch it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Subject {
    /// Whether a person is around.
    Person(UserId),
    /// Who is typing in a conversation.
    Typing(ConversationId),
}

/// Starts watch `watch` on `subject`.
fn watch_subject(watch: u64, subject: &Subject) -> String {
    let stream = match subject {
        Subject::Person(person) => json!([
            ["user-state-changes"],
            [1],
            [[["state"], ["user"], [&*person.0], ["availability"]]]
        ]),
        Subject::Typing(conversation) => {
            let (ConversationId::Dm(id) | ConversationId::Space(id)) = conversation;
            json!([
                ["group-state-changes"],
                [1],
                [[["state"], ["group"], [&**id], ["typing"]]]
            ])
        }
    };
    let spec = json!([null, null, null, WATCH_VERSION, null, stream, null, null, 1]);
    json!([[[watch, spec]]]).to_string()
}

/// Ends watch `watch`.
fn unwatch(watch: u64) -> String {
    json!([[[watch, null, []]]]).to_string()
}

/// A number that may be written as a number or as a string.
fn integer(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

/// The server Google chose for a channel.
#[derive(Clone, Debug, PartialEq)]
struct Server {
    session: String,
    stream_type: i64,
    /// How long Google asks a first connection to wait.
    delay: Duration,
}

impl Server {
    fn parse(reply: &[u8]) -> Option<Self> {
        let reply: Value = serde_json::from_slice(reply).ok()?;
        let session = reply
            .get(0)?
            .as_str()
            .filter(|session| !session.is_empty())?;
        let delay = reply.get(2).and_then(integer).unwrap_or(0).clamp(0, 30_000);
        Some(Self {
            session: session.to_owned(),
            stream_type: reply.get(1).and_then(integer).unwrap_or(1),
            delay: Duration::from_millis(delay as u64),
        })
    }
}

/// One thing the channel says. Most are about one watch, named by its number.
#[derive(Clone, Debug, PartialEq)]
enum Update {
    /// The channel's id, for renewing its credentials.
    Channel(String),
    /// The watch is running.
    Started(u64),
    /// Everything up to this moment has been sent.
    Watermark(u64, i64),
    /// What the watch reports, still encoded.
    Change(u64, Vec<u8>),
    /// The server will not run the watch as it was asked to.
    Refused(u64),
    /// The server wants a new channel.
    Close,
}

/// What one batch from the channel says.
fn updates(batch: &Value) -> Vec<Update> {
    let mut updates = Vec::new();
    let responses = batch.get(0).and_then(Value::as_array);
    for response in responses.into_iter().flatten() {
        let Some(watch) = response
            .get(0)
            .and_then(integer)
            .and_then(|watch| u64::try_from(watch).ok())
        else {
            continue;
        };
        if let Some(change) = response.get(1).filter(|change| change.is_array()) {
            if change.get(0).and_then(|started| started.get(0)).is_some() {
                updates.push(Update::Started(watch));
            }
            let items = change
                .get(1)
                .and_then(|items| items.get(0))
                .and_then(Value::as_array);
            for item in items.into_iter().flatten() {
                let payload = item
                    .get(1)
                    .and_then(Value::as_str)
                    .and_then(|text| base64::engine::general_purpose::STANDARD.decode(text).ok());
                updates.extend(payload.map(|payload| Update::Change(watch, payload)));
            }
            if let Some(mark) = change.get(2).and_then(|mark| mark.get(0)).and_then(integer) {
                updates.push(Update::Watermark(watch, mark));
            }
            if change.get(4).is_some_and(Value::is_array) {
                updates.push(Update::Refused(watch));
            }
        }
        let failed = response
            .get(2)
            .and_then(|status| status.get(0))
            .and_then(|status| status.get(0))
            .and_then(integer)
            .is_some_and(|code| code != 0);
        if failed {
            updates.push(Update::Refused(watch));
        }
    }
    if batch.get(1).and_then(|flag| flag.get(0)).and_then(integer) == Some(1) {
        updates.push(Update::Close);
    }
    if let Some(channel) = batch
        .get(2)
        .and_then(|channel| channel.get(0))
        .and_then(Value::as_str)
    {
        updates.push(Update::Channel(channel.to_owned()));
    }
    updates
}

/// The watches one channel carries besides the one on the signed-in person's events.
struct Watches {
    /// The number of each subject's watch.
    watching: HashMap<Subject, u64>,
    watched_by: HashMap<u64, Subject>,
    /// What the server would not watch, which is not asked for again on this channel.
    refused: HashSet<Subject>,
    next_watch: u64,
    /// The number of the last request sent, and how many messages have been sent in all.
    request: u32,
    sent: usize,
}

impl Watches {
    /// The messages that bring the channel's watches in line with `wanted`.
    fn changes(&mut self, wanted: &[Subject]) -> Vec<String> {
        let mut changes = Vec::new();
        let gone: Vec<Subject> = self
            .watching
            .keys()
            .filter(|subject| !wanted.contains(subject))
            .cloned()
            .collect();
        for subject in gone {
            if let Some(watch) = self.watching.remove(&subject) {
                self.watched_by.remove(&watch);
                changes.push(unwatch(watch));
            }
        }
        for subject in wanted {
            if self.watching.contains_key(subject) || self.refused.contains(subject) {
                continue;
            }
            self.next_watch += 1;
            self.watching.insert(subject.clone(), self.next_watch);
            self.watched_by.insert(self.next_watch, subject.clone());
            changes.push(watch_subject(self.next_watch, subject));
        }
        changes
    }
}

/// Why a channel ended.
enum Ended {
    /// The registration ran out; a new channel takes over at once.
    Renew,
    /// The listener went away: nothing is left to tell.
    Unheard,
    Failed(Error),
}

/// What outlasts one channel.
#[derive(Default)]
struct Stream {
    /// The last watermark: where the next channel resumes.
    resume: Option<i64>,
    /// The stream could not be resumed, so events may have been missed.
    gap: bool,
    /// The most recent changes, to drop the repeats Google sends.
    recent: Vec<Vec<u8>>,
    opened: usize,
    /// Whether the listener has been told the channel is up.
    connected: bool,
}

impl Inner {
    /// Reports events to `listener` until it goes away or Google ends the session,
    /// reconnecting whenever the channel drops.
    pub(super) async fn listen(self: Arc<Self>, listener: UnboundedSender<Event>) {
        let mut stream = Stream::default();
        let mut failures = 0u32;
        loop {
            let opened = Instant::now();
            let ended = self.channel(&mut stream, &listener).await;
            let was_connected = std::mem::take(&mut stream.connected);
            if was_connected && listener.unbounded_send(Event::Disconnected).is_err() {
                return;
            }
            match ended {
                Ended::Unheard => return,
                Ended::Renew => continue,
                Ended::Failed(Error::SignedOut) => {
                    listener.unbounded_send(Event::SignedOut).ok();
                    return;
                }
                Ended::Failed(_) => {}
            }
            failures = if was_connected && opened.elapsed() >= SETTLED {
                1
            } else {
                failures + 1
            };
            let wait = RETRY_FIRST
                .saturating_mul(1 << (failures - 1).min(6))
                .min(RETRY_LONGEST);
            tokio::time::sleep(wait).await;
            if listener.is_closed() {
                return;
            }
        }
    }

    /// Opens one channel and reads it until it ends.
    async fn channel(&self, stream: &mut Stream, listener: &UnboundedSender<Event>) -> Ended {
        match self.read_channel(stream, listener).await {
            Ok(ended) => ended,
            Err(error) => Ended::Failed(error),
        }
    }

    async fn read_channel(
        &self,
        stream: &mut Stream,
        listener: &UnboundedSender<Event>,
    ) -> Result<Ended, Error> {
        let first = stream.opened == 0;
        stream.opened += 1;
        // A reconnect reads the page again: its registration may have run out.
        let registration = self.registration(!first).await?;
        let renew_at = Instant::now() + registration.lease / 2;

        let server = self
            .live(Method::POST, &registration, "v1/chooseServer", &[])
            .await?
            .header(header::CONTENT_TYPE, JSON_PROTOBUF)
            .body(registration.choose_server());
        let server = Server::parse(&send(server).await?)
            .ok_or(Error::Unreadable("the live channel's server"))?;
        if first {
            tokio::time::sleep(server.delay).await;
        }

        let channel = "multi-watch/channel";
        let common = [("VER", "8"), ("gsessionid", server.session.as_str())];
        let request = time_as_number();
        let watch = registration.watch(
            stream.resume.unwrap_or(registration.start),
            stream.resume.is_some(),
            !first,
            server.stream_type,
        );
        let open = self
            .live(
                Method::POST,
                &registration,
                channel,
                &[
                    &common[..],
                    &[("RID", &request.to_string()), ("CVER", "22"), ("t", "1")],
                ]
                .concat(),
            )
            .await?
            .header("X-WebChannel-Content-Type", JSON_PROTOBUF)
            .form(&[("count", "1"), ("ofs", "0"), ("req0___data__", &watch)]);
        let opening = send(open).await?;
        let (session, keepalive) = Reader::default()
            .feed(&opening)
            .ok()
            .into_iter()
            .flatten()
            .find_map(|frame| match frame.payload {
                Payload::Opened {
                    session,
                    keepalive_ms,
                } => Some((session, Duration::from_millis(keepalive_ms))),
                _ => None,
            })
            .ok_or(Error::Unreadable("the live channel's opening"))?;
        // The server sends something at least this often; silence for longer means the
        // connection is dead.
        let patience = keepalive.mul_f32(1.5);

        let mut acknowledged = 0;
        let mut channel_id: Option<String> = None;
        let mut renew_credentials_at = Instant::now() + RENEW_CREDENTIALS;
        let mut watches = Watches {
            watching: HashMap::new(),
            watched_by: HashMap::new(),
            refused: HashSet::new(),
            next_watch: WATCH,
            request,
            sent: 1,
        };
        // A new channel watches nothing else yet.
        let mut watches_changed = true;
        loop {
            let poll = self
                .live(
                    Method::GET,
                    &registration,
                    channel,
                    &[
                        &common[..],
                        &[
                            ("RID", "rpc"),
                            ("SID", &session),
                            ("AID", &acknowledged.to_string()),
                            ("CI", "0"),
                            ("TYPE", "xmlhttp"),
                            ("t", "1"),
                        ],
                    ]
                    .concat(),
                )
                .await?;
            let mut response = tokio::time::timeout(TIMEOUT, poll.send())
                .await
                .map_err(|_| Error::Network("the live channel did not answer".into()))?
                .map_err(network)?;
            let status = response.status().as_u16();
            if status != 200 {
                return Err(refusal(status, None, ""));
            }

            let mut reader = Reader::default();
            let mut heard_any = false;
            loop {
                if std::mem::take(&mut watches_changed) {
                    let wanted = self
                        .watched
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    for messages in watches.changes(&wanted).chunks(WATCHES_PER_REQUEST) {
                        watches.request += 1;
                        let count = messages.len().to_string();
                        let offset = watches.sent.to_string();
                        watches.sent += messages.len();
                        let mut form =
                            vec![("count".to_owned(), count), ("ofs".to_owned(), offset)];
                        for (index, message) in messages.iter().enumerate() {
                            form.push((format!("req{index}___data__"), message.clone()));
                        }
                        let change = self
                            .live(
                                Method::POST,
                                &registration,
                                channel,
                                &[
                                    &common[..],
                                    &[
                                        ("SID", &session),
                                        ("RID", &watches.request.to_string()),
                                        ("AID", &acknowledged.to_string()),
                                        ("t", "1"),
                                    ],
                                ]
                                .concat(),
                            )
                            .await?
                            .header("X-WebChannel-Content-Type", JSON_PROTOBUF)
                            .form(&form);
                        send(change).await?;
                    }
                }

                let chunk = tokio::select! {
                    chunk = tokio::time::timeout(patience, response.chunk()) => chunk
                        .map_err(|_| Error::Network("the live channel went quiet".into()))?
                        .map_err(network)?,
                    () = self.watched_changed.notified() => {
                        watches_changed = true;
                        continue;
                    }
                };
                let Some(chunk) = chunk else { break };
                let frames = reader
                    .feed(&chunk)
                    .map_err(|_| Error::Unreadable("the live channel"))?;
                for Frame { number, payload } in frames {
                    heard_any = true;
                    acknowledged = acknowledged.max(number);
                    let batch = match payload {
                        Payload::Data(batch) => batch,
                        Payload::Stop => {
                            return Err(Error::Network("the live channel closed".into()));
                        }
                        _ => continue,
                    };
                    for update in updates(&batch) {
                        let heard = match update {
                            Update::Channel(id) => {
                                channel_id = Some(id);
                                continue;
                            }
                            Update::Watermark(WATCH, mark) => {
                                stream.resume = Some(stream.resume.unwrap_or(0).max(mark));
                                continue;
                            }
                            Update::Started(WATCH) if !stream.connected => {
                                stream.connected = true;
                                let gap = std::mem::take(&mut stream.gap);
                                vec![Event::Connected { gap }]
                            }
                            Update::Change(WATCH, change) => {
                                if stream.recent.contains(&change) {
                                    continue;
                                }
                                // An event Peep cannot read is skipped, not fatal.
                                let report = wire::Event::decode(change.as_slice());
                                if stream.recent.len() == REMEMBERED {
                                    stream.recent.remove(0);
                                }
                                stream.recent.push(change);
                                report.map(events).unwrap_or_default()
                            }
                            Update::Refused(WATCH) => {
                                // Too far back to resume: start again from now, and say
                                // that what happened in between is not known.
                                stream.resume = None;
                                stream.gap = true;
                                return Err(Error::Network(
                                    "the live channel would not resume".into(),
                                ));
                            }
                            Update::Close => {
                                return Err(Error::Network("the live channel closed".into()));
                            }
                            // What another watch reports is an event like any other.
                            Update::Change(_, change) => wire::Event::decode(change.as_slice())
                                .map(events)
                                .unwrap_or_default(),
                            Update::Refused(watch) => {
                                if let Some(subject) = watches.watched_by.remove(&watch) {
                                    watches.watching.remove(&subject);
                                    watches.refused.insert(subject);
                                }
                                continue;
                            }
                            Update::Started(_) | Update::Watermark(..) => continue,
                        };
                        for event in heard {
                            if listener.unbounded_send(event).is_err() {
                                return Ok(Ended::Unheard);
                            }
                        }
                    }
                }

                if listener.is_closed() {
                    return Ok(Ended::Unheard);
                }
                if Instant::now() >= renew_at {
                    return Ok(Ended::Renew);
                }
                if Instant::now() >= renew_credentials_at
                    && let Some(id) = &channel_id
                {
                    renew_credentials_at = Instant::now() + RENEW_CREDENTIALS;
                    let renewal = self
                        .live(
                            Method::POST,
                            &registration,
                            "v1/refreshCreds",
                            &[("gsessionid", server.session.as_str())],
                        )
                        .await?
                        .header(header::CONTENT_TYPE, JSON_PROTOBUF)
                        .body(json!([id]).to_string());
                    // If this fails the channel soon ends and a new one is opened.
                    tokio::spawn(async move { renewal.timeout(TIMEOUT).send().await.ok() });
                }
            }
            if !heard_any {
                // An empty reply would make this loop spin as fast as the network allows.
                return Err(Error::Unreadable("the live channel"));
            }
        }
    }

    /// A request to the live service at `registration`, signed as the web client signs it.
    async fn live(
        &self,
        method: Method,
        registration: &Registration,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<RequestBuilder, Error> {
        let token = self.token().await?;
        let now = session::now();
        let cookie_path = format!("{}/{path}", registration.path());
        let mut request = self
            .streaming
            .request(method, format!("{}/{path}", registration.base))
            .query(&[("key", registration.key.as_str())])
            .query(query)
            .header(
                header::COOKIE,
                self.session.cookie_header(&cookie_path, now),
            )
            .header(header::ORIGIN, ORIGIN)
            .header(header::REFERER, format!("{ORIGIN}/"))
            .header("X-Framework-XSRF-Token", token);
        if let Some(secret) = self.session.api_secret(now) {
            request = request
                .header(header::AUTHORIZATION, authorization(secret, now as u64))
                .header("X-Goog-AuthUser", "0");
        }
        Ok(request)
    }
}

/// The `Authorization` header Google's services take beside the cookies: a hash of the
/// time, the session's API secret and the page the request comes from.
fn authorization(secret: &str, seconds: u64) -> String {
    let hash = sha1_smol::Sha1::from(format!("{seconds} {secret} {ORIGIN}")).digest();
    format!("SAPISIDHASH {seconds}_{hash}")
}

/// Sends a short request and returns its reply's body.
async fn send(request: RequestBuilder) -> Result<Vec<u8>, Error> {
    let response = request.timeout(TIMEOUT).send().await.map_err(network)?;
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.bytes().await.map_err(network)?;
    if status == 200 {
        return Ok(body.to_vec());
    }
    Err(refusal(
        status,
        location.as_deref(),
        &String::from_utf8_lossy(&body),
    ))
}

/// A number to start a channel's request numbering from, as the web client picks one.
fn time_as_number() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    10_000 + nanos % 80_000
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<script>x = ["other"]; y = ["dfe.cr.rr",["reg-1"],[null,[null,"/punctual/prod-dynamite-prod-02-us/user-targeted-changes",["","user-state-changes","group-state-changes"]],[1791432459,227491000],[3600],"utid1","key1"],1];</script>"#;

    fn registration() -> Registration {
        Registration::in_page(PAGE).unwrap()
    }

    #[test]
    fn the_registration_is_read_from_the_page() {
        assert_eq!(
            registration(),
            Registration {
                base: "https://chat.google.com/punctual/prod-02-us".into(),
                start: 1_791_432_459_227_491,
                lease: Duration::from_secs(3600),
                target: "utid1".into(),
                key: "key1".into(),
            }
        );
        assert_eq!(registration().path(), "/punctual/prod-02-us");

        assert_eq!(Registration::in_page("<html>no registration</html>"), None);
        let elsewhere = PAGE.replace("prod-dynamite-prod-02-us", "evil.example/x");
        assert_eq!(Registration::in_page(&elsewhere), None);
    }

    #[test]
    fn requests_match_what_the_web_client_sends() {
        let registration = registration();
        assert_eq!(
            registration.choose_server(),
            r#"[[null,null,null,[9,5],null,[["user-targeted-changes"],[null,1],[[["userTargeted"],["utid1"],["events"]]]]],null,null,0,0]"#
        );
        assert_eq!(
            registration.watch(100, false, false, 1),
            r#"[[[1,[100,null,null,[9,5],null,[["user-targeted-changes"],[null,1],[[["userTargeted"],["utid1"],["events"]]]],null,0,1],null,1]]]"#
        );
        assert!(
            registration
                .watch(300, true, true, 2)
                .ends_with("null,1,3],null,2]]]"),
            "resumed, after the channel dropped"
        );
    }

    #[test]
    fn the_authorization_hashes_the_time_the_secret_and_the_origin() {
        // sha1("1700000000 secret https://chat.google.com")
        assert_eq!(
            authorization("secret", 1_700_000_000),
            "SAPISIDHASH 1700000000_5da24ba425f21388a2cf1ee24b8a883a5bf91232"
        );
    }

    #[test]
    fn the_chosen_server_is_read_from_googles_reply() {
        assert_eq!(
            Server::parse(br#"["G1",1,null,"10","20"]"#),
            Some(Server {
                session: "G1".into(),
                stream_type: 1,
                delay: Duration::ZERO
            })
        );
        assert_eq!(
            Server::parse(br#"["G2","2",120000]"#).unwrap().delay,
            Duration::from_secs(30),
            "a long wait is cut short"
        );
        assert_eq!(Server::parse(br#"[""]"#), None);
    }

    #[test]
    fn a_batch_says_what_happened_to_each_watch() {
        let read = |batch: &str| updates(&serde_json::from_str(batch).unwrap());

        assert_eq!(
            read(r#"[null,null,["ch1"]]"#),
            [Update::Channel("ch1".into())]
        );
        assert_eq!(read(r#"[[["1",[["100"]]]]]"#), [Update::Started(1)]);
        assert_eq!(
            read(r#"[[[1,[null,[[[null,"CAE=","9"]]]]]]]"#),
            [Update::Change(1, vec![8, 1])]
        );
        assert_eq!(
            read(r#"[[["1",[null,null,["300"]]]]]"#),
            [Update::Watermark(1, 300)]
        );
        assert_eq!(
            read(r#"[[["7",null,[[14,null,"busy"]]]]]"#),
            [Update::Refused(7)]
        );
        assert_eq!(
            read(r#"[[["1",[null,null,null,null,[[1]]]]]]"#),
            [Update::Refused(1)]
        );
        assert_eq!(read(r#"[null,[1]]"#), [Update::Close]);
        assert_eq!(
            read(r#"[[[2,[["100"]]],[3,[null,[[[null,"CAE="]]]]]]]"#),
            [Update::Started(2), Update::Change(3, vec![8, 1])],
            "several watches in one batch"
        );
    }

    #[test]
    fn watching_matches_what_the_web_client_sends() {
        assert_eq!(
            watch_subject(2, &Subject::Person(UserId("123".into()))),
            r#"[[[2,[null,null,null,[9,5],null,[["user-state-changes"],[1],[[["state"],["user"],["123"],["availability"]]]],null,null,1]]]]"#
        );
        assert_eq!(
            watch_subject(27, &Subject::Typing(ConversationId::Dm("abc".into()))),
            r#"[[[27,[null,null,null,[9,5],null,[["group-state-changes"],[1],[[["state"],["group"],["abc"],["typing"]]]],null,null,1]]]]"#
        );
        assert_eq!(unwatch(2), "[[[2,null,[]]]]");
    }

    #[test]
    fn a_channels_watches_follow_what_is_wanted() {
        let mut watches = Watches {
            watching: HashMap::new(),
            watched_by: HashMap::new(),
            refused: HashSet::new(),
            next_watch: WATCH,
            request: 0,
            sent: 1,
        };
        let person = |id: &str| Subject::Person(UserId(id.into()));
        let typing = Subject::Typing(ConversationId::Dm("d".into()));

        let changes = watches.changes(&[person("a"), typing.clone()]);
        assert_eq!(
            changes,
            [watch_subject(2, &person("a")), watch_subject(3, &typing)]
        );
        assert_eq!(
            watches.changes(&[person("a"), typing.clone()]),
            [] as [String; 0]
        );

        let changes = watches.changes(&[typing.clone(), person("c")]);
        assert_eq!(changes, [unwatch(2), watch_subject(4, &person("c"))]);
        assert_eq!(watches.watched_by.get(&3), Some(&typing));

        watches.refused.insert(person("d"));
        assert_eq!(
            watches.changes(&[typing, person("c"), person("d")]),
            [] as [String; 0],
            "what the server would not watch is not asked for again"
        );
    }
}
