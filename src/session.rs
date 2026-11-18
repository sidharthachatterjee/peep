//! The Google session Peep acts with: the cookies Google set when the person signed in.

mod chrome;

use std::fmt;

pub use chrome::{Error, SignIn, stored};

const CHAT_HOST: &str = "chat.google.com";

#[derive(Clone, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// As Google set it. A leading dot includes subdomains.
    pub domain: String,
    pub path: String,
    pub secure: bool,
    /// Seconds since the Unix epoch. `None` lasts until sign-out.
    pub expires: Option<f64>,
}

impl Cookie {
    /// Whether a request to `https://{host}{path}` at `now` carries this cookie.
    fn applies(&self, host: &str, path: &str, now: f64) -> bool {
        let well_formed = !self.name.is_empty()
            && !self.value.is_empty()
            && !self
                .name
                .contains(|c: char| c.is_whitespace() || "=;".contains(c))
            && !self.value.contains([';', '\r', '\n']);
        let live = self.expires.is_none_or(|expires| expires > now);

        let domain = self.domain.to_ascii_lowercase();
        let (bare, subdomains) = match domain.strip_prefix('.') {
            Some(bare) => (bare, true),
            None => (domain.as_str(), false),
        };
        let ours = bare == "google.com" || bare == CHAT_HOST;
        let host_matches = host == bare
            || (subdomains
                && host
                    .strip_suffix(bare)
                    .is_some_and(|rest| rest.ends_with('.')));

        let cookie_path = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        let path_matches = path == cookie_path
            || path
                .strip_prefix(cookie_path)
                .is_some_and(|rest| cookie_path.ends_with('/') || rest.starts_with('/'));

        well_formed && live && ours && host_matches && path_matches
    }
}

/// Prints the cookie's name only: its value is a credential.
impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cookie({} on {})", self.name, self.domain)
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    cookies: Vec<Cookie>,
    user_agent: String,
}

impl Session {
    /// A session from the cookies a sign-in left behind, or `None` when Google has not
    /// finished signing the person in to Google Chat: that takes a Google account cookie
    /// plus Google Chat's own `OSID` and `COMPASS`.
    pub fn new(cookies: Vec<Cookie>, user_agent: String, now: f64) -> Option<Self> {
        let session = Self {
            cookies,
            user_agent,
        };
        let sent = |name: &str| {
            session
                .cookies
                .iter()
                .any(|cookie| cookie.name == name && cookie.applies(CHAT_HOST, "/", now))
        };
        let compass = session.cookies.iter().any(|cookie| {
            cookie.name == "COMPASS"
                && cookie.domain.to_ascii_lowercase().ends_with(CHAT_HOST)
                && cookie.expires.is_none_or(|expires| expires > now)
        });
        ((sent("SID") || sent("__Secure-1PSID")) && sent("OSID") && compass).then_some(session)
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// The secret Google's services want a hash of beside the cookies, if the session has
    /// one.
    pub fn api_secret(&self, now: f64) -> Option<&str> {
        ["SAPISID", "__Secure-3PAPISID"].iter().find_map(|name| {
            self.cookies
                .iter()
                .find(|cookie| cookie.name == *name && cookie.applies(CHAT_HOST, "/", now))
                .map(|cookie| cookie.value.as_str())
        })
    }

    /// The `Cookie` header for a request to `https://chat.google.com{path}`.
    pub fn cookie_header(&self, path: &str, now: f64) -> String {
        self.cookie_header_for(CHAT_HOST, path, now)
    }

    /// The `Cookie` header for a request to `https://{host}{path}`, for another of Google's
    /// services the session reaches.
    pub fn cookie_header_for(&self, host: &str, path: &str, now: f64) -> String {
        let mut cookies: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|cookie| cookie.applies(host, path, now))
            .collect();
        cookies.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then_with(|| a.name.cmp(&b.name))
        });
        cookies
            .iter()
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Seconds since the Unix epoch.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |elapsed| elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_700_000_000.;

    fn cookie(name: &str, domain: &str, path: &str) -> Cookie {
        Cookie {
            name: name.into(),
            value: format!("{name}-value"),
            domain: domain.into(),
            path: path.into(),
            secure: true,
            expires: None,
        }
    }

    fn signed_in() -> Vec<Cookie> {
        vec![
            cookie("SID", ".google.com", "/"),
            cookie("OSID", "chat.google.com", "/"),
            cookie("COMPASS", "chat.google.com", "/"),
        ]
    }

    #[test]
    fn a_session_needs_an_account_cookie_and_chats_own_cookies() {
        assert!(Session::new(signed_in(), "agent".into(), NOW).is_some());

        for missing in ["SID", "OSID", "COMPASS"] {
            let cookies = signed_in()
                .into_iter()
                .filter(|cookie| cookie.name != missing)
                .collect();
            assert!(
                Session::new(cookies, "agent".into(), NOW).is_none(),
                "without {missing}"
            );
        }

        let mut mail_only = signed_in();
        mail_only[1].domain = "mail.google.com".into();
        assert!(
            Session::new(mail_only, "agent".into(), NOW).is_none(),
            "another service's OSID does not count"
        );

        let mut expired = signed_in();
        expired[1].expires = Some(NOW - 1.);
        assert!(Session::new(expired, "agent".into(), NOW).is_none());
    }

    #[test]
    fn the_cookie_header_carries_only_what_applies() {
        let mut cookies = signed_in();
        cookies.push(cookie("HOSTONLY", "google.com", "/"));
        cookies.push(cookie("OTHER", ".youtube.com", "/"));
        cookies.push(cookie("DEEP", "chat.google.com", "/api"));
        cookies.push(cookie("ELSEWHERE", "chat.google.com", "/u/0/webchannel"));
        let mut stale = cookie("STALE", ".google.com", "/");
        stale.expires = Some(NOW - 1.);
        cookies.push(stale);
        let session = Session::new(cookies, "agent".into(), NOW).unwrap();

        assert_eq!(
            session.cookie_header("/api/paginated_world", NOW),
            "DEEP=DEEP-value; COMPASS=COMPASS-value; OSID=OSID-value; SID=SID-value",
            "longest path first, then by name"
        );
        assert_eq!(
            session.cookie_header("/apiary", NOW),
            "COMPASS=COMPASS-value; OSID=OSID-value; SID=SID-value",
            "a path matches only at a segment boundary"
        );
    }

    #[test]
    fn the_api_secret_is_the_first_of_googles_two_cookies_for_it() {
        let mut cookies = signed_in();
        assert_eq!(
            Session::new(cookies.clone(), "agent".into(), NOW)
                .unwrap()
                .api_secret(NOW),
            None
        );

        cookies.push(cookie("__Secure-3PAPISID", ".google.com", "/"));
        cookies.push(cookie("SAPISID", ".google.com", "/"));
        let session = Session::new(cookies, "agent".into(), NOW).unwrap();
        assert_eq!(session.api_secret(NOW), Some("SAPISID-value"));
    }

    #[test]
    fn debug_output_never_shows_cookie_values() {
        let session = Session::new(signed_in(), "agent".into(), NOW).unwrap();
        assert!(!format!("{session:?}").contains("-value"));
    }
}
