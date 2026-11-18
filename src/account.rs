//! The signed-in Google account: signing in, its direct messages, and the messages in them.

mod timeline;

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};

use futures::StreamExt as _;
use gpui::{App, AppContext as _, Context, EventEmitter, SharedString, Task};

use self::timeline::Sender;
pub use self::timeline::{Change, Entry, Timeline};
use crate::{
    cache,
    chat::{
        self, Client, ConversationId, Directory, Draft, Event, Message, MessageKind, Page,
        Presence, Service, Showing, Snapshot, Status as PersonStatus, UserId,
    },
    photos,
    session::{self, Session, SignIn},
    theme::ActiveTheme,
};

/// The largest unread count shown as a number.
const MOST_UNREAD_SHOWN: u32 = 99;
/// How old the list may get before coming back to Peep reloads it.
const STALE_AFTER: Duration = Duration::from_secs(20);
/// With the live channel up, how old the list may be when the channel reconnects before it
/// is reloaded.
const RELOAD_ON_RECONNECT_AFTER: Duration = Duration::from_secs(3);
/// How long a change to the list waits for more changes before the list is reloaded once.
const SETTLE: Duration = Duration::from_millis(500);
/// How many rows past each end of what the list shows have their people watched too, so
/// scrolling a little shows dots that are already current.
const WATCHED_BEYOND_VIEW: usize = 15;
/// The most people watched at once. Google's own client watches those on screen.
const MOST_WATCHED: usize = 100;
/// How long the list must hold still before the people watched change with it.
const WATCH_SETTLE: Duration = Duration::from_millis(300);
/// How often the watched people's do-not-disturb and notes are asked for while Peep is in
/// front. Google reports comings and goings as they happen, but not these.
const SETTINGS_EVERY: Duration = Duration::from_secs(5 * 60);
/// How often Chrome is asked whether the sign-in has finished.
const SIGN_IN_POLL: Duration = Duration::from_millis(400);
/// How many of the most recent conversations have their messages loaded ahead of being
/// opened, so opening them shows messages at once.
const LOADED_AHEAD: usize = 5;
/// How long someone counts as typing after Google last said so. Google says so every few
/// seconds while they type, and often does not say when they stop.
const TYPING_LASTS: Duration = Duration::from_secs(8);
/// How often Google is told the signed-in person is typing, while they are.
const TYPING_EVERY: Duration = Duration::from_secs(5);
/// What a message from the signed-in person is headed with.
const YOU: &str = "You";
const UNKNOWN_SENDER: &str = "Unknown";
/// Who is typing, when their name is not known.
const SOMEONE: &str = "Someone";
/// What a message with no text is shown as, where only text fits.
const ATTACHMENT: &str = "Sent an attachment";

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Looking for a session from an earlier launch.
    Starting,
    SignedOut,
    /// Chrome is open on Google's sign-in page.
    SigningIn,
    /// Signed in; the first load is under way.
    Loading,
    Ready,
    Failed(SharedString),
}

/// One row of the conversation list, ready to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub id: SharedString,
    pub title: SharedString,
    /// The unread count as shown, if there is anything unread.
    pub unread: Option<SharedString>,
    pub muted: bool,
    /// The other person, when the conversation is with one person.
    pub person: Option<UserId>,
    /// Where that person's photo is.
    pub photo: Option<SharedString>,
    /// Whether that person is around, once Google has said.
    pub presence: Option<Presence>,
    /// What that person says they are doing, or nothing.
    pub note: SharedString,
    /// Whether someone is typing in it.
    pub typing: bool,
}

/// Someone who can be named in a message.
#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub id: UserId,
    pub name: SharedString,
    /// Where their photo is, if they have one.
    pub photo: Option<SharedString>,
}

/// A message from someone else arrived.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
    pub conversation: SharedString,
    /// What the conversation is called.
    pub title: SharedString,
    pub sender: SharedString,
    /// The message's id, and its text or what stands for it.
    pub message: SharedString,
    pub text: SharedString,
}

/// A message could not be sent. It has been taken back out of the conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct SendFailed {
    pub conversation: SharedString,
    pub draft: Draft,
    pub error: SharedString,
}

/// The messages of a conversation changed.
#[derive(Clone, Debug, PartialEq)]
pub struct TimelineChanged {
    pub conversation: SharedString,
    pub change: Change,
}

pub struct Account {
    status: Status,
    rows: Vec<Row>,
    /// What the signed-in person chose to show others.
    showing: Showing,
    /// Wakes when do not disturb ends by itself, to say so.
    showing_task: Option<Task<()>>,
    /// The conversations and people behind the rows.
    directory: Directory,
    /// Messages loaded so far, by conversation.
    timelines: HashMap<SharedString, Timeline>,
    /// How people are doing, as last heard.
    statuses: HashMap<UserId, PersonStatus>,
    /// The rows the list shows, and the open conversation: their people are watched.
    rows_in_view: Range<usize>,
    open_conversation: Option<SharedString>,
    /// The people the live channel reports on as they come and go, and the conversations it
    /// reports typing in.
    watched: Vec<UserId>,
    watched_typing: Vec<ConversationId>,
    /// Who is typing, by conversation.
    typing: HashMap<SharedString, Vec<UserId>>,
    /// Each ends someone's typing unless Google says again that they are.
    typing_ends: HashMap<(SharedString, UserId), Task<()>>,
    /// When Google was last told the signed-in person is typing, by conversation.
    typing_said: HashMap<SharedString, Instant>,
    /// Those of them it has reported on since it connected. What it says about someone
    /// is newer than what asking says.
    reported: HashSet<UserId>,
    /// Whether the live channel has connected before, so a connection is a reconnection.
    reconnected: bool,
    /// Waits for the list to hold still before changing who is watched.
    watch_task: Option<Task<()>>,
    /// Asks for the watched people's settings every so often while Peep is in front.
    settings_task: Option<Task<()>>,
    /// When their settings were last asked for.
    settings_asked: Option<Instant>,
    /// False for an account made for a test or a preview, which starts no Chrome and keeps
    /// no files.
    live: bool,
    /// What the last launch kept, until a client takes it over.
    kept: Snapshot,
    client: Option<Rc<dyn Service>>,
    /// Whether the live channel is up, bringing changes as they happen.
    connected: bool,
    /// When the list was last asked for.
    asked: Option<Instant>,
    sign_in_task: Option<Task<()>>,
    load_task: Option<Task<()>>,
    /// Reads the live channel for as long as the client is in use.
    live_task: Option<Task<()>>,
    /// Waits out a burst of changes to the list before reloading it.
    settle_task: Option<Task<()>>,
}

impl EventEmitter<TimelineChanged> for Account {}
impl EventEmitter<Incoming> for Account {}
impl EventEmitter<SendFailed> for Account {}

impl Account {
    /// Shows what the last launch kept, and resumes its session if there is one.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let kept = cache::folder()
            .map(|folder| cache::read(&folder))
            .unwrap_or_default();
        let rows = rows(&kept.directory, &HashMap::new(), &HashMap::new());
        let status = if rows.is_empty() {
            Status::Starting
        } else {
            Status::Ready
        };
        let mut this = Self::with(status, rows);
        this.set_showing(kept.directory.showing, cx);
        this.directory = kept.directory.clone();
        this.kept = kept;
        this.live = true;
        this.resume(cx);
        this
    }

    /// An account in a given state, with no sign-in or network behind it.
    pub fn with(status: Status, rows: Vec<Row>) -> Self {
        Self {
            status,
            rows,
            showing: Showing::Automatic,
            showing_task: None,
            directory: Directory::default(),
            timelines: HashMap::new(),
            statuses: HashMap::new(),
            rows_in_view: 0..0,
            open_conversation: None,
            watched: Vec::new(),
            watched_typing: Vec::new(),
            typing: HashMap::new(),
            typing_ends: HashMap::new(),
            typing_said: HashMap::new(),
            reported: HashSet::new(),
            reconnected: false,
            watch_task: None,
            settings_task: None,
            settings_asked: None,
            live: false,
            kept: Snapshot::default(),
            client: None,
            connected: false,
            asked: None,
            sign_in_task: None,
            load_task: None,
            live_task: None,
            settle_task: None,
        }
    }

    /// An account whose Google Chat is `service`, already signed in.
    #[cfg(test)]
    pub(crate) fn over(service: Rc<dyn Service>, cx: &mut Context<Self>) -> Self {
        let mut this = Self::with(Status::Starting, Vec::new());
        this.attach(service, cx);
        this
    }

    /// Gives a test account direct messages with these ids, each with one person.
    #[cfg(test)]
    pub(crate) fn know(&mut self, ids: &[&str]) {
        use crate::chat::{Conversation, UserId};

        self.directory.me = Some(UserId("me".into()));
        self.directory.conversations = ids
            .iter()
            .map(|id| Conversation {
                id: ConversationId::Dm((*id).into()),
                title: format!("Person {id}").into(),
                members: Vec::new(),
                name: None,
                unread: 0,
                muted: false,
                sort_time: 0,
            })
            .collect();
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// How many conversations have something unread, leaving out muted ones. Google Chat's
    /// own total is not used: it counts conversations that were read long ago.
    pub fn unread(&self) -> u32 {
        u32::try_from(self.unread_rows(usize::MAX).count()).unwrap_or(u32::MAX)
    }

    /// What the signed-in person is showing others right now.
    pub fn showing(&self) -> Showing {
        self.showing.at(now())
    }

    /// Changes what the signed-in person shows others, on every device.
    pub fn show(&mut self, showing: Showing, cx: &mut Context<Self>) {
        let Some(client) = &self.client else { return };
        let saving = client.show(showing);
        let before = self.showing;
        // Shown at once; Google's answer follows.
        self.set_showing(showing, cx);
        cx.spawn(async move |this, cx| {
            let saved = saving.await;
            this.update(cx, |this, cx| match saved {
                Ok(saved) => this.set_showing(saved, cx),
                Err(chat::Error::SignedOut) => this.signed_out(cx),
                Err(error) => {
                    eprintln!("Peep could not change your status: {error}");
                    this.set_showing(before, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn set_showing(&mut self, showing: Showing, cx: &mut Context<Self>) {
        let showing = showing.at(now());
        self.showing_task = match showing {
            // Do not disturb ends by itself; whoever shows it is told when.
            Showing::DoNotDisturb { until } => {
                let left = Duration::from_micros(u64::try_from(until - now()).unwrap_or(0));
                let timer = cx.background_executor().timer(left);
                Some(cx.spawn(async move |this, cx| {
                    timer.await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                }))
            }
            _ => None,
        };
        if self.showing != showing {
            self.showing = showing;
            cx.notify();
        }
    }

    /// Up to `most` conversations with something unread, most recently active first. Muted
    /// ones are left out.
    pub fn unread_rows(&self, most: usize) -> impl Iterator<Item = &Row> {
        self.rows
            .iter()
            .filter(|row| row.unread.is_some() && !row.muted)
            .take(most)
    }

    /// Stops what only matters while Peep's window is open: watching the people on
    /// screen and asking for their settings.
    pub fn rest(&mut self, cx: &mut Context<Self>) {
        self.show_rows(0..0, None, cx);
        self.set_frontmost(false, cx);
    }

    /// The direct messages, most recently active first.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Says which rows the list shows and which conversation is open. The people in them
    /// are watched: the live channel reports their comings and goings as they happen.
    pub fn show_rows(
        &mut self,
        rows: Range<usize>,
        open: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        if (&self.rows_in_view, &self.open_conversation) != (&rows, &open) {
            self.rows_in_view = rows;
            self.open_conversation = open;
            self.watch_soon(cx);
        }
    }

    /// Says whether Peep's window is in front. While it is, the watched people's settings
    /// are asked for every so often.
    pub fn set_frontmost(&mut self, frontmost: bool, cx: &mut Context<Self>) {
        if !frontmost {
            self.settings_task = None;
        } else if self.settings_task.is_none() {
            let wait = self.settings_asked.map_or(SETTINGS_EVERY, |asked| {
                SETTINGS_EVERY.saturating_sub(asked.elapsed())
            });
            let timer = cx.background_executor().clone();
            self.settings_task = Some(cx.spawn(async move |this, cx| {
                let mut wait = wait;
                loop {
                    timer.timer(wait).await;
                    let asked = this.update(cx, |this, cx| this.ask(this.watched.clone(), cx));
                    if asked.is_err() {
                        return;
                    }
                    wait = SETTINGS_EVERY;
                }
            }));
        }
    }

    /// Changes who is watched once the list has held still for a moment, so scrolling
    /// through it does not watch everyone it passes.
    fn watch_soon(&mut self, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(WATCH_SETTLE);
        self.watch_task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| this.watch(cx)).ok();
        }));
    }

    fn watch(&mut self, cx: &mut Context<Self>) {
        let Some(client) = &self.client else { return };
        let in_view = rows_in_view(&self.rows, &self.rows_in_view, &self.open_conversation);
        let people = people_of(&in_view);
        let conversations: Vec<ConversationId> = in_view
            .iter()
            .filter_map(|row| {
                let known = &self.directory.conversations;
                Some(
                    known
                        .iter()
                        .find(|known| known.id.has_key(&row.id))?
                        .id
                        .clone(),
                )
            })
            .collect();
        // The same ones in another order need nothing done.
        if same(&people, &self.watched) && same(&conversations, &self.watched_typing) {
            return;
        }
        let new: Vec<UserId> = people
            .iter()
            .filter(|person| !self.watched.contains(person))
            .cloned()
            .collect();
        client.watch(people.clone(), conversations.clone());
        self.watched = people;
        self.watched_typing = conversations;
        // The channel reports changes from now on; how they are doing now is asked.
        self.ask(new, cx);
    }

    /// Asks how `people` are doing.
    fn ask(&mut self, people: Vec<UserId>, cx: &mut Context<Self>) {
        let Some(client) = self.client.as_ref().filter(|_| !people.is_empty()) else {
            return;
        };
        if people == self.watched {
            self.settings_asked = Some(Instant::now());
        }
        let asked = client.statuses(&people);
        cx.spawn(async move |this, cx| {
            // A failed request leaves what was last heard in place.
            if let Ok(statuses) = asked.await {
                this.update(cx, |this, cx| this.statuses_arrived(statuses, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(crate) fn statuses_arrived(
        &mut self,
        statuses: HashMap<UserId, PersonStatus>,
        cx: &mut Context<Self>,
    ) {
        for (person, mut status) in statuses {
            // Asking lags behind what the channel reported about someone.
            if self.reported.contains(&person)
                && let Some(known) = self.statuses.get(&person)
            {
                status.availability = known.availability;
            }
            self.statuses.insert(person, status);
        }
        self.statuses_changed(cx);
    }

    /// Redraws the rows whose people are doing differently.
    fn statuses_changed(&mut self, cx: &mut Context<Self>) {
        let before: Vec<_> = self
            .rows
            .iter()
            .map(|row| (row.presence, row.note.clone()))
            .collect();
        show_statuses(&mut self.rows, &self.statuses);
        let changed = self
            .rows
            .iter()
            .zip(before)
            .any(|(row, before)| (row.presence, &row.note) != (before.0, &before.1));
        if changed {
            cx.notify();
        }
    }

    /// The messages of a conversation, once [`Account::open`] has asked for them.
    pub fn timeline(&self, conversation: &SharedString) -> Option<&Timeline> {
        self.timelines.get(conversation)
    }

    /// Loads the newest messages of a conversation, unless they arrived a moment ago.
    pub fn open(&mut self, conversation: &SharedString, cx: &mut Context<Self>) {
        let Some((client, id)) = self.reach(conversation) else {
            return;
        };
        let timeline = self.timelines.entry(conversation.clone()).or_default();
        if !timeline.is_stale(self.connected) {
            return;
        }
        timeline.loading = true;
        cx.notify();
        let page = client.messages(&id, None);
        let conversation = conversation.clone();
        cx.spawn(async move |this, cx| {
            let page = page.await;
            this.update(cx, |this, cx| this.newest_arrived(&conversation, page, cx))
                .ok();
        })
        .detach();
    }

    /// Loads the messages before the oldest one loaded, if there are any.
    pub fn load_older(&mut self, conversation: &SharedString, cx: &mut Context<Self>) {
        // Scrolling asks many times a second, so the cheap check comes first.
        let Some(before) = self.timelines.get(conversation).and_then(Timeline::older) else {
            return;
        };
        let Some((client, id)) = self.reach(conversation) else {
            return;
        };
        if let Some(timeline) = self.timelines.get_mut(conversation) {
            timeline.loading_older = true;
        }
        cx.notify();
        let page = client.messages(&id, Some(before));
        let conversation = conversation.clone();
        cx.spawn(async move |this, cx| {
            let page = page.await;
            this.update(cx, |this, cx| this.older_arrived(&conversation, page, cx))
                .ok();
        })
        .detach();
    }

    /// Tells Google the person has read a conversation up to its newest loaded message, and
    /// clears its unread count.
    pub fn mark_read(&mut self, conversation: &SharedString, cx: &mut Context<Self>) {
        let unread = self
            .rows
            .iter_mut()
            .find(|row| row.id == *conversation)
            .filter(|row| row.unread.is_some());
        let newest = self
            .timelines
            .get(conversation)
            .and_then(Timeline::newest_time);
        let (Some(row), Some(newest)) = (unread, newest) else {
            return;
        };
        row.unread = None;
        cx.notify();
        let Some((client, id)) = self.reach(conversation) else {
            return;
        };
        if let Some(conversation) = self
            .directory
            .conversations
            .iter_mut()
            .find(|known| known.id == id)
        {
            conversation.unread = 0;
        }
        let marked = client.mark_read(&id, newest);
        cx.background_spawn(async move {
            if let Err(error) = marked.await {
                eprintln!("Peep could not mark a conversation read: {error}");
            }
        })
        .detach();
    }

    pub(crate) fn newest_arrived(
        &mut self,
        conversation: &SharedString,
        page: Result<Page, chat::Error>,
        cx: &mut Context<Self>,
    ) {
        let directory = &self.directory;
        let timeline = self.timelines.entry(conversation.clone()).or_default();
        let change = match page {
            Ok(page) => timeline.merge_newest(page, |id, name| sender_of(directory, id, name)),
            Err(error) => {
                timeline.loading = false;
                // A failed reload leaves the messages already showing in place.
                if !timeline.is_loaded() {
                    timeline.failed = Some(error.to_string().into());
                }
                None
            }
        };
        self.changed(conversation, change, cx);
    }

    pub(crate) fn older_arrived(
        &mut self,
        conversation: &SharedString,
        page: Result<Page, chat::Error>,
        cx: &mut Context<Self>,
    ) {
        let directory = &self.directory;
        let Some(timeline) = self.timelines.get_mut(conversation) else {
            return;
        };
        let change = match page {
            Ok(page) => timeline.merge_older(page, |id, name| sender_of(directory, id, name)),
            Err(_) => {
                // Scrolling up again asks again.
                timeline.loading_older = false;
                None
            }
        };
        self.changed(conversation, change, cx);
    }

    fn changed(
        &mut self,
        conversation: &SharedString,
        change: Option<Change>,
        cx: &mut Context<Self>,
    ) {
        if let Some(change) = change {
            cx.emit(TimelineChanged {
                conversation: conversation.clone(),
                change,
            });
        }
        cx.notify();
    }

    /// The client and the id to ask it about a conversation with.
    fn reach(&self, conversation: &SharedString) -> Option<(Rc<dyn Service>, ConversationId)> {
        let id = self
            .directory
            .conversations
            .iter()
            .find(|known| known.id.has_key(conversation))?
            .id
            .clone();
        Some((self.client.clone()?, id))
    }

    /// Opens Chrome on Google's sign-in page and waits for the person to finish there.
    /// Closing that window cancels.
    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        if !self.live {
            return;
        }
        let size = cx.theme().size.sign_in_window;
        let (width, height) = (f32::from(size.width) as u32, f32::from(size.height) as u32);
        let timer = cx.background_executor().clone();
        // Dropping this task drops the sign-in, which closes Chrome.
        let signing_in = cx.background_spawn(async move {
            let mut sign_in = SignIn::open(width, height)?;
            loop {
                timer.timer(SIGN_IN_POLL).await;
                if let Some(session) = sign_in.poll()? {
                    sign_in.finish();
                    return Ok(session);
                }
            }
        });
        self.set_status(Status::SigningIn, cx);
        self.sign_in_task = Some(cx.spawn(async move |this, cx| {
            let result: Result<Session, session::Error> = signing_in.await;
            this.update(cx, |this, cx| match result {
                Ok(session) => this.connect(session, cx),
                Err(session::Error::Closed) => this.set_status(Status::SignedOut, cx),
                Err(error) => this.set_status(Status::Failed(error.to_string().into()), cx),
            })
            .ok();
        }));
    }

    /// Reloads the list if it has not been asked for in a while. This keeps the list
    /// current while the live channel is down; while it is up, changes arrive by themselves.
    pub fn refresh_if_stale(&mut self, cx: &mut Context<Self>) {
        if !self.connected
            && self
                .asked
                .is_some_and(|asked| asked.elapsed() > STALE_AFTER)
        {
            self.load(cx);
        }
    }

    /// Looks for a session again, or reloads with the one in use.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if self.client.is_some() {
            self.load(cx);
        } else {
            self.resume(cx);
        }
    }

    fn resume(&mut self, cx: &mut Context<Self>) {
        if !self.live {
            return;
        }
        let stored = cx.background_spawn(async { session::stored() });
        if self.rows.is_empty() {
            self.set_status(Status::Starting, cx);
        }
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = stored.await;
            this.update(cx, |this, cx| match result {
                Ok(Some(session)) => this.connect(session, cx),
                Ok(None) => this.signed_out(cx),
                Err(error) => this.set_status(Status::Failed(error.to_string().into()), cx),
            })
            .ok();
        }));
    }

    fn connect(&mut self, session: Session, cx: &mut Context<Self>) {
        match Client::new(session, std::mem::take(&mut self.kept)) {
            Ok(client) => self.attach(Rc::new(client), cx),
            Err(error) => self.set_status(Status::Failed(error.to_string().into()), cx),
        }
    }

    /// Starts using `service`: listens to its live channel and loads the list.
    fn attach(&mut self, service: Rc<dyn Service>, cx: &mut Context<Self>) {
        let mut events = service.events();
        self.live_task = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |this, cx| this.heard(event, cx)).is_err() {
                    break;
                }
            }
        }));
        self.client = Some(service);
        self.load(cx);
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(client) = &self.client else { return };
        let refresh = client.refresh();
        self.asked = Some(Instant::now());
        if self.rows.is_empty() {
            self.set_status(Status::Loading, cx);
        }
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = refresh.await;
            this.update(cx, |this, cx| this.loaded(result, cx)).ok();
        }));
    }

    fn loaded(&mut self, result: Result<Snapshot, chat::Error>, cx: &mut Context<Self>) {
        match result {
            Ok(snapshot) => {
                self.rows = rows(&snapshot.directory, &self.statuses, &self.typing);
                self.set_showing(snapshot.directory.showing, cx);
                self.directory = snapshot.directory.clone();
                self.set_status(Status::Ready, cx);
                // The same rows may now hold other people.
                self.watch_soon(cx);
                let recent: Vec<SharedString> = self
                    .rows
                    .iter()
                    .take(LOADED_AHEAD)
                    .map(|row| row.id.clone())
                    .filter(|id| !self.timelines.contains_key(id))
                    .collect();
                for conversation in recent {
                    self.open(&conversation, cx);
                }
                if self.live {
                    cx.background_spawn(async move {
                        if let Some(folder) = cache::folder()
                            && let Err(error) = cache::write(&folder, &snapshot)
                        {
                            eprintln!("Peep could not keep its snapshot: {error}");
                        }
                    })
                    .detach();
                }
            }
            Err(chat::Error::SignedOut) => self.signed_out(cx),
            // A failed refresh leaves what is already showing in place.
            Err(_) if !self.rows.is_empty() => {}
            Err(error) => self.set_status(Status::Failed(error.to_string().into()), cx),
        }
    }

    /// Takes in what the live channel reported.
    pub(crate) fn heard(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Connected { gap } => {
                self.connected = true;
                // A new channel says nothing about what happened while there was none.
                self.reported.clear();
                if std::mem::replace(&mut self.reconnected, true) {
                    self.ask(self.watched.clone(), cx);
                }
                if gap {
                    for timeline in self.timelines.values_mut() {
                        timeline.outdated = true;
                    }
                }
                // The channel starts from when it connected; the list may be older.
                if gap
                    || self
                        .asked
                        .is_none_or(|asked| asked.elapsed() > RELOAD_ON_RECONNECT_AFTER)
                {
                    self.load(cx);
                }
                cx.notify();
            }
            Event::Disconnected => self.connected = false,
            Event::SignedOut => self.signed_out(cx),
            Event::Message {
                conversation,
                message,
                posted,
            } => self.message_arrived(conversation, message, posted, cx),
            Event::Removed {
                conversation,
                message,
            } => {
                let key: SharedString = conversation.key().into();
                let directory = &self.directory;
                let change = self.timelines.get_mut(&key).and_then(|timeline| {
                    timeline.remove(&message, |id, name| sender_of(directory, id, name))
                });
                if change.is_some() {
                    self.changed(&key, change, cx);
                }
            }
            Event::Read {
                conversation,
                unread,
            } => {
                if let Some(known) = self
                    .directory
                    .conversations
                    .iter_mut()
                    .find(|known| known.id == conversation)
                    && (known.unread > 0) != unread
                {
                    known.unread = u32::from(unread);
                    self.rows = rows(&self.directory, &self.statuses, &self.typing);
                    cx.notify();
                }
            }
            Event::Status {
                person, showing, ..
            } if self.directory.me.as_ref() == Some(&person) => self.set_showing(showing, cx),
            Event::Status {
                person,
                quiet,
                note,
                ..
            } => {
                // Only someone already asked about is shown, so only they are kept.
                if let Some(status) = self.statuses.get_mut(&person) {
                    status.quiet = quiet;
                    status.note = note;
                    self.statuses_changed(cx);
                }
            }
            Event::Availability {
                person,
                availability,
            } => {
                self.statuses
                    .entry(person.clone())
                    .or_default()
                    .availability = availability;
                self.reported.insert(person);
                self.statuses_changed(cx);
            }
            Event::Reactions {
                conversation,
                message,
                reactions,
            } => {
                let key: SharedString = conversation.key().into();
                let directory = &self.directory;
                let timeline = self.timelines.get_mut(&key);
                let change = timeline.and_then(|timeline| {
                    timeline.react(&message, reactions, |id, name| {
                        sender_of(directory, id, name)
                    })
                });
                if change.is_some() {
                    self.changed(&key, change, cx);
                }
            }
            Event::Typing {
                conversation,
                person,
                typing,
            } => {
                if self.directory.me.as_ref() != Some(&person) {
                    self.set_typing(conversation.key().into(), person, typing, cx);
                }
            }
            Event::ListChanged => self.reload_soon(cx),
            Event::Other(_) => {}
        }
    }

    fn message_arrived(
        &mut self,
        conversation: ConversationId,
        message: Message,
        posted: bool,
        cx: &mut Context<Self>,
    ) {
        let me = self.directory.me.clone();
        let Some(known) = self
            .directory
            .conversations
            .iter_mut()
            .find(|known| known.id == conversation)
        else {
            // Someone new wrote. Spaces report here too, and Peep does not list those.
            if posted && matches!(conversation, ConversationId::Dm(_)) {
                self.reload_soon(cx);
            }
            return;
        };
        let key: SharedString = conversation.key().into();
        if posted {
            known.sort_time = known.sort_time.max(message.time);
            let from_someone_else =
                message.kind == MessageKind::Sent && me.as_ref() != Some(&message.sender);
            if from_someone_else {
                known.unread = known.unread.saturating_add(1);
                if !known.muted {
                    let incoming = Incoming {
                        conversation: key.clone(),
                        title: SharedString::from(known.title.clone()),
                        sender: sender_of(&self.directory, &message.sender, &message.sender_name)
                            .name,
                        message: SharedString::from(message.id.clone()),
                        text: preview(&message),
                    };
                    cx.emit(incoming);
                }
            }
            // Whoever wrote it has stopped typing it.
            self.set_typing(key.clone(), message.sender.clone(), false, cx);
            self.directory
                .conversations
                .sort_by_key(|conversation| std::cmp::Reverse(conversation.sort_time));
            self.rows = rows(&self.directory, &self.statuses, &self.typing);
            // The same rows now hold other people.
            self.watch_soon(cx);
        }

        let directory = &self.directory;
        let change = self.timelines.get_mut(&key).and_then(|timeline| {
            timeline.apply(message, posted, |id, name| sender_of(directory, id, name))
        });
        self.changed(&key, change, cx);
    }

    /// Notes that someone is typing in a conversation, or has stopped. Typing ends by
    /// itself unless Google says again that it goes on.
    fn set_typing(
        &mut self,
        conversation: SharedString,
        person: UserId,
        typing: bool,
        cx: &mut Context<Self>,
    ) {
        let typists = self.typing.entry(conversation.clone()).or_default();
        let was_typing = typists.contains(&person);
        let ends = (conversation.clone(), person.clone());
        if typing {
            if !was_typing {
                typists.push(person.clone());
            }
            let timer = cx.background_executor().timer(TYPING_LASTS);
            let ended = ends.clone();
            let end = cx.spawn(async move |this, cx| {
                timer.await;
                let (conversation, person) = ended;
                this.update(cx, |this, cx| {
                    this.set_typing(conversation, person, false, cx)
                })
                .ok();
            });
            self.typing_ends.insert(ends, end);
        } else {
            typists.retain(|typist| *typist != person);
            self.typing_ends.remove(&ends);
        }
        if self.typing.get(&conversation).is_some_and(Vec::is_empty) {
            self.typing.remove(&conversation);
        }
        if was_typing != typing {
            if let Some(row) = self.rows.iter_mut().find(|row| row.id == conversation) {
                row.typing = self.typing.contains_key(&conversation);
            }
            cx.notify();
        }
    }

    /// The names of those typing in a conversation, in the order they started.
    pub fn typing_in(&self, conversation: &SharedString) -> Vec<SharedString> {
        let typists = self.typing.get(conversation).into_iter().flatten();
        typists
            .map(|person| {
                let name = self.directory.users.get(person).map(|user| &user.name);
                name.filter(|name| !name.is_empty())
                    .map_or(SOMEONE.into(), |name| SharedString::from(name.clone()))
            })
            .collect()
    }

    /// Tells Google the signed-in person is typing in a conversation. Called as they type,
    /// it tells Google only every few seconds.
    pub fn typing(&mut self, conversation: &SharedString, cx: &mut Context<Self>) {
        let said = self.typing_said.get(conversation);
        if said.is_some_and(|said| said.elapsed() < TYPING_EVERY) {
            return;
        }
        let Some((client, id)) = self.reach(conversation) else {
            return;
        };
        self.typing_said
            .insert(conversation.clone(), Instant::now());
        let said = client.typing(&id);
        // Nothing is lost if this fails: the next keystroke after a while tries again.
        cx.background_spawn(async move { drop(said.await) })
            .detach();
    }

    /// The people who can be named in a message to a conversation: everyone in it but the
    /// signed-in person.
    pub fn people_in(&self, conversation: &SharedString) -> Vec<Person> {
        let known = &self.directory.conversations;
        let members = known
            .iter()
            .find(|known| known.id.has_key(conversation))
            .map_or(&[][..], |known| known.members.as_slice());
        members
            .iter()
            .filter_map(|id| {
                let user = self.directory.users.get(id)?;
                Some(Person {
                    id: id.clone(),
                    name: SharedString::from(user.name.clone()),
                    photo: photo(&user.photo),
                })
            })
            .filter(|person| !person.name.is_empty())
            .collect()
    }

    /// Finds people in the organisation whose name starts with `typed`, other than the
    /// signed-in person. A search that fails finds no one.
    pub fn find_people(&self, typed: &str, cx: &App) -> Task<Vec<Person>> {
        let Some(client) = &self.client else {
            return Task::ready(Vec::new());
        };
        let found = client.people(typed);
        let me = self.directory.me.clone();
        cx.background_spawn(async move {
            let found = found.await.unwrap_or_default().into_iter();
            found
                .filter(|(id, user)| Some(id) != me.as_ref() && !user.name.is_empty())
                .map(|(id, user)| Person {
                    id,
                    name: SharedString::from(user.name),
                    photo: photo(&user.photo),
                })
                .collect()
        })
    }

    /// Sends a message to a conversation. It shows there at once; if Google will not take
    /// it, it is taken back out and [`SendFailed`] says why.
    pub fn send(&mut self, conversation: &SharedString, draft: Draft, cx: &mut Context<Self>) {
        let (Some((client, id)), Some(me)) = (self.reach(conversation), self.directory.me.clone())
        else {
            return;
        };
        if draft.text.trim().is_empty() {
            return;
        }
        let message_id = chat::new_id();
        let sending = client.send(&id, &draft, &message_id);
        let shown = Message {
            id: message_id.clone().into(),
            sender: me,
            sender_name: "".into(),
            time: now(),
            kind: MessageKind::Sent,
            text: draft.text.clone().into(),
            attachments: Vec::new(),
            edited: false,
            marks: Vec::new(),
            reactions: Vec::new(),
            quote: None,
        };
        self.message_arrived(id.clone(), shown, true, cx);
        // The next thing typed is a new message, said to be typed at once.
        self.typing_said.remove(conversation);
        let conversation = conversation.clone();
        cx.spawn(async move |this, cx| {
            let sent = sending.await;
            this.update(cx, |this, cx| {
                let kept = sent.as_ref().is_ok_and(|sent| *sent.id == *message_id);
                if !kept {
                    // Google kept it under another id, or not at all.
                    let directory = &this.directory;
                    let timeline = this.timelines.get_mut(&conversation);
                    let change = timeline.and_then(|timeline| {
                        timeline.remove(&message_id, |id, name| sender_of(directory, id, name))
                    });
                    this.changed(&conversation, change, cx);
                }
                match sent {
                    Ok(sent) => this.message_arrived(id, sent, true, cx),
                    Err(chat::Error::SignedOut) => this.signed_out(cx),
                    Err(error) => cx.emit(SendFailed {
                        conversation,
                        draft,
                        error: error.to_string().into(),
                    }),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Reloads the list once a burst of changes to it has passed.
    fn reload_soon(&mut self, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(SETTLE);
        self.settle_task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| this.load(cx)).ok();
        }));
    }

    /// Google no longer takes the session: forget what was kept for it.
    fn signed_out(&mut self, cx: &mut Context<Self>) {
        self.client = None;
        self.live_task = None;
        self.connected = false;
        self.kept = Snapshot::default();
        self.rows.clear();
        self.set_showing(Showing::Automatic, cx);
        self.directory = Directory::default();
        self.timelines.clear();
        self.statuses.clear();
        self.watched.clear();
        self.watched_typing.clear();
        self.typing.clear();
        self.typing_ends.clear();
        self.typing_said.clear();
        self.reported.clear();
        self.watch_task = None;
        self.settings_task = None;
        self.set_status(Status::SignedOut, cx);
        if self.live {
            cx.background_spawn(async {
                photos::forget();
                if let Some(folder) = cache::folder()
                    && let Err(error) = cache::clear(&folder)
                {
                    eprintln!("Peep could not forget its snapshot: {error}");
                }
            })
            .detach();
        }
    }

    fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.status = status;
        cx.notify();
    }
}

/// Now, in microseconds since the Unix epoch.
fn now() -> i64 {
    (session::now() * 1_000_000.) as i64
}

/// How someone is shown as the sender of a message. `carried` is the name the message
/// itself gives them, used when nothing else is known of them.
fn sender_of(directory: &Directory, person: &UserId, carried: &str) -> Sender {
    let known = directory.users.get(person);
    let photo = known.and_then(|user| photo(&user.photo));
    if directory.me.as_ref() == Some(person) {
        return Sender {
            name: YOU.into(),
            photo,
            me: true,
        };
    }
    let name = known
        .map(|user| {
            if user.name.is_empty() {
                &user.email
            } else {
                &user.name
            }
        })
        .filter(|name| !name.is_empty());
    let name = match name {
        Some(name) => SharedString::from(name.clone()),
        None if !carried.is_empty() => SharedString::from(carried.to_owned()),
        None => UNKNOWN_SENDER.into(),
    };
    Sender {
        name,
        photo,
        me: false,
    }
}

/// A message in a line or two, for a notification.
fn preview(message: &Message) -> SharedString {
    let text = message.text.trim();
    match (text.is_empty(), message.attachments.first()) {
        (false, _) => SharedString::from(text.to_owned()),
        (true, Some(name)) => SharedString::from(name.clone()),
        (true, None) => ATTACHMENT.into(),
    }
}

fn photo(address: &str) -> Option<SharedString> {
    (!address.is_empty()).then(|| SharedString::from(address.to_owned()))
}

/// The rows to watch the people and the typing of: those the list shows and a little
/// beyond, and the open conversation first. A list showing no rows has none beyond them.
fn rows_in_view<'a>(
    rows: &'a [Row],
    in_view: &Range<usize>,
    open: &Option<SharedString>,
) -> Vec<&'a Row> {
    let beyond = if in_view.is_empty() {
        0
    } else {
        WATCHED_BEYOND_VIEW
    };
    let start = in_view.start.saturating_sub(beyond).min(rows.len());
    let end = (in_view.end + beyond).min(rows.len());
    let open = rows.iter().find(|row| open.as_ref() == Some(&row.id));
    let mut watched: Vec<&Row> = Vec::new();
    for row in open.into_iter().chain(&rows[start..end]) {
        if watched.len() < MOST_WATCHED && !watched.iter().any(|seen| seen.id == row.id) {
            watched.push(row);
        }
    }
    watched
}

/// Whether two lists hold the same things, in any order.
fn same<T: PartialEq>(one: &[T], other: &[T]) -> bool {
    one.len() == other.len() && one.iter().all(|item| other.contains(item))
}

/// The people those rows are with, each once.
fn people_of(rows: &[&Row]) -> Vec<UserId> {
    let mut people: Vec<UserId> = Vec::new();
    for person in rows.iter().filter_map(|row| row.person.as_ref()) {
        if !people.contains(person) {
            people.push(person.clone());
        }
    }
    people
}

/// Puts how each row's person is doing into the rows.
fn show_statuses(rows: &mut [Row], statuses: &HashMap<UserId, PersonStatus>) {
    for row in rows {
        let status = row.person.as_ref().and_then(|person| statuses.get(person));
        row.presence = status.map(PersonStatus::presence);
        row.note = status.map_or_else(SharedString::default, |status| {
            SharedString::from(status.note.clone())
        });
    }
}

/// The conversation list as drawn, most recently active first.
fn rows(
    directory: &Directory,
    statuses: &HashMap<UserId, PersonStatus>,
    typing: &HashMap<SharedString, Vec<UserId>>,
) -> Vec<Row> {
    let mut rows: Vec<Row> = directory
        .conversations
        .iter()
        .map(|conversation| {
            let person = match (&conversation.id, conversation.members.as_slice()) {
                (ConversationId::Dm(_), [person]) => Some(person.clone()),
                _ => None,
            };
            let id = SharedString::from(conversation.id.key());
            Row {
                typing: typing.contains_key(&id),
                id,
                title: SharedString::from(conversation.title.clone()),
                unread: match conversation.unread {
                    0 => None,
                    unread if unread > MOST_UNREAD_SHOWN => {
                        Some(format!("{MOST_UNREAD_SHOWN}+").into())
                    }
                    unread => Some(unread.to_string().into()),
                },
                muted: conversation.muted,
                photo: person
                    .as_ref()
                    .and_then(|person| directory.users.get(person))
                    .and_then(|user| photo(&user.photo)),
                person,
                presence: None,
                note: SharedString::default(),
            }
        })
        .collect();
    show_statuses(&mut rows, statuses);
    rows
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;
    use crate::chat::{Availability, Conversation, User};

    fn directory() -> Directory {
        let person = |name: &str, photo: &str| User {
            name: name.into(),
            email: "".into(),
            photo: photo.into(),
        };
        let conversation = |id: ConversationId, members: &[&str], sort_time| Conversation {
            id,
            title: members.join(", ").into(),
            members: members.iter().map(|id| UserId((*id).into())).collect(),
            name: None,
            unread: 0,
            muted: false,
            sort_time,
        };
        Directory {
            me: Some(UserId("me".into())),
            showing: chat::Showing::Automatic,
            conversations: vec![
                conversation(ConversationId::Dm("a".into()), &["ada"], 3),
                conversation(ConversationId::Space("s".into()), &["ada", "grace"], 2),
                conversation(ConversationId::Dm("g".into()), &["grace"], 1),
            ],
            users: [
                (UserId("ada".into()), person("Ada", "//photos/ada")),
                (UserId("grace".into()), person("Grace", "")),
                (UserId("me".into()), person("Me", "//photos/me")),
            ]
            .into(),
        }
    }

    fn status(availability: Availability, quiet: bool, note: &str) -> PersonStatus {
        PersonStatus {
            availability,
            quiet,
            note: note.into(),
        }
    }

    #[test]
    fn a_row_with_one_person_shows_their_photo_and_how_they_are_doing() {
        let statuses = [
            (
                UserId("ada".into()),
                status(Availability::Idle, false, "Lunch"),
            ),
            (
                UserId("grace".into()),
                status(Availability::Active, true, ""),
            ),
        ]
        .into();

        let rows = rows(&directory(), &statuses, &HashMap::new());

        let shown: Vec<_> = rows
            .iter()
            .map(|row| {
                let photo = row.photo.as_ref().map(|photo| photo.as_ref());
                (photo, row.presence, row.note.as_ref())
            })
            .collect();
        assert_eq!(
            shown,
            [
                (Some("//photos/ada"), Some(Presence::Idle), "Lunch"),
                (None, None, ""),
                (None, Some(Presence::DoNotDisturb), ""),
            ],
            "a group shows neither; someone without a photo shows none"
        );
    }

    #[test]
    fn a_message_is_from_you_or_from_the_person_by_name() {
        let directory = directory();
        let from = |sender: &str, name: &str| {
            let message = Message {
                id: "m".into(),
                sender: UserId(sender.into()),
                sender_name: name.into(),
                time: 0,
                kind: MessageKind::Sent,
                text: "".into(),
                attachments: Vec::new(),
                edited: false,
                marks: Vec::new(),
                reactions: Vec::new(),
                quote: None,
            };
            let sender = sender_of(&directory, &message.sender, &message.sender_name);
            let photo = sender.photo.map(|photo| photo.to_string());
            (sender.name.to_string(), photo)
        };

        assert_eq!(from("me", ""), ("You".into(), Some("//photos/me".into())));
        assert_eq!(from("ada", ""), ("Ada".into(), Some("//photos/ada".into())));
        assert_eq!(from("stranger", "Alan"), ("Alan".into(), None));
        assert_eq!(from("stranger", ""), ("Unknown".into(), None));
    }

    #[test]
    fn the_people_watched_are_those_on_screen_and_in_the_open_conversation() {
        let row = |id: &str, person: Option<&str>| Row {
            id: id.to_owned().into(),
            person: person.map(|person| UserId(person.into())),
            ..Default::default()
        };
        let mut rows: Vec<Row> = (0..200)
            .map(|n| row(&format!("dm/{n}"), Some(&format!("p{n}"))))
            .collect();
        rows[41] = row("space/s", None);
        rows[42] = row("dm/again", Some("p40"));
        let watched = |in_view: Range<usize>, open: Option<&str>| {
            let open = open.map(|open| SharedString::from(open.to_owned()));
            let people = people_of(&rows_in_view(&rows, &in_view, &open));
            people
                .iter()
                .map(|person| person.0.to_string())
                .collect::<Vec<_>>()
        };

        let near = watched(40..44, None);
        assert_eq!(
            near.first().map(String::as_str),
            Some("p25"),
            "a little before"
        );
        assert_eq!(
            near.last().map(String::as_str),
            Some("p58"),
            "and a little after"
        );
        assert_eq!(
            near.len(),
            32,
            "a group has no one to watch; no one is watched twice"
        );

        let with_open = watched(0..2, Some("dm/150"));
        assert_eq!(with_open[..2], ["p150", "p0"]);
        assert_eq!(watched(0..2, Some("dm/1")).len(), 17, "already on screen");

        assert_eq!(
            watched(0..200, None).len(),
            MOST_WATCHED - 2,
            "the first hundred rows, whoever is in them"
        );
        assert_eq!(watched(500..600, None), Vec::<String>::new());
        assert_eq!(watched(0..0, None), Vec::<String>::new(), "no list, no one");
    }

    #[gpui::test]
    fn what_i_show_follows_other_devices_and_ends_by_itself(cx: &mut TestAppContext) {
        let account = cx.new(|_| {
            let mut account = Account::with(Status::Ready, Vec::new());
            account.directory = directory();
            account
        });
        let showing =
            |cx: &mut TestAppContext| account.read_with(cx, |account, _| account.showing());
        let hear = |person: &str, showing: Showing, cx: &mut TestAppContext| {
            let event = Event::Status {
                person: UserId(person.into()),
                showing,
                quiet: false,
                note: "".into(),
            };
            account.update(cx, |account, cx| account.heard(event, cx));
        };
        assert_eq!(showing(cx), Showing::Automatic);

        hear("ada", Showing::Away, cx);
        assert_eq!(showing(cx), Showing::Automatic, "someone else's status");

        hear("me", Showing::Away, cx);
        assert_eq!(showing(cx), Showing::Away, "set on another device");

        let later = Showing::DoNotDisturb {
            until: now() + 60_000_000,
        };
        hear("me", later, cx);
        assert_eq!(showing(cx), later);

        hear("me", Showing::DoNotDisturb { until: now() - 1 }, cx);
        assert_eq!(showing(cx), Showing::Automatic, "it has already ended");
    }

    #[test]
    fn the_unread_rows_are_the_newest_few() {
        let row = |id: &str, unread: bool| Row {
            id: id.to_owned().into(),
            unread: unread.then(|| "1".into()),
            ..Default::default()
        };
        let rows = vec![
            row("a", true),
            row("b", false),
            row("c", true),
            row("d", true),
        ];
        let account = Account::with(Status::Ready, rows);

        let ids = |most| {
            account
                .unread_rows(most)
                .map(|row| row.id.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(2), ["a", "c"]);
        assert_eq!(ids(5), ["a", "c", "d"]);
    }

    #[gpui::test]
    fn what_is_heard_about_a_person_changes_their_row(cx: &mut TestAppContext) {
        let account = cx.new(|_| {
            let mut account = Account::with(Status::Ready, Vec::new());
            account.directory = directory();
            account.rows = rows(&account.directory, &account.statuses, &account.typing);
            account
        });
        let shown = |row: usize, cx: &mut TestAppContext| {
            account.read_with(cx, |account, _| {
                let row = &account.rows()[row];
                (row.presence, row.note.to_string())
            })
        };
        let hear = |event: Event, cx: &mut TestAppContext| {
            account.update(cx, |account, cx| account.heard(event, cx));
        };
        let came_or_went = |person: &str, availability| Event::Availability {
            person: UserId(person.into()),
            availability,
        };
        let asked = |answers: &[(&str, PersonStatus)], cx: &mut TestAppContext| {
            let answers = answers
                .iter()
                .map(|(person, status)| (UserId((*person).into()), status.clone()))
                .collect();
            account.update(cx, |account, cx| account.statuses_arrived(answers, cx));
        };
        assert_eq!(shown(0, cx).0, None, "nothing is known yet");

        asked(&[("ada", status(Availability::Away, false, "Lunch"))], cx);
        assert_eq!(shown(0, cx), (Some(Presence::Away), "Lunch".into()));

        hear(came_or_went("ada", Availability::Active), cx);
        assert_eq!(shown(0, cx).0, Some(Presence::Active), "as it happens");

        asked(&[("ada", status(Availability::Away, true, ""))], cx);
        hear(
            Event::Status {
                person: UserId("ada".into()),
                showing: chat::Showing::Automatic,
                quiet: false,
                note: "Back".into(),
            },
            cx,
        );
        assert_eq!(
            shown(0, cx),
            (Some(Presence::Active), "Back".into()),
            "asking does not undo what the channel reported, but brings her settings"
        );

        hear(came_or_went("grace", Availability::Idle), cx);
        assert_eq!(
            shown(2, cx).0,
            Some(Presence::Idle),
            "someone not yet asked about"
        );

        hear(Event::Connected { gap: false }, cx);
        asked(&[("ada", status(Availability::Away, false, ""))], cx);
        assert_eq!(
            shown(0, cx).0,
            Some(Presence::Away),
            "a new channel knows nothing of before, so asking is believed"
        );
    }
}
