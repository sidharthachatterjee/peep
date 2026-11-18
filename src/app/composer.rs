//! Where a message is written: the box, who is typing back, and the list of people to name.

use std::{collections::HashMap, ops::Range, time::Duration};

use gpui::{
    Context, Entity, FocusHandle, Focusable, IntoElement, Render, SharedString, Subscription, Task,
    Window, div, prelude::*,
};

use crate::{
    account::{Account, Person, SendFailed},
    chat::{Draft, Mention, UserId},
    theme::ActiveTheme,
    ui::{
        Avatar, Button, ButtonVariant, Choice, Color, ControlSize, Elevation, Label, ListItem,
        Surface, TextInput, TextInputEvent, TextSize,
    },
};

/// How many rows the box grows to before it scrolls.
const MOST_ROWS: usize = 8;
/// How many people are offered at once for a name being typed.
const MOST_OFFERED: usize = 6;
/// How far back from the caret an `@` is looked for.
const LONGEST_NAME: usize = 40;
/// How long typing a name must pause before the organisation is searched for it.
const SEARCH_AFTER: Duration = Duration::from_millis(150);

/// A name being typed after an `@`, and who it could be.
#[derive(Clone, Debug, PartialEq)]
struct Naming {
    /// The `@` and what follows it, up to the caret.
    range: Range<usize>,
    /// What follows the `@`.
    typed: String,
    /// The people in the conversation it could be, then others in the organisation, as
    /// Google Chat offers them.
    members: Vec<Person>,
    others: Vec<Person>,
    /// Which of them all is chosen, counting through the members first.
    chosen: usize,
}

impl Naming {
    fn offered(&self) -> usize {
        self.members.len() + self.others.len()
    }
}

/// What is being written to one conversation.
#[derive(Clone, Debug, Default)]
struct Writing {
    text: String,
    /// The parts of the text that name someone.
    named: Vec<Range<usize>>,
}

pub struct Composer {
    account: Entity<Account>,
    conversation: Option<SharedString>,
    input: Entity<TextInput>,
    /// What was left unsent in the other conversations.
    unsent: HashMap<SharedString, Writing>,
    /// Who each `@Name` in the text is, and whether they are in the conversation.
    names: HashMap<String, (UserId, bool)>,
    naming: Option<Naming>,
    /// Searches the organisation for the name being typed.
    search: Task<()>,
    /// Why the last message could not be sent.
    error: Option<SharedString>,
    _subscriptions: [Subscription; 3],
}

impl Composer {
    pub fn new(account: Entity<Account>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Message", cx).most_rows(MOST_ROWS));
        Self {
            _subscriptions: [
                cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| match event {
                    TextInputEvent::Changed => this.written(cx),
                    TextInputEvent::Submitted => this.send(cx),
                    TextInputEvent::Choice(choice) => this.choose(*choice, cx),
                }),
                cx.subscribe(&account, |this, _, failed: &SendFailed, cx| {
                    this.failed(failed, cx);
                }),
                cx.observe(&account, |_, _, cx| cx.notify()),
            ],
            account,
            conversation: None,
            input,
            unsent: HashMap::new(),
            names: HashMap::new(),
            naming: None,
            search: Task::ready(()),
            error: None,
        }
    }

    /// Writes to another conversation, or none. What was being written to the one before is
    /// kept for when it is come back to.
    pub fn show(&mut self, conversation: Option<SharedString>, cx: &mut Context<Self>) {
        if self.conversation == conversation {
            return;
        }
        let writing = self.writing(cx);
        if let Some(before) = self.conversation.take() {
            if writing.text.is_empty() {
                self.unsent.remove(&before);
            } else {
                self.unsent.insert(before, writing);
            }
        }
        let writing = conversation
            .as_ref()
            .and_then(|conversation| self.unsent.remove(conversation))
            .unwrap_or_default();
        self.conversation = conversation;
        self.error = None;
        self.set_writing(writing, cx);
    }

    /// The text in the box and the reason the last message was not sent.
    #[cfg(test)]
    pub(super) fn state(&self, cx: &gpui::App) -> (String, Option<String>) {
        let error = self.error.as_ref().map(ToString::to_string);
        (self.input.read(cx).text().to_owned(), error)
    }

    fn writing(&self, cx: &Context<Self>) -> Writing {
        let input = self.input.read(cx);
        Writing {
            text: input.text().to_owned(),
            named: input.accented().to_vec(),
        }
    }

    fn set_writing(&mut self, writing: Writing, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_text(writing.text, cx);
            input.set_accented(writing.named, cx);
        });
        self.stop_naming(cx);
    }

    /// After the text or the caret changed: says the person is typing, and offers people
    /// for a name being typed.
    fn written(&mut self, cx: &mut Context<Self>) {
        let Some(conversation) = self.conversation.clone() else {
            return;
        };
        let (text, caret, named) = {
            let input = self.input.read(cx);
            let caret = input.caret();
            (input.text().to_owned(), caret, input.accented().to_vec())
        };
        if !text.is_empty() {
            self.error = None;
            self.account
                .update(cx, |account, cx| account.typing(&conversation, cx));
        }

        let naming = name_being_typed(&text, caret)
            .filter(|range| !named.iter().any(|named| named.contains(&range.start)))
            .map(|range| {
                let typed = text[range.start + 1..range.end].to_owned();
                let before = self.naming.as_ref().filter(|before| before.typed == typed);
                Naming {
                    members: matching(self.account.read(cx).people_in(&conversation), &typed),
                    // What the organisation was found to hold for this name still stands.
                    others: before.map_or_else(Vec::new, |before| before.others.clone()),
                    chosen: before.map_or(0, |before| before.chosen),
                    range,
                    typed,
                }
            });
        let searched = self.naming.as_ref().map(|before| &before.typed);
        if let Some(typed) = naming.as_ref().map(|naming| &naming.typed)
            && searched != Some(typed)
        {
            self.search(typed.clone(), cx);
        }
        self.set_naming(naming, cx);
    }

    /// Searches the organisation for a name being typed, once typing pauses.
    fn search(&mut self, typed: String, cx: &mut Context<Self>) {
        self.search = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_AFTER).await;
            let finding = this.update(cx, |this, cx| this.account.read(cx).find_people(&typed, cx));
            let Ok(finding) = finding else { return };
            let found = finding.await;
            this.update(cx, |this, cx| {
                let Some(mut naming) = this.naming.clone().filter(|naming| naming.typed == typed)
                else {
                    return;
                };
                let room = MOST_OFFERED.saturating_sub(naming.members.len());
                naming.others = found
                    .into_iter()
                    .filter(|person| !naming.members.iter().any(|member| member.id == person.id))
                    .take(room)
                    .collect();
                this.set_naming(Some(naming), cx);
            })
            .ok();
        });
    }

    fn set_naming(&mut self, naming: Option<Naming>, cx: &mut Context<Self>) {
        let naming = naming.map(|mut naming| {
            naming.chosen = naming.chosen.min(naming.offered().saturating_sub(1));
            naming
        });
        if self.naming != naming {
            let offering = naming.as_ref().is_some_and(|naming| naming.offered() > 0);
            self.naming = naming;
            self.input
                .update(cx, |input, cx| input.set_choosing(offering, cx));
            cx.notify();
        }
    }

    fn stop_naming(&mut self, cx: &mut Context<Self>) {
        self.search = Task::ready(());
        self.set_naming(None, cx);
        cx.notify();
    }

    /// A key pressed while people are offered.
    fn choose(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let Some(naming) = &mut self.naming else {
            return;
        };
        let (chosen, last) = (naming.chosen, naming.offered().saturating_sub(1));
        match choice {
            Choice::Previous => naming.chosen = chosen.checked_sub(1).unwrap_or(last),
            Choice::Next => naming.chosen = if chosen == last { 0 } else { chosen + 1 },
            Choice::Accept => self.name(chosen, cx),
            Choice::Dismiss => self.stop_naming(cx),
        }
        cx.notify();
    }

    /// Puts the name of one of the people offered where it was being typed.
    fn name(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(naming) = self.naming.take() else {
            return;
        };
        let member = index < naming.members.len();
        let Some(person) = naming.members.iter().chain(&naming.others).nth(index) else {
            return;
        };
        let name = format!("@{}", person.name);
        self.names.insert(name.clone(), (person.id.clone(), member));
        self.search = Task::ready(());
        self.input.update(cx, |input, cx| {
            let start = naming.range.start;
            input.replace(naming.range, &format!("{name} "), cx);
            let mut named = input.accented().to_vec();
            named.push(start..start + name.len());
            input.set_accented(named, cx);
            input.set_choosing(false, cx);
        });
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(conversation) = self.conversation.clone() else {
            return;
        };
        let writing = self.writing(cx);
        // What is sent has no space or line break at either end; the names keep their place.
        let start = writing.text.len() - writing.text.trim_start().len();
        let text = writing.text.trim();
        if text.is_empty() {
            return;
        }
        let mentions = writing
            .named
            .iter()
            .filter_map(|range| {
                let (person, member) = self.names.get(&writing.text[range.clone()])?.clone();
                let range = range.start.checked_sub(start)?..range.end.checked_sub(start)?;
                (range.end <= text.len()).then_some(Mention {
                    person,
                    range,
                    member,
                })
            })
            .collect();
        let draft = Draft {
            text: text.to_owned(),
            mentions,
        };
        self.error = None;
        self.set_writing(Writing::default(), cx);
        self.account
            .update(cx, |account, cx| account.send(&conversation, draft, cx));
    }

    /// A message was not sent: it goes back in the box if nothing else has been written.
    fn failed(&mut self, failed: &SendFailed, cx: &mut Context<Self>) {
        let writing = Writing {
            text: failed.draft.text.clone(),
            named: failed
                .draft
                .mentions
                .iter()
                .map(|m| m.range.clone())
                .collect(),
        };
        if self.conversation.as_ref() != Some(&failed.conversation) {
            self.unsent
                .entry(failed.conversation.clone())
                .or_insert(writing);
            return;
        }
        self.error = Some(failed.error.clone());
        if self.input.read(cx).text().is_empty() {
            self.set_writing(writing, cx);
        }
        cx.notify();
    }
}

/// The `@` before the caret and the name typed after it, if a name is being typed: the `@`
/// starts a word, and nothing since it ends the line.
fn name_being_typed(text: &str, caret: usize) -> Option<Range<usize>> {
    let before = &text[..caret];
    let at = before.rfind('@')?;
    let typed = &before[at + 1..];
    let starts_word = before[..at]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    let plausible = typed.len() <= LONGEST_NAME
        && !typed.contains('\n')
        && !typed.starts_with(char::is_whitespace);
    (starts_word && plausible).then_some(at..caret)
}

/// The people whose name, or a word of it, starts with what was typed, best first.
fn matching(people: Vec<Person>, typed: &str) -> Vec<Person> {
    let typed = typed.to_lowercase();
    let mut matching: Vec<(u8, Person)> = people
        .into_iter()
        .filter_map(|person| {
            let name = person.name.to_lowercase();
            let rank = if name.starts_with(&typed) {
                0
            } else if name.split_whitespace().any(|word| word.starts_with(&typed)) {
                1
            } else {
                return None;
            };
            Some((rank, person))
        })
        .collect();
    matching.sort_by(|one, other| (one.0, &one.1.name).cmp(&(other.0, &other.1.name)));
    matching.truncate(MOST_OFFERED);
    matching.into_iter().map(|(_, person)| person).collect()
}

/// Who is typing, in words.
fn typing_line(names: &[SharedString]) -> Option<SharedString> {
    let first = |name: &SharedString| name.split_whitespace().next().unwrap_or(name).to_owned();
    Some(SharedString::from(match names {
        [] => return None,
        [one] => format!("{} is typing…", first(one)),
        [one, other] => format!("{} and {} are typing…", first(one), first(other)),
        _ => "Several people are typing…".to_owned(),
    }))
}

impl Focusable for Composer {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let typing = self
            .conversation
            .as_ref()
            .and_then(|conversation| typing_line(&self.account.read(cx).typing_in(conversation)));
        let below = match (&self.error, typing) {
            (Some(error), _) => Some(
                Label::new(format!("Not sent: {error}"))
                    .size(TextSize::Caption)
                    .color(Color::Danger),
            ),
            (None, Some(typing)) => Some(
                Label::new(typing)
                    .size(TextSize::Caption)
                    .color(Color::Muted),
            ),
            (None, None) => None,
        };
        let empty = self.input.read(cx).text().trim().is_empty();

        let offering = self.naming.as_ref().filter(|naming| naming.offered() > 0);
        let offered = offering.map(|naming| {
            let person = |index: usize, person: &Person| {
                ListItem::new(("person", index))
                    .selected(index == naming.chosen)
                    .on_click(cx.listener(move |this, _, _, cx| this.name(index, cx)))
                    .child(
                        Avatar::new(person.name.clone())
                            .size(ControlSize::Small)
                            .photo(person.photo.clone()),
                    )
                    .child(Label::new(person.name.clone()).truncate())
            };
            let heading = |text: &'static str| {
                div().px(theme.spacing.sm).py(theme.spacing.xs).child(
                    Label::new(text)
                        .size(TextSize::Caption)
                        .color(Color::Muted)
                        .emphasized(),
                )
            };
            let members = naming.members.iter().enumerate();
            let others = naming.others.iter().enumerate();
            let first_other = naming.members.len();
            div()
                .absolute()
                .bottom_full()
                .left(theme.spacing.lg)
                .pb(theme.spacing.xs)
                .child(
                    Surface::new(Elevation::Overlay).child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w(theme.size.popover_min)
                            .p(theme.spacing.xs)
                            .children((first_other > 0).then(|| heading("In this conversation")))
                            .children(members.map(|(index, member)| person(index, member)))
                            .children((!naming.others.is_empty()).then(|| heading("People")))
                            .children(
                                others.map(|(index, other)| person(first_other + index, other)),
                            ),
                    ),
                )
        });

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .px(theme.spacing.lg)
            .pb(theme.spacing.sm)
            .children(offered)
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(theme.spacing.sm)
                    .px(theme.spacing.md)
                    .py(theme.spacing.sm)
                    .rounded(theme.radius.lg)
                    .border(theme.size.border)
                    .border_color(theme.colors.border)
                    .bg(theme.colors.surface)
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .child(
                        Button::new("send", "Send")
                            .variant(ButtonVariant::Primary)
                            .size(ControlSize::Small)
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
                    ),
            )
            // Always as tall as its text, so the box does not move when someone types.
            .child(
                div()
                    .h(theme.text.caption.line_height)
                    .pt(theme.spacing.xxs)
                    .px(theme.spacing.xs)
                    .children(below),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_being_typed_after_an_at_that_starts_a_word() {
        fn typed(text: &str) -> Option<&str> {
            name_being_typed(text, text.len()).map(|range| &text[range])
        }
        assert_eq!(typed("@"), Some("@"));
        assert_eq!(typed("hi @ad"), Some("@ad"));
        assert_eq!(typed("hi @Ada Lo"), Some("@Ada Lo"), "names have spaces");
        assert_eq!(typed("line\n@gr"), Some("@gr"));
        assert_eq!(typed("mail me at ada@example"), None, "inside a word");
        assert_eq!(typed("@ ada"), None);
        assert_eq!(typed("@ada\nand"), None, "the line ended");
        assert_eq!(typed("nothing"), None);
        assert_eq!(
            name_being_typed("@ada and", 4),
            Some(0..4),
            "up to the caret"
        );
    }

    #[test]
    fn people_match_by_the_start_of_any_word_of_their_name() {
        let person = |name: &str| Person {
            id: UserId(name.to_lowercase().into()),
            name: name.to_owned().into(),
            photo: None,
        };
        let people = || {
            vec![
                person("Grace Hopper"),
                person("Ada Lovelace"),
                person("Alan Hart"),
            ]
        };
        let names = |typed: &str| {
            let matching = matching(people(), typed);
            matching
                .into_iter()
                .map(|person| person.name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(""), ["Ada Lovelace", "Alan Hart", "Grace Hopper"]);
        assert_eq!(names("a"), ["Ada Lovelace", "Alan Hart"]);
        assert_eq!(names("H"), ["Alan Hart", "Grace Hopper"], "a later word");
        assert_eq!(names("ada lo"), ["Ada Lovelace"]);
        assert_eq!(names("z"), [] as [&str; 0]);
    }

    #[test]
    fn who_is_typing_is_said_by_first_name() {
        let line = |names: &[&str]| {
            let names: Vec<SharedString> = names.iter().map(|name| (*name).into()).collect();
            typing_line(&names).map(|line| line.to_string())
        };
        assert_eq!(line(&[]), None);
        assert_eq!(line(&["Ada Lovelace"]).as_deref(), Some("Ada is typing…"));
        assert_eq!(
            line(&["Ada Lovelace", "Grace Hopper"]).as_deref(),
            Some("Ada and Grace are typing…")
        );
        assert_eq!(
            line(&["A", "B", "C"]).as_deref(),
            Some("Several people are typing…")
        );
    }
}
