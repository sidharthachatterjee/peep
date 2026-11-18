//! Checks against Google Chat itself, with the session Peep is signed in with. They only
//! read, and print counts, never names or messages.
//!
//! They are skipped unless asked for: `cargo test --test live -- --ignored`.
#![cfg(target_os = "macos")]

use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use futures::{channel::mpsc::TryRecvError, executor::block_on};
use peep::chat::{Client, Event, Snapshot, UserId};

/// One client for every test: reading the session starts Chrome on Peep's profile, which
/// only one Chrome can use at a time.
fn client() -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            let session = peep::session::stored()
                .expect("the stored session can be read")
                .expect("Peep is signed in");
            let kept = peep::cache::folder()
                .map(|folder| peep::cache::read(&folder))
                .unwrap_or_default();
            Client::new(session, kept).expect("a client starts")
        })
        .clone()
}

fn refreshed(client: &Client) -> Snapshot {
    block_on(client.refresh()).expect("Google Chat answers")
}

#[test]
#[ignore = "asks Google Chat"]
fn the_list_loads_with_the_people_in_it() {
    let directory = refreshed(&client()).directory;

    println!(
        "{} conversations, {} people, {} unread by Google's count",
        directory.conversations.len(),
        directory.users.len(),
        directory.unread
    );
    assert!(directory.me.is_some());
    assert!(!directory.conversations.is_empty());
    let named = directory
        .conversations
        .iter()
        .filter(|c| !c.title.is_empty());
    assert_eq!(named.count(), directory.conversations.len());
}

#[test]
#[ignore = "asks Google Chat"]
fn the_newest_conversation_has_messages_and_older_ones_before_them() {
    let client = client();
    let newest = refreshed(&client).directory.conversations.remove(0);

    let page = block_on(client.messages(&newest.id, None)).expect("messages load");
    println!(
        "{} messages, older ones: {}",
        page.messages.len(),
        page.older.is_some()
    );
    assert!(!page.messages.is_empty());
    assert!(page.messages.is_sorted_by_key(|message| message.time));

    if let Some(before) = page.older {
        let older = block_on(client.messages(&newest.id, Some(before))).expect("older load");
        println!("{} older messages", older.messages.len());
        assert!(
            older
                .messages
                .iter()
                .all(|message| message.time < page.messages[0].time)
        );
    }
}

#[test]
#[ignore = "asks Google Chat"]
fn google_says_how_people_are_doing() {
    let client = client();
    let directory = refreshed(&client).directory;
    let people: Vec<UserId> = directory
        .conversations
        .iter()
        .filter_map(|conversation| match conversation.members.as_slice() {
            [person] => Some(person.clone()),
            _ => None,
        })
        .take(20)
        .collect();

    let statuses = block_on(client.statuses(&people)).expect("statuses load");
    println!(
        "asked about {}, heard about {}",
        people.len(),
        statuses.len()
    );
    assert!(!statuses.is_empty());
}

#[test]
#[ignore = "asks Google Chat"]
fn the_live_channel_connects() {
    let client = client();
    let mut events = client.events();
    let started = Instant::now();

    let connected = loop {
        match events.try_recv() {
            Ok(Event::Connected { .. }) => break true,
            Err(TryRecvError::Closed) => break false,
            _ if started.elapsed() > Duration::from_secs(30) => break false,
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    println!("connected: {connected} after {:?}", started.elapsed());
    assert!(connected);
}
