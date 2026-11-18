//! Peep from its window down to a Google Chat that the tests fill in and listen to.

use std::{rc::Rc, time::Duration};

use gpui::{Entity, SharedString, TestAppContext, VisualTestContext};

use super::*;
use crate::chat::{
    Availability, ConversationId, Event, Presence, Status as PersonStatus, UserId,
    fake::{Asked, Fake},
};

/// Longer than anything Peep waits out before acting.
const A_MOMENT: Duration = Duration::from_secs(1);

/// Two people: Ada wrote a minute ago and has not been read, Grace was read ten minutes ago.
fn two_people() -> Fake {
    Fake::new()
        .conversation(
            "Ada",
            &[("me", "Is the engine ready?"), ("ada", "Nearly.")],
            1,
            1,
        )
        .conversation(
            "Grace",
            &[
                ("grace", "One"),
                ("me", "Two"),
                ("grace", "Three"),
                ("grace", "Four"),
                ("me", "Five"),
            ],
            10,
            5,
        )
}

/// Starts Peep signed in to `service`, with its window open and in front.
fn start<'a>(
    service: &Fake,
    cx: &'a mut TestAppContext,
) -> (Entity<Account>, &'a mut VisualTestContext) {
    let service = Rc::new(service.clone());
    let account = cx.update(|cx| {
        init(cx);
        let account = cx.new(|cx| Account::over(service, cx));
        launch(account.clone(), true, cx);
        account
    });
    let window = cx.update(|cx| cx.windows()[0]);
    let cx = VisualTestContext::from_window(window, cx).into_mut();
    cx.update(|window, _| window.activate_window());
    settle(cx);
    (account, cx)
}

/// Lets everything Peep was waiting to do happen.
fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.executor().advance_clock(A_MOMENT);
    cx.run_until_parked();
}

/// The list as it shows: each row's title and unread count.
fn list(account: &Entity<Account>, cx: &mut VisualTestContext) -> Vec<(String, Option<String>)> {
    account.read_with(cx, |account, _| {
        account
            .rows()
            .iter()
            .map(|row| {
                let unread = row.unread.as_ref().map(ToString::to_string);
                (row.title.to_string(), unread)
            })
            .collect()
    })
}

/// The text of the messages showing in the open conversation.
fn messages(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let main = main(cx);
        let selected = main.read(cx).conversations.read(cx).selected_id();
        let account = cx.global::<SignedIn>().0.read(cx);
        selected
            .and_then(|selected| account.timeline(&selected))
            .map(|timeline| {
                timeline
                    .entries()
                    .iter()
                    .map(|entry| entry.text.to_string())
            })
            .into_iter()
            .flatten()
            .collect()
    })
}

fn main(cx: &App) -> Entity<Main> {
    let open = cx.global::<OpenWindow>().0.as_ref();
    open.and_then(WeakEntity::upgrade)
        .expect("the window is open")
}

fn row(title: &str, unread: Option<&str>) -> (String, Option<String>) {
    (title.to_owned(), unread.map(str::to_owned))
}

#[gpui::test]
fn it_lists_the_conversations_and_reading_one_clears_its_count(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);

    assert_eq!(
        list(&account, cx),
        [row("Ada", Some("1")), row("Grace", None)]
    );
    assert_eq!(cx.update(|_, cx| menu_bar_content(cx)).unread, 1);
    assert_eq!(
        cx.update(|_, cx| menu_bar_content(cx)).conversations,
        [("dm/ada-dm".to_owned(), "Ada".to_owned())]
    );
    let asked = service.asked();
    assert_eq!(asked[0], Asked::Refresh);
    assert!(
        asked.contains(&Asked::Messages("dm/grace-dm".into(), None)),
        "recent conversations are loaded ahead of being opened: {asked:?}"
    );

    // Tab reaches the list and Down selects its first row.
    cx.simulate_keystrokes("tab down");
    settle(cx);

    assert_eq!(messages(cx), ["Is the engine ready?", "Nearly."]);
    assert_eq!(list(&account, cx), [row("Ada", None), row("Grace", None)]);
    assert!(
        service
            .asked()
            .iter()
            .any(|asked| matches!(asked, Asked::MarkRead(id, _) if id == "dm/ada-dm")),
        "Google is told it was read"
    );
    assert_eq!(cx.update(|_, cx| menu_bar_content(cx)).unread, 0);
}

#[gpui::test]
fn a_message_that_arrives_moves_its_conversation_up_and_shows(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);
    cx.simulate_keystrokes("tab down");
    settle(cx);

    service.post("dm/grace-dm", "grace", "Six");
    settle(cx);

    assert_eq!(
        list(&account, cx),
        [row("Grace", Some("1")), row("Ada", None)]
    );
    assert_eq!(messages(cx), ["Is the engine ready?", "Nearly."]);

    // The open conversation shows what arrives in it, and it counts as read.
    service.post("dm/ada-dm", "ada", "Ready now.");
    settle(cx);

    assert_eq!(
        messages(cx),
        ["Is the engine ready?", "Nearly.", "Ready now."]
    );
    assert_eq!(
        list(&account, cx),
        [row("Ada", None), row("Grace", Some("1"))]
    );
}

#[gpui::test]
fn older_messages_load_a_page_at_a_time(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);
    cx.simulate_keystrokes("tab down down");
    settle(cx);
    assert_eq!(messages(cx), ["Three", "Four", "Five"]);

    let grace = SharedString::from("dm/grace-dm");
    account.update(cx, |account, cx| account.load_older(&grace, cx));
    settle(cx);

    assert_eq!(messages(cx), ["One", "Two", "Three", "Four", "Five"]);
}

#[gpui::test]
fn the_people_on_screen_are_watched_until_the_window_closes(cx: &mut TestAppContext) {
    let service = two_people().status(
        "ada",
        PersonStatus {
            availability: Availability::Active,
            quiet: false,
            note: "".into(),
        },
    );
    let (account, cx) = start(&service, cx);
    let presence =
        |cx: &mut VisualTestContext| account.read_with(cx, |account, _| account.rows()[0].presence);

    assert!(
        service
            .asked()
            .contains(&Asked::Watch(vec!["ada".into(), "grace".into()]))
    );
    assert_eq!(presence(cx), Some(Presence::Active), "asked at once");

    service.say(Event::Availability {
        person: UserId("ada".into()),
        availability: Availability::Idle,
    });
    settle(cx);
    assert_eq!(presence(cx), Some(Presence::Idle), "heard as it happens");

    cx.simulate_keystrokes("cmd-w");
    settle(cx);
    assert_eq!(service.asked().last(), Some(&Asked::Watch(Vec::new())));
}

#[gpui::test]
fn a_status_chosen_from_the_menu_bar_reaches_google(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);
    service.asked();

    cx.update(|_, cx| carry_out(Command::Show(Choice::Away), cx));
    settle(cx);

    assert_eq!(service.asked(), [Asked::Show(Showing::Away)]);
    assert_eq!(
        cx.update(|_, cx| menu_bar_content(cx)).showing,
        Showing::Away
    );

    // Another device changes it back.
    service.say(Event::Status {
        person: UserId("me".into()),
        showing: Showing::Automatic,
        quiet: false,
        note: "".into(),
    });
    settle(cx);
    assert_eq!(
        account.read_with(cx, |account, _| account.showing()),
        Showing::Automatic
    );
}

#[gpui::test]
fn an_unread_conversation_opens_from_the_menu_bar(cx: &mut TestAppContext) {
    let service = two_people();
    let (_, cx) = start(&service, cx);

    cx.update(|_, cx| carry_out(Command::OpenConversation("dm/ada-dm".into()), cx));
    settle(cx);

    assert_eq!(messages(cx), ["Is the engine ready?", "Nearly."]);
}

#[gpui::test]
fn googles_own_count_shows_in_the_menu_bar_as_it_changes(cx: &mut TestAppContext) {
    let service = two_people();
    let (_, cx) = start(&service, cx);

    service.say(Event::Unread(7));
    settle(cx);

    assert_eq!(cx.update(|_, cx| menu_bar_content(cx)).unread, 7);
}

#[gpui::test]
fn being_signed_out_by_google_asks_for_a_sign_in_again(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);

    service.sign_out();
    service.say(Event::ListChanged);
    settle(cx);

    assert_eq!(
        account.read_with(cx, |account, _| account.status().clone()),
        Status::SignedOut
    );
    assert_eq!(list(&account, cx), []);
    assert_eq!(cx.update(|_, cx| menu_bar_content(cx)).unread, 0);
}

#[gpui::test]
fn a_read_on_another_device_clears_the_count_here(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);

    service.say(Event::Read {
        conversation: ConversationId::Dm("ada-dm".into()),
        unread: false,
    });
    settle(cx);

    assert_eq!(list(&account, cx), [row("Ada", None), row("Grace", None)]);
}
