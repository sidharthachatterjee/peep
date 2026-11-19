//! Peep from its window down to a Google Chat that the tests fill in and listen to.

use std::{rc::Rc, time::Duration};

use gpui::{Entity, SharedString, TestAppContext, VisualTestContext};

use super::*;
use crate::chat::{
    Availability, ConversationId, Draft, Event, Mention, Presence, Reaction,
    Status as PersonStatus, UserId,
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

    assert!(service.asked().contains(&Asked::Watch(
        vec!["ada".into(), "grace".into()],
        vec!["dm/ada-dm".into(), "dm/grace-dm".into()]
    )));
    assert_eq!(presence(cx), Some(Presence::Active), "asked at once");

    service.say(Event::Availability {
        person: UserId("ada".into()),
        availability: Availability::Idle,
    });
    settle(cx);
    assert_eq!(presence(cx), Some(Presence::Idle), "heard as it happens");

    cx.simulate_keystrokes("cmd-w");
    settle(cx);
    assert_eq!(
        service.asked().last(),
        Some(&Asked::Watch(Vec::new(), Vec::new()))
    );
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
fn the_count_is_of_unread_conversations_that_are_not_muted(cx: &mut TestAppContext) {
    let service = two_people()
        .conversation("Linus", &[("linus", "Ping"), ("linus", "Ping")], 5, 0)
        .mute("dm/linus-dm");
    let (account, cx) = start(&service, cx);
    let in_menu_bar = |cx: &mut VisualTestContext| {
        let content = cx.update(|_, cx| menu_bar_content(cx));
        let names: Vec<String> = content
            .conversations
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        (content.unread, names)
    };

    assert_eq!(
        list(&account, cx),
        [
            row("Ada", Some("1")),
            row("Linus", Some("2")),
            row("Grace", None)
        ]
    );
    assert_eq!(in_menu_bar(cx), (1, vec!["Ada".to_owned()]));

    service.post("dm/grace-dm", "grace", "Six");
    settle(cx);
    assert_eq!(
        in_menu_bar(cx),
        (2, vec!["Grace".to_owned(), "Ada".to_owned()])
    );

    // Ada is read on another device.
    service.say(Event::Read {
        conversation: ConversationId::Dm("ada-dm".into()),
        unread: false,
    });
    settle(cx);
    assert_eq!(in_menu_bar(cx), (1, vec!["Grace".to_owned()]));
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

/// What is in the box a message is written in, and why the last one was not sent.
fn composer(cx: &mut VisualTestContext) -> (String, Option<String>) {
    cx.update(|_, cx| {
        let messages = main(cx).read(cx).messages.clone();
        let composer = messages.read(cx).composer().clone();
        composer.read(cx).state(cx)
    })
}

fn typing(conversation: &str, person: &str, typing: bool) -> Event {
    Event::Typing {
        conversation: ConversationId::Dm(conversation.into()),
        person: UserId(person.into()),
        typing,
    }
}

#[gpui::test]
fn who_is_typing_shows_until_they_stop_send_or_go_quiet(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);
    let shown = |cx: &mut VisualTestContext| {
        account.read_with(cx, |account, _| {
            let rows = account.rows();
            let row = rows.iter().find(|row| row.title == "Ada").unwrap();
            (row.typing, account.typing_in(&"dm/ada-dm".into()))
        })
    };
    assert!(
        service.asked().iter().any(|asked| matches!(
            asked,
            Asked::Watch(_, typing) if typing.contains(&"dm/ada-dm".to_owned())
        )),
        "Google is asked who types in the conversations on screen"
    );

    service.say(typing("ada-dm", "ada", true));
    cx.run_until_parked();
    assert_eq!(shown(cx), (true, vec!["Ada".into()]));

    service.say(typing("ada-dm", "ada", false));
    cx.run_until_parked();
    assert_eq!(shown(cx), (false, Vec::new()), "she stopped");

    service.say(typing("ada-dm", "ada", true));
    service.say(typing("ada-dm", "me", true));
    cx.run_until_parked();
    assert_eq!(shown(cx).1.len(), 1, "my own typing elsewhere is not shown");
    service.post("dm/ada-dm", "ada", "There.");
    cx.run_until_parked();
    assert_eq!(shown(cx), (false, Vec::new()), "what she typed arrived");

    service.say(typing("ada-dm", "ada", true));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(6));
    service.say(typing("ada-dm", "ada", true));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(6));
    cx.run_until_parked();
    assert!(shown(cx).0, "Google said again that she is typing");
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
    assert_eq!(shown(cx), (false, Vec::new()), "nothing more was heard");
}

#[gpui::test]
fn a_message_written_and_sent_shows_at_once_and_reaches_google(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);
    // Down selects Grace below Ada; Enter opens her conversation to write in.
    cx.simulate_keystrokes("tab down down enter");
    settle(cx);
    service.asked();

    cx.simulate_input("Six, then.");
    assert_eq!(
        service.asked(),
        [Asked::Typing("dm/grace-dm".into())],
        "Google is told once, not for every letter"
    );
    assert_eq!(composer(cx).0, "Six, then.");

    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(
        service.asked(),
        [Asked::Send(
            "dm/grace-dm".into(),
            Draft {
                text: "Six, then.".into(),
                mentions: Vec::new()
            }
        )]
    );
    assert_eq!(messages(cx), ["Three", "Four", "Five", "Six, then."]);
    assert_eq!(composer(cx), (String::new(), None));
    assert_eq!(
        list(&account, cx),
        [row("Grace", None), row("Ada", None)],
        "it moves to the top, and my own message is not unread"
    );

    cx.simulate_keystrokes("enter");
    cx.simulate_input("  ");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert!(
        !service
            .asked()
            .iter()
            .any(|asked| matches!(asked, Asked::Send(..))),
        "nothing is sent of nothing"
    );
}

#[gpui::test]
fn someone_named_with_an_at_is_mentioned(cx: &mut TestAppContext) {
    let service = two_people();
    let (_, cx) = start(&service, cx);
    cx.simulate_keystrokes("tab down enter");
    settle(cx);

    cx.simulate_input("thanks @a");
    // Enter takes the person offered rather than sending.
    cx.simulate_keystrokes("enter");
    assert_eq!(composer(cx).0, "thanks @Ada ");
    cx.simulate_input("!");
    service.asked();
    cx.simulate_keystrokes("enter");
    settle(cx);

    assert_eq!(
        service.asked(),
        [Asked::Send(
            "dm/ada-dm".into(),
            Draft {
                text: "thanks @Ada !".into(),
                mentions: vec![Mention {
                    person: UserId("ada".into()),
                    range: 7..11,
                    member: true,
                }]
            }
        )]
    );

    // A name with a letter taken out is only text.
    cx.simulate_input("@a");
    cx.simulate_keystrokes("tab backspace backspace escape enter");
    settle(cx);
    assert_eq!(
        service.asked().last(),
        Some(&Asked::Send(
            "dm/ada-dm".into(),
            Draft {
                text: "@Ad".into(),
                mentions: Vec::new()
            }
        ))
    );
}

#[gpui::test]
fn a_message_google_refuses_goes_back_in_the_box(cx: &mut TestAppContext) {
    let service = two_people();
    let (_, cx) = start(&service, cx);
    cx.simulate_keystrokes("tab down enter");
    settle(cx);
    service.refuse_messages();

    cx.simulate_input("Hello?");
    cx.simulate_keystrokes("enter");
    settle(cx);

    assert_eq!(messages(cx), ["Is the engine ready?", "Nearly."]);
    let (text, error) = composer(cx);
    assert_eq!(text, "Hello?");
    assert!(error.is_some_and(|error| error.contains("403")));
}

#[gpui::test]
fn what_is_being_written_is_kept_for_each_conversation(cx: &mut TestAppContext) {
    let service = two_people();
    let (_, cx) = start(&service, cx);
    let open = |id: &str, cx: &mut VisualTestContext| {
        let id = id.to_owned();
        cx.update(|_, cx| carry_out(Command::OpenConversation(id), cx));
        settle(cx);
    };

    open("dm/ada-dm", cx);
    cx.simulate_input("to Ada");
    open("dm/grace-dm", cx);
    assert_eq!(composer(cx).0, "");
    cx.simulate_input("to Grace");
    open("dm/ada-dm", cx);
    assert_eq!(composer(cx).0, "to Ada");
    open("dm/grace-dm", cx);
    assert_eq!(composer(cx).0, "to Grace");
}

/// Does something in the app whether or not its window is open.
fn in_app<R>(cx: &mut TestAppContext, change: impl FnOnce(&mut App) -> R) -> R {
    cx.update(change)
}

/// What each message that arrived came to: nothing, the sound, or a banner's title, body
/// and whether it had the sound.
fn notified(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let notified = std::mem::take(&mut cx.global_mut::<notify::Notifier>().notified);
        notified
            .into_iter()
            .map(|notify| match notify {
                notify::Notify::Nothing => "nothing".to_owned(),
                notify::Notify::Sound => "sound".to_owned(),
                notify::Notify::Banner(notice) => {
                    let sound = if notice.sound { "with sound" } else { "silent" };
                    format!("{}: {} ({sound})", notice.title, notice.body)
                }
            })
            .collect()
    })
}

#[gpui::test]
fn a_message_brings_a_banner_only_when_peep_is_not_showing_it(cx: &mut TestAppContext) {
    let service = two_people();
    let (_, cx) = start(&service, cx);
    cx.simulate_keystrokes("tab down");
    settle(cx);

    service.post("dm/ada-dm", "ada", "Ready now.");
    service.post("dm/grace-dm", "grace", "Six");
    service.post("dm/grace-dm", "me", "Seven, from my phone");
    settle(cx);
    assert_eq!(
        notified(cx),
        ["nothing", "sound"],
        "in front: nothing for the open conversation, the sound for another, and never \
         anything for my own messages"
    );

    cx.simulate_keystrokes("cmd-w");
    settle(cx);
    service.post("dm/ada-dm", "ada", "Shall I start it?");
    service.post("dm/ada-dm", "ada", "Starting.");
    settle(cx);
    assert_eq!(
        notified(cx),
        [
            "Ada: Shall I start it? (with sound)",
            "Ada: Starting. (silent)"
        ],
        "with the window closed: a banner each, chirping once"
    );
}

#[gpui::test]
fn pausing_and_do_not_disturb_keep_banners_away(cx: &mut TestAppContext) {
    let service = two_people().mute("dm/grace-dm");
    let (_, cx) = start(&service, cx);
    cx.simulate_keystrokes("cmd-w");
    settle(cx);

    service.post("dm/grace-dm", "grace", "Six");
    settle(cx);
    assert_eq!(notified(cx), [] as [&str; 0], "a muted conversation");

    in_app(cx, |cx| carry_out(Command::Pause(true), cx));
    service.post("dm/ada-dm", "ada", "One");
    settle(cx);
    in_app(cx, |cx| carry_out(Command::Pause(false), cx));
    let until_later = Choice::DoNotDisturbFor(Duration::from_secs(1800));
    in_app(cx, |cx| carry_out(Command::Show(until_later), cx));
    settle(cx);
    service.post("dm/ada-dm", "ada", "Two");
    settle(cx);
    in_app(cx, |cx| carry_out(Command::Show(Choice::Automatic), cx));
    settle(cx);
    service.post("dm/ada-dm", "ada", "Three");
    settle(cx);

    assert_eq!(
        notified(cx),
        ["nothing", "nothing", "Ada: Three (with sound)"],
        "paused, then not to be disturbed, then neither"
    );
}

#[gpui::test]
fn reactions_show_as_they_change_and_my_messages_are_told_from_others(cx: &mut TestAppContext) {
    let service = two_people();
    let (account, cx) = start(&service, cx);
    cx.simulate_keystrokes("tab down");
    settle(cx);
    let shown = |cx: &mut VisualTestContext| {
        account.read_with(cx, |account, _| {
            let entries = account.timeline(&"dm/ada-dm".into()).unwrap().entries();
            let shown = entries.iter().map(|entry| {
                let reactions: Vec<String> = entry
                    .reactions
                    .iter()
                    .map(|one| format!("{}{}", one.emoji, one.count))
                    .collect();
                (entry.mine, reactions.join(" "))
            });
            shown.collect::<Vec<_>>()
        })
    };
    assert_eq!(shown(cx), [(true, String::new()), (false, String::new())]);

    service.say(Event::Reactions {
        conversation: ConversationId::Dm("ada-dm".into()),
        message: "ada-1".into(),
        reactions: vec![Reaction {
            emoji: "👍".into(),
            count: 2,
            mine: true,
        }],
    });
    settle(cx);
    assert_eq!(shown(cx), [(true, String::new()), (false, "👍2".into())]);

    service.say(Event::Reactions {
        conversation: ConversationId::Dm("ada-dm".into()),
        message: "ada-1".into(),
        reactions: Vec::new(),
    });
    settle(cx);
    assert_eq!(shown(cx)[1].1, "", "the last one was taken back");
}

#[gpui::test]
fn someone_outside_the_conversation_is_found_and_only_named(cx: &mut TestAppContext) {
    let service = two_people().colleague("Linus").colleague("Lina");
    let (_, cx) = start(&service, cx);
    cx.simulate_keystrokes("tab down enter");
    settle(cx);
    service.asked();

    cx.simulate_input("ask @li");
    assert!(
        !service.asked().contains(&Asked::People("li".into())),
        "the organisation is not searched letter by letter"
    );
    settle(cx);
    assert!(service.asked().contains(&Asked::People("li".into())));

    // Nobody in the conversation is called Li-something: the first found is chosen.
    cx.simulate_keystrokes("down enter");
    assert_eq!(composer(cx).0, "ask @Linus ");
    service.asked();
    cx.simulate_keystrokes("enter");
    settle(cx);

    assert_eq!(
        service.asked().last(),
        Some(&Asked::Send(
            "dm/ada-dm".into(),
            Draft {
                text: "ask @Linus".into(),
                mentions: vec![Mention {
                    person: UserId("linus".into()),
                    range: 4..10,
                    member: false,
                }]
            }
        ))
    );
}

#[gpui::test]
fn the_list_shows_with_the_names_kept_and_then_with_any_that_changed(cx: &mut TestAppContext) {
    let service = two_people().renamed("ada", "Ada King");
    let (account, cx) = start(&service, cx);

    let asked = service.asked();
    let refreshes = asked
        .iter()
        .filter(|asked| **asked == Asked::Refresh)
        .count();
    assert_eq!(
        asked.iter().position(|asked| *asked == Asked::RenewPeople),
        asked
            .iter()
            .position(|asked| *asked == Asked::Refresh)
            .map(|first| first + 1),
        "people are looked up again only after the list has loaded: {asked:?}"
    );
    assert_eq!(refreshes, 2, "a changed name loads the list once more");
    assert_eq!(list(&account, cx)[0].0, "Ada King");

    service.say(Event::ListChanged);
    settle(cx);
    assert!(
        !service.asked().contains(&Asked::RenewPeople),
        "once a launch"
    );
}
