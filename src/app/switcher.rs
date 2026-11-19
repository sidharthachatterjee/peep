//! Goes to a conversation by typing some of its name. With nothing typed it lists what is
//! unread, then what was last active.

use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, MouseButton, Render,
    SharedString, Subscription, Window, div, prelude::*,
};

use crate::{
    account::{Account, Row},
    theme::ActiveTheme,
    ui::{
        Avatar, Badge, Choice, Color, ControlSize, Divider, Elevation, Label, ListItem, Surface,
        TextInput, TextInputEvent, TextSize,
    },
};

/// The most conversations listed at once.
const MOST_LISTED: usize = 8;

/// What the switcher tells the window it is in.
#[derive(Clone, Debug, PartialEq)]
pub enum SwitcherEvent {
    /// Go to the conversation with this id.
    Picked(SharedString),
    /// Put the switcher away.
    Dismissed,
}

/// A run of conversations under a heading, or under none.
struct Section {
    heading: Option<&'static str>,
    rows: Vec<Row>,
}

pub struct Switcher {
    account: Entity<Account>,
    input: Entity<TextInput>,
    /// Which of the conversations listed is chosen, counting through every section.
    chosen: usize,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<SwitcherEvent> for Switcher {}

impl Switcher {
    pub fn new(account: Entity<Account>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Go to a conversation", cx).most_rows(1);
            // Up, Down, Enter and Escape are for the list from the start.
            input.set_choosing(true, cx);
            input
        });
        Self {
            _subscriptions: [
                cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| match event {
                    TextInputEvent::Changed => {
                        this.chosen = 0;
                        cx.notify();
                    }
                    TextInputEvent::Submitted => {}
                    TextInputEvent::Choice(choice) => this.choose(*choice, cx),
                }),
                cx.observe(&account, |_, _, cx| cx.notify()),
            ],
            account,
            input,
            chosen: 0,
        }
    }

    /// The conversations to list for what is typed.
    fn sections(&self, cx: &Context<Self>) -> Vec<Section> {
        let account = self.account.read(cx);
        let typed = self.input.read(cx).text();
        if !typed.trim().is_empty() {
            return vec![Section {
                heading: None,
                rows: matching(account.rows(), typed),
            }];
        }
        let unread: Vec<Row> = account.unread_rows(MOST_LISTED).cloned().collect();
        let recent: Vec<Row> = account
            .rows()
            .iter()
            .filter(|row| !unread.iter().any(|unread| unread.id == row.id))
            .take(MOST_LISTED - unread.len())
            .cloned()
            .collect();
        let section = |heading, rows: Vec<Row>| Section {
            heading: Some(heading),
            rows,
        };
        let sections = [section("Unread", unread), section("Recent", recent)];
        sections
            .into_iter()
            .filter(|section| !section.rows.is_empty())
            .collect()
    }

    fn choose(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let listed: Vec<SharedString> = self
            .sections(cx)
            .into_iter()
            .flat_map(|section| section.rows)
            .map(|row| row.id)
            .collect();
        let last = listed.len().saturating_sub(1);
        let chosen = self.chosen.min(last);
        match choice {
            Choice::Previous => self.chosen = chosen.checked_sub(1).unwrap_or(last),
            Choice::Next => self.chosen = if chosen == last { 0 } else { chosen + 1 },
            Choice::Accept => {
                if let Some(id) = listed.get(chosen) {
                    cx.emit(SwitcherEvent::Picked(id.clone()));
                }
            }
            Choice::Dismiss => cx.emit(SwitcherEvent::Dismissed),
        }
        cx.notify();
    }

    /// What is listed, a line each: headings in brackets, the chosen conversation marked.
    #[cfg(test)]
    pub(super) fn listed(&self, cx: &Context<Self>) -> Vec<String> {
        let mut index = 0;
        let mut lines = Vec::new();
        for section in self.sections(cx) {
            lines.extend(section.heading.map(|heading| format!("[{heading}]")));
            for row in section.rows {
                let mark = if index == self.chosen { "> " } else { "" };
                lines.push(format!("{mark}{}", row.title));
                index += 1;
            }
        }
        lines
    }
}

/// The conversations whose name fits what was typed, best first: a name that starts with
/// it, then one whose words start with its words, then one that has it anywhere. Among
/// equals the most recently active comes first, as `rows` has them.
fn matching(rows: &[Row], typed: &str) -> Vec<Row> {
    let typed = typed.trim().to_lowercase();
    let words: Vec<&str> = typed.split_whitespace().collect();
    let mut matching: Vec<(u8, &Row)> = rows
        .iter()
        .filter_map(|row| {
            let title = row.title.to_lowercase();
            let starts_a_word = |typed: &&str| {
                let mut words = title.split(|c: char| !c.is_alphanumeric());
                words.any(|word| word.starts_with(typed))
            };
            let rank = if title.starts_with(&typed) {
                0
            } else if words.iter().all(starts_a_word) {
                1
            } else if title.contains(&typed) {
                2
            } else {
                return None;
            };
            Some((rank, row))
        })
        .collect();
    matching.sort_by_key(|(rank, _)| *rank);
    matching.truncate(MOST_LISTED);
    matching.into_iter().map(|(_, row)| row.clone()).collect()
}

impl Focusable for Switcher {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for Switcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let sections = self.sections(cx);
        let chosen = self.chosen;
        let nothing = sections.is_empty();

        let mut index = 0;
        let mut list = div().flex().flex_col().p(theme.spacing.xs);
        for section in sections {
            list = list.children(section.heading.map(|heading| {
                div().px(theme.spacing.sm).py(theme.spacing.xs).child(
                    Label::new(heading)
                        .size(TextSize::Caption)
                        .color(Color::Muted)
                        .emphasized(),
                )
            }));
            for row in section.rows {
                let id = row.id.clone();
                let title = Label::new(row.title.clone()).truncate();
                list =
                    list.child(
                        ListItem::new(("conversation", index))
                            .selected(index == chosen)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(SwitcherEvent::Picked(id.clone()));
                            }))
                            .child(
                                Avatar::new(row.title.clone())
                                    .size(ControlSize::Small)
                                    .photo(row.photo.clone())
                                    .presence(row.presence),
                            )
                            .child(div().flex_1().overflow_hidden().child(
                                match row.unread.is_some() {
                                    true => title.emphasized(),
                                    false => title,
                                },
                            ))
                            .children(row.unread.clone().map(Badge::new)),
                    );
                index += 1;
            }
        }
        if nothing {
            list = list.child(
                div()
                    .px(theme.spacing.sm)
                    .py(theme.spacing.md)
                    .child(Label::new("No conversation has that name.").color(Color::Muted)),
            );
        }

        div()
            .w(theme.size.switcher)
            .max_w_full()
            // A click inside is not a click away.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Surface::new(Elevation::Modal)
                    .child(
                        div()
                            .px(theme.spacing.lg)
                            .py(theme.spacing.md)
                            .child(self.input.clone()),
                    )
                    .child(Divider::horizontal())
                    .child(list),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_by_their_start_then_their_words_then_anywhere() {
        let rows: Vec<Row> = [
            "Grace Hopper",
            "Ada Lovelace",
            "Alan Hart, Ada Lovelace",
            "Engine room",
            "Margaret Hamilton",
        ]
        .iter()
        .map(|title| Row {
            id: (*title).into(),
            title: (*title).into(),
            ..Default::default()
        })
        .collect();
        let found = |typed: &str| {
            let matching = matching(&rows, typed);
            matching
                .into_iter()
                .map(|row| row.title.to_string())
                .collect::<Vec<_>>()
        };

        assert_eq!(found("ada"), ["Ada Lovelace", "Alan Hart, Ada Lovelace"]);
        assert_eq!(
            found("h"),
            [
                "Grace Hopper",
                "Alan Hart, Ada Lovelace",
                "Margaret Hamilton"
            ]
        );
        assert_eq!(found("ada lo"), ["Ada Lovelace", "Alan Hart, Ada Lovelace"]);
        assert_eq!(
            found("lo ad"),
            ["Ada Lovelace", "Alan Hart, Ada Lovelace"],
            "in any order"
        );
        assert_eq!(found(" GRACE "), ["Grace Hopper"]);
        assert_eq!(found("gine"), ["Engine room"], "anywhere, last");
        assert_eq!(found("zz"), [] as [&str; 0]);
    }
}
