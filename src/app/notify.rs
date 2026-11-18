//! Whether a message that arrives is brought to the person's notice, and how.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use futures::{StreamExt as _, channel::mpsc};
use gpui::{App, Entity, Global, SharedString, WeakEntity};

use super::{Command, NotificationsPaused, OpenWindow, SignedIn, carry_out};
use crate::{
    account::{Account, Incoming},
    chat::Showing,
    notifications::{self, Notice, Notifications},
};

/// A banner within this long of the one before it in its conversation comes without the
/// sound, so a run of messages chirps once.
const QUIET_AFTER_ONE: Duration = Duration::from_secs(5);
/// The most of a message a banner is given. macOS shows less.
const LONGEST_PREVIEW: usize = 300;

/// How a message that arrived is brought to the person's notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Notify {
    Nothing,
    /// Only the sound: Peep is in front, showing another conversation.
    Sound,
    Banner(Notice),
}

/// What the person is attending to when a message arrives.
#[derive(Clone, Debug, Default)]
pub(super) struct Attention {
    /// Notifications are paused.
    pub paused: bool,
    /// The person has asked not to be disturbed.
    pub quiet: bool,
    /// Peep's window is in front.
    pub in_front: bool,
    /// The conversation Peep's window shows.
    pub open: Option<SharedString>,
    /// How long ago the conversation last had a banner, if one of its banners is still up.
    pub since_last: Option<Duration>,
}

pub(super) fn notify(incoming: &Incoming, attention: &Attention) -> Notify {
    if attention.paused || attention.quiet {
        return Notify::Nothing;
    }
    if attention.in_front {
        return match attention.open.as_ref() == Some(&incoming.conversation) {
            // They are looking at it.
            true => Notify::Nothing,
            false => Notify::Sound,
        };
    }
    let mut body: String = incoming.text.chars().take(LONGEST_PREVIEW).collect();
    if body.len() < incoming.text.len() {
        body.push('…');
    }
    // A conversation with one person is named after them; a group says who wrote.
    let subtitle = match incoming.sender == incoming.title {
        true => String::new(),
        false => incoming.sender.to_string(),
    };
    Notify::Banner(Notice {
        id: incoming.message.to_string(),
        conversation: incoming.conversation.to_string(),
        title: incoming.title.to_string(),
        subtitle,
        body,
        sound: attention
            .since_last
            .is_none_or(|since| since >= QUIET_AFTER_ONE),
    })
}

/// Peep's banners, and which of them are up.
#[derive(Default)]
pub(super) struct Notifier {
    /// `None` when macOS will show none, as when Peep is not run from its app bundle.
    system: Option<Notifications>,
    /// The banners up, by conversation, and when the last of them went up.
    up: HashMap<SharedString, (Vec<String>, Instant)>,
    /// What each message that arrived came to.
    #[cfg(test)]
    pub notified: Vec<Notify>,
}

impl Global for Notifier {}

/// Starts bringing the account's incoming messages to the person's notice.
pub(super) fn start(account: &Entity<Account>, cx: &mut App) {
    let (opened, mut clicked) = mpsc::unbounded();
    cx.set_global(Notifier {
        system: Notifications::new(opened),
        ..Default::default()
    });
    cx.subscribe(account, |_, incoming: &Incoming, cx| arrived(incoming, cx))
        .detach();
    cx.observe(account, |account, cx| take_down_read(&account, cx))
        .detach();
    // A banner that is clicked opens its conversation.
    cx.spawn(async move |cx| {
        while let Some(conversation) = clicked.next().await {
            cx.update(|cx| carry_out(Command::OpenConversation(conversation), cx));
        }
    })
    .detach();
}

fn arrived(incoming: &Incoming, cx: &mut App) {
    let main = cx.global::<OpenWindow>().0.as_ref();
    let main = main.and_then(WeakEntity::upgrade);
    let up = cx.global::<Notifier>().up.get(&incoming.conversation);
    let main = main.as_ref().map(|main| main.read(cx));
    let attention = Attention {
        paused: cx.global::<NotificationsPaused>().0,
        quiet: matches!(
            cx.global::<SignedIn>().0.read(cx).showing(),
            Showing::DoNotDisturb { .. }
        ),
        in_front: main.is_some_and(|main| main.in_front),
        open: main.and_then(|main| main.conversations.read(cx).selected_id()),
        since_last: up.map(|(_, last)| last.elapsed()),
    };
    let notify = notify(incoming, &attention);
    let notifier = cx.global_mut::<Notifier>();
    match &notify {
        Notify::Nothing => {}
        Notify::Sound => notifications::chirp(),
        Notify::Banner(notice) => {
            let up = notifier.up.entry(incoming.conversation.clone());
            let (ids, last) = up.or_insert_with(|| (Vec::new(), Instant::now()));
            ids.push(notice.id.clone());
            *last = Instant::now();
            if let Some(system) = &notifier.system {
                system.show(notice.clone());
            }
        }
    }
    #[cfg(test)]
    notifier.notified.push(notify);
}

/// Takes down the banners of conversations that have since been read, here or elsewhere.
fn take_down_read(account: &Entity<Account>, cx: &mut App) {
    if cx.global::<Notifier>().up.is_empty() {
        return;
    }
    let rows = account.read(cx).rows();
    let read: Vec<SharedString> = cx
        .global::<Notifier>()
        .up
        .keys()
        .filter(|conversation| {
            let row = rows.iter().find(|row| row.id == **conversation);
            row.is_none_or(|row| row.unread.is_none())
        })
        .cloned()
        .collect();
    let notifier = cx.global_mut::<Notifier>();
    for conversation in read {
        if let (Some((ids, _)), Some(system)) =
            (notifier.up.remove(&conversation), &notifier.system)
        {
            system.clear(ids);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(title: &str, sender: &str, text: &str) -> Incoming {
        Incoming {
            conversation: "dm/a".into(),
            title: title.to_owned().into(),
            sender: sender.to_owned().into(),
            message: "m1".into(),
            text: text.to_owned().into(),
        }
    }

    fn banner(notify: Notify) -> Notice {
        match notify {
            Notify::Banner(notice) => notice,
            other => panic!("expected a banner, not {other:?}"),
        }
    }

    #[test]
    fn a_message_comes_with_a_banner_unless_the_person_is_looking_or_asked_for_quiet() {
        let message = incoming("Ada", "Ada", "Nearly.");
        let attending = |change: fn(&mut Attention)| {
            let mut attention = Attention::default();
            change(&mut attention);
            notify(&message, &attention)
        };

        let notice = banner(attending(|_| {}));
        assert_eq!(
            notice,
            Notice {
                id: "m1".into(),
                conversation: "dm/a".into(),
                title: "Ada".into(),
                subtitle: String::new(),
                body: "Nearly.".into(),
                sound: true,
            }
        );

        assert_eq!(attending(|a| a.paused = true), Notify::Nothing);
        assert_eq!(attending(|a| a.quiet = true), Notify::Nothing);
        assert_eq!(
            attending(|a| {
                a.in_front = true;
                a.open = Some("dm/a".into());
            }),
            Notify::Nothing,
            "they are looking at it"
        );
        assert_eq!(
            attending(|a| {
                a.in_front = true;
                a.open = Some("dm/other".into());
            }),
            Notify::Sound,
            "in front, but showing another conversation"
        );
        assert!(
            matches!(
                attending(|a| a.open = Some("dm/a".into())),
                Notify::Banner(_)
            ),
            "open, but behind other apps"
        );
    }

    #[test]
    fn a_run_of_messages_chirps_once_and_a_group_says_who_wrote() {
        let soon = Attention {
            since_last: Some(Duration::from_secs(2)),
            ..Default::default()
        };
        let later = Attention {
            since_last: Some(Duration::from_secs(9)),
            ..Default::default()
        };
        let message = incoming("Engine room", "Ada", &"long ".repeat(100));

        let notice = banner(notify(&message, &soon));
        assert!(!notice.sound);
        assert_eq!(notice.subtitle, "Ada");
        assert_eq!(notice.body.chars().count(), LONGEST_PREVIEW + 1);
        assert!(notice.body.ends_with('…'));
        assert!(banner(notify(&message, &later)).sound);
    }
}
