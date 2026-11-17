//! Sign-in through Google Chrome, on a profile of Peep's own that is kept apart from the
//! person's browsing. Chrome can use a security key, which a web view inside Peep cannot.
//! Chrome keeps the cookies; Peep reads them over Chrome's debugging pipe.

use std::{
    io::{self, BufRead, BufReader, PipeReader, PipeWriter, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use serde_json::{Value, json};

use super::{CHAT_HOST, Cookie, Session, now};

const CHROME: &str = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const START_URL: &str = "https://chat.google.com/";

/// Chrome reads commands on this descriptor and writes replies on the next one.
const COMMANDS_FD: RawFd = 3;
const REPLIES_FD: RawFd = 4;
/// A descriptor number above anything Chrome's pipe uses, to stage duplicates on.
const SCRATCH_FD: RawFd = 10;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Peep signs in through Google Chrome, which is not installed")]
    NotInstalled,
    #[error("Peep could not find your home folder")]
    NoHome,
    /// Chrome exited, which during sign-in means the person closed its window.
    #[error("Google Chrome closed")]
    Closed,
    #[error("Google Chrome could not be used: {0}")]
    Io(#[from] io::Error),
}

/// The session left by an earlier sign-in, if Google would still take it for one.
/// Blocks while Chrome starts, so call it off the main thread.
pub fn stored() -> Result<Option<Session>, Error> {
    let profile = profile()?;
    if !profile.exists() {
        return Ok(None);
    }
    let mut chrome = Chrome::launch(&profile, &["--headless=new", "about:blank"])?;
    let session = chrome.session();
    chrome.close();
    session
}

/// A Chrome window showing Google's sign-in page.
pub struct SignIn(Chrome);

impl SignIn {
    /// Opens the window. `width` and `height` are in points.
    pub fn open(width: u32, height: u32) -> Result<Self, Error> {
        let profile = profile()?;
        std::fs::create_dir_all(&profile)?;
        Chrome::launch(
            &profile,
            &[
                &format!("--app={START_URL}"),
                &format!("--window-size={width},{height}"),
            ],
        )
        .map(Self)
    }

    /// The session, once Google has finished and handed the person on to Google Chat.
    /// [`Error::Closed`] means the person closed the window first.
    pub fn poll(&mut self) -> Result<Option<Session>, Error> {
        if !self.0.shows(CHAT_HOST)? {
            return Ok(None);
        }
        self.0.session()
    }

    /// Closes the window, letting Chrome save the session first.
    pub fn finish(self) {
        self.0.close();
    }
}

fn profile() -> Result<PathBuf, Error> {
    Ok(crate::cache::folder().ok_or(Error::NoHome)?.join("Chrome"))
}

/// A running Chrome that Peep started and can question.
struct Chrome {
    process: Child,
    commands: PipeWriter,
    replies: BufReader<PipeReader>,
    next_id: u64,
}

impl Chrome {
    fn launch(profile: &Path, arguments: &[&str]) -> Result<Self, Error> {
        if !Path::new(CHROME).exists() {
            return Err(Error::NotInstalled);
        }
        let (commands_out, commands) = io::pipe()?;
        let (replies, replies_in) = io::pipe()?;
        let (commands_fd, replies_fd) = (commands_out.as_raw_fd(), replies_in.as_raw_fd());

        let mut command = Command::new(CHROME);
        command
            .arg("--remote-debugging-pipe")
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(["--no-first-run", "--no-default-browser-check"])
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: the closure runs in the forked child before Chrome starts and only
        // duplicates descriptors, which is safe there. Each end is first copied out of the
        // way, so neither copy lands on a descriptor the other still needs.
        unsafe {
            command.pre_exec(move || {
                let commands = duplicate(commands_fd, SCRATCH_FD)?;
                let replies = duplicate(replies_fd, SCRATCH_FD)?;
                place(commands, COMMANDS_FD)?;
                place(replies, REPLIES_FD)
            });
        }
        let process = command.spawn()?;

        Ok(Self {
            process,
            commands,
            replies: BufReader::new(replies),
            next_id: 0,
        })
    }

    /// Whether any page Chrome shows is on `host`.
    fn shows(&mut self, host: &str) -> Result<bool, Error> {
        let targets = self.call("Target.getTargets")?;
        Ok(targets["targetInfos"].as_array().is_some_and(|targets| {
            targets.iter().any(|target| {
                target["type"] == "page"
                    && target["url"]
                        .as_str()
                        .and_then(|url| url.strip_prefix("https://"))
                        .and_then(|rest| rest.split(['/', '?', '#']).next())
                        == Some(host)
            })
        }))
    }

    fn session(&mut self) -> Result<Option<Session>, Error> {
        let cookies = self.call("Storage.getCookies")?;
        let cookies = cookies["cookies"]
            .as_array()
            .map(|cookies| cookies.iter().filter_map(cookie).collect())
            .unwrap_or_default();
        let version = self.call("Browser.getVersion")?;
        let user_agent = user_agent(version["product"].as_str().unwrap_or_default());
        Ok(Session::new(cookies, user_agent, now()))
    }

    fn call(&mut self, method: &str) -> Result<Value, Error> {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = json!({ "id": id, "method": method })
            .to_string()
            .into_bytes();
        message.push(0);
        self.commands.write_all(&message).map_err(closed)?;

        // Chrome also sends events, which carry no id; skip to the reply.
        loop {
            let mut reply = Vec::new();
            if self.replies.read_until(0, &mut reply).map_err(closed)? == 0 {
                return Err(Error::Closed);
            }
            reply.pop();
            let Ok(mut reply) = serde_json::from_slice::<Value>(&reply) else {
                continue;
            };
            if reply["id"] == id {
                return Ok(reply["result"].take());
            }
        }
    }

    /// Asks Chrome to quit, so it saves its cookies, and waits for it.
    fn close(mut self) {
        // If Chrome has already gone there is nothing to ask or wait for.
        if self.call("Browser.close").is_ok() {
            self.process.wait().ok();
        }
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        // Stops a Chrome that was not closed in order; harmless if it already exited.
        self.process.kill().ok();
        self.process.wait().ok();
    }
}

fn closed(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof => Error::Closed,
        _ => Error::Io(error),
    }
}

fn duplicate(fd: RawFd, at_least: RawFd) -> io::Result<RawFd> {
    // SAFETY: `fcntl` with `F_DUPFD` only reads its integer arguments.
    match unsafe { libc::fcntl(fd, libc::F_DUPFD, at_least) } {
        -1 => Err(io::Error::last_os_error()),
        copy => Ok(copy),
    }
}

fn place(fd: RawFd, at: RawFd) -> io::Result<()> {
    // SAFETY: `dup2` only reads its integer arguments.
    match unsafe { libc::dup2(fd, at) } {
        -1 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

/// A cookie as Chrome reports it, if it belongs to Google or Google Chat.
fn cookie(cookie: &Value) -> Option<Cookie> {
    let domain = cookie["domain"].as_str()?;
    let bare = domain.trim_start_matches('.').to_ascii_lowercase();
    if bare != "google.com" && bare != CHAT_HOST {
        return None;
    }
    Some(Cookie {
        name: cookie["name"].as_str()?.to_owned(),
        value: cookie["value"].as_str()?.to_owned(),
        domain: domain.to_owned(),
        path: cookie["path"].as_str()?.to_owned(),
        secure: cookie["secure"].as_bool().unwrap_or(false),
        expires: match cookie["session"].as_bool() {
            Some(true) => None,
            _ => cookie["expires"].as_f64().filter(|expires| *expires > 0.),
        },
    })
}

/// The user agent Chrome sends, from its product string such as `Chrome/157.0.8103.79`.
/// Chrome reports only its major version to websites.
fn user_agent(product: &str) -> String {
    let major = product
        .rsplit('/')
        .next()
        .and_then(|version| version.split('.').next())
        .filter(|major| !major.is_empty() && major.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("150");
    format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromes_cookies_are_kept_only_for_google_and_chat() {
        let chrome = |domain: &str, session: bool, expires: f64| {
            json!({
                "name": "SID", "value": "secret", "domain": domain, "path": "/",
                "secure": true, "session": session, "expires": expires,
            })
        };

        let account = cookie(&chrome(".google.com", false, 1_800_000_000.)).unwrap();
        assert_eq!(account.domain, ".google.com");
        assert_eq!(account.expires, Some(1_800_000_000.));
        assert!(account.secure);

        let chat = cookie(&chrome("chat.google.com", true, -1.)).unwrap();
        assert_eq!(chat.expires, None, "a session cookie has no expiry");

        assert!(cookie(&chrome("accounts.google.com", true, -1.)).is_none());
        assert!(cookie(&chrome(".youtube.com", true, -1.)).is_none());
        assert!(cookie(&json!({ "domain": ".google.com" })).is_none());
    }

    #[test]
    fn the_user_agent_carries_chromes_major_version_only() {
        assert!(user_agent("Chrome/157.0.8103.79").contains(" Chrome/157.0.0.0 "));
        assert!(user_agent("HeadlessChrome/157.0.8103.79").contains(" Chrome/157.0.0.0 "));
        assert!(user_agent("").contains(" Chrome/150.0.0.0 "));
    }
}
