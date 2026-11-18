use gpui::{
    Context, Entity, FocusHandle, Focusable, IntoElement, Render, ScrollStrategy, SharedString,
    Subscription, UniformListScrollHandle, Window, div, prelude::*, uniform_list,
};

use crate::{
    account::Account,
    theme::ActiveTheme,
    ui::{Avatar, Badge, Color, Label, ListItem, Next, Previous, focus_visible},
};

/// The list of direct messages. It is one stop for Tab; Up and Down move the selection.
pub struct Conversations {
    account: Entity<Account>,
    /// The id of the selected conversation, so the selection survives the list reordering.
    selected: Option<SharedString>,
    /// The rows last drawn and the selection then, as last told to the account.
    shown: (std::ops::Range<usize>, Option<SharedString>),
    focus_handle: FocusHandle,
    scroll: UniformListScrollHandle,
    _account_changed: Subscription,
}

impl Conversations {
    pub fn new(account: Entity<Account>, cx: &mut Context<Self>) -> Self {
        Self {
            _account_changed: cx.observe(&account, |this, account, cx| {
                let gone = this.selected.as_ref().is_some_and(|selected| {
                    !account
                        .read(cx)
                        .rows()
                        .iter()
                        .any(|row| row.id == *selected)
                });
                if gone {
                    this.selected = None;
                }
                cx.notify();
            }),
            account,
            selected: None,
            shown: (0..0, None),
            focus_handle: cx.focus_handle().tab_index(0).tab_stop(true),
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// The id of the selected conversation. It is cleared when the conversation leaves the
    /// list.
    pub fn selected_id(&self) -> Option<SharedString> {
        self.selected.clone()
    }

    /// Selects the conversation with this id, if it is in the list.
    pub fn select_id(&mut self, id: &str, cx: &mut Context<Self>) {
        let index = self
            .account
            .read(cx)
            .rows()
            .iter()
            .position(|row| row.id.as_ref() == id);
        if let Some(index) = index {
            self.select(index, cx);
        }
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(id) = self
            .account
            .read(cx)
            .rows()
            .get(index)
            .map(|row| row.id.clone())
        else {
            return;
        };
        if self.selected.as_ref() != Some(&id) {
            self.selected = Some(id);
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
            cx.notify();
        }
    }

    /// Moves the selection one row down or up, stopping at the ends. With nothing selected,
    /// either direction selects the first row.
    fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let rows = self.account.read(cx).rows();
        let Some(last) = rows.len().checked_sub(1) else {
            return;
        };
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|row| row.id == *selected));
        let target = match current {
            None => 0,
            Some(index) if down => (index + 1).min(last),
            Some(index) => index.saturating_sub(1),
        };
        self.select(target, cx);
    }
}

impl Focusable for Conversations {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Conversations {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let count = self.account.read(cx).rows().len();
        let show_focus = focus_visible(cx) && self.focus_handle.is_focused(window);

        let rows = uniform_list(
            "conversations",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                // The account watches the people on screen. It is told once this frame is
                // drawn: it cannot change while it is being read.
                let shown = (range.clone(), this.selected.clone());
                if this.shown != shown {
                    this.shown = shown.clone();
                    let account = this.account.clone();
                    cx.defer(move |cx| {
                        account.update(cx, |account, cx| account.show_rows(shown.0, shown.1, cx));
                    });
                }
                let rows = this.account.read(cx).rows();
                let start = range.start;
                rows.get(range)
                    .unwrap_or_default()
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| {
                        let index = start + offset;
                        let selected = this.selected.as_ref() == Some(&row.id);
                        let title = Label::new(row.title.clone()).truncate();
                        ListItem::new(row.id.clone())
                            .selected(selected)
                            .focused(selected && show_focus)
                            .on_click(cx.listener(move |this, _, _, cx| this.select(index, cx)))
                            .child(
                                Avatar::new(row.title.clone())
                                    .photo(row.photo.clone())
                                    .presence(row.presence),
                            )
                            .child(div().flex_1().overflow_hidden().child(
                                if row.unread.is_some() {
                                    title.emphasized()
                                } else if row.muted {
                                    title.color(Color::Muted)
                                } else {
                                    title
                                },
                            ))
                            .children(row.unread.clone().map(Badge::new))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full();

        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &Next, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &Previous, _, cx| this.step(false, cx)))
            .flex_none()
            .w(theme.size.sidebar)
            .h_full()
            .p(theme.spacing.sm)
            .bg(theme.colors.surface)
            .child(rows)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext, VisualTestContext, point};

    use super::*;
    use crate::{
        account::{Row, Status},
        ui::Root,
    };

    fn row(id: &str) -> Row {
        Row {
            id: id.to_owned().into(),
            title: format!("Person {id}").into(),
            ..Default::default()
        }
    }

    fn open(
        cx: &mut TestAppContext,
        rows: Vec<Row>,
    ) -> (
        Entity<Account>,
        Entity<Conversations>,
        &mut VisualTestContext,
    ) {
        cx.update(crate::ui::init);
        let account = cx.new(|_| Account::with(Status::Ready, rows));
        let mut list = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Conversations::new(account.clone(), cx));
            list = Some(view.clone());
            Root::new(view, window, cx)
        });
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        (account, list.unwrap(), cx)
    }

    fn selected(list: &Entity<Conversations>, cx: &mut VisualTestContext) -> Option<String> {
        list.read_with(cx, |list, _| list.selected_id().map(|id| id.to_string()))
    }

    #[gpui::test]
    fn up_and_down_move_the_selection_and_stop_at_the_ends(cx: &mut TestAppContext) {
        let (_, list, cx) = open(cx, vec![row("a"), row("b"), row("c")]);
        assert_eq!(selected(&list, cx), None);

        cx.simulate_keystrokes("tab down");
        assert_eq!(selected(&list, cx).as_deref(), Some("a"));

        cx.simulate_keystrokes("down down down");
        assert_eq!(
            selected(&list, cx).as_deref(),
            Some("c"),
            "stops at the last row"
        );

        cx.simulate_keystrokes("up up up");
        assert_eq!(
            selected(&list, cx).as_deref(),
            Some("a"),
            "stops at the first row"
        );
    }

    #[gpui::test]
    fn clicking_a_row_selects_it(cx: &mut TestAppContext) {
        let (_, list, cx) = open(cx, vec![row("a"), row("b"), row("c")]);
        let padding = cx.update(|_, cx| cx.theme().spacing.sm);
        let titlebar = cx.update(|_, cx| cx.theme().size.titlebar);
        let row_height = cx.update(|_, cx| {
            let theme = cx.theme();
            theme.size.avatar_md + theme.spacing.xs * 2.
        });

        let second_row = point(padding * 4., titlebar + padding + row_height * 1.5);
        cx.simulate_click(second_row, Modifiers::none());

        assert_eq!(selected(&list, cx).as_deref(), Some("b"));
    }

    #[gpui::test]
    fn the_selection_follows_its_conversation_when_the_list_changes(cx: &mut TestAppContext) {
        let (account, list, cx) = open(cx, vec![row("a"), row("b")]);
        cx.simulate_keystrokes("tab down down");
        assert_eq!(selected(&list, cx).as_deref(), Some("b"));

        account.update(cx, |account, cx| {
            *account = Account::with(Status::Ready, vec![row("c"), row("b"), row("a")]);
            cx.notify();
        });
        assert_eq!(
            selected(&list, cx).as_deref(),
            Some("b"),
            "reordering keeps it"
        );

        cx.simulate_keystrokes("up");
        assert_eq!(selected(&list, cx).as_deref(), Some("c"));

        account.update(cx, |account, cx| {
            *account = Account::with(Status::Ready, vec![row("a")]);
            cx.notify();
        });
        assert_eq!(selected(&list, cx), None, "its conversation is gone");
    }
}
