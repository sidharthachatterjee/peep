//! The signed-in Google account: signing in, and the direct messages loaded for it.

use std::time::{Duration, Instant};

use gpui::{AppContext as _, Context, SharedString, Task};

use crate::{
    cache,
    chat::{self, Client, Directory, Snapshot},
    session::{self, Session, SignIn},
    theme::ActiveTheme,
};

/// The largest unread count shown as a number.
const MOST_UNREAD_SHOWN: u32 = 99;
/// How old the list may get before coming back to Peep reloads it.
const STALE_AFTER: Duration = Duration::from_secs(20);
/// How often Chrome is asked whether the sign-in has finished.
const SIGN_IN_POLL: Duration = Duration::from_millis(400);

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
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: SharedString,
    pub title: SharedString,
    /// The unread count as shown, if there is anything unread.
    pub unread: Option<SharedString>,
    pub muted: bool,
}

pub struct Account {
    status: Status,
    rows: Vec<Row>,
    /// False for an account made with [`Account::with`], which never starts Chrome.
    live: bool,
    /// What the last launch kept, until a client takes it over.
    kept: Snapshot,
    client: Option<Client>,
    /// When the list was last asked for.
    asked: Option<Instant>,
    sign_in_task: Option<Task<()>>,
    load_task: Option<Task<()>>,
}

impl Account {
    /// Shows what the last launch kept, and resumes its session if there is one.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let kept = cache::folder()
            .map(|folder| cache::read(&folder))
            .unwrap_or_default();
        let rows = rows(&kept.directory);
        let status = if rows.is_empty() {
            Status::Starting
        } else {
            Status::Ready
        };
        let mut this = Self::with(status, rows);
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
            live: false,
            kept: Snapshot::default(),
            client: None,
            asked: None,
            sign_in_task: None,
            load_task: None,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The direct messages, most recently active first.
    pub fn rows(&self) -> &[Row] {
        &self.rows
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

    /// Reloads the list if it has not been asked for in a while. Until Peep hears about
    /// new messages as they happen, this is what keeps the list current.
    pub fn refresh_if_stale(&mut self, cx: &mut Context<Self>) {
        if self
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
            Ok(client) => {
                self.client = Some(client);
                self.load(cx);
            }
            Err(error) => self.set_status(Status::Failed(error.to_string().into()), cx),
        }
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
                self.rows = rows(&snapshot.directory);
                self.set_status(Status::Ready, cx);
                cx.background_spawn(async move {
                    if let Some(folder) = cache::folder()
                        && let Err(error) = cache::write(&folder, &snapshot)
                    {
                        eprintln!("Peep could not keep its snapshot: {error}");
                    }
                })
                .detach();
            }
            Err(chat::Error::SignedOut) => self.signed_out(cx),
            // A failed refresh leaves what is already showing in place.
            Err(_) if !self.rows.is_empty() => {}
            Err(error) => self.set_status(Status::Failed(error.to_string().into()), cx),
        }
    }

    /// Google no longer takes the session: forget what was kept for it.
    fn signed_out(&mut self, cx: &mut Context<Self>) {
        self.client = None;
        self.kept = Snapshot::default();
        self.rows.clear();
        self.set_status(Status::SignedOut, cx);
        cx.background_spawn(async {
            if let Some(folder) = cache::folder()
                && let Err(error) = cache::clear(&folder)
            {
                eprintln!("Peep could not forget its snapshot: {error}");
            }
        })
        .detach();
    }

    fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        self.status = status;
        cx.notify();
    }
}

/// The conversation list as drawn, most recently active first.
fn rows(directory: &Directory) -> Vec<Row> {
    directory
        .conversations
        .iter()
        .map(|conversation| Row {
            id: conversation.id.key().into(),
            title: SharedString::from(conversation.title.clone()),
            unread: match conversation.unread {
                0 => None,
                unread if unread > MOST_UNREAD_SHOWN => {
                    Some(format!("{MOST_UNREAD_SHOWN}+").into())
                }
                unread => Some(unread.to_string().into()),
            },
            muted: conversation.muted,
        })
        .collect()
}
