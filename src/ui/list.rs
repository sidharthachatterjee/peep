use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, IntoElement, ParentElement, RenderOnce, Window, div,
    prelude::*, transparent_black,
};

use super::{
    actions::{Confirm, Next, Previous},
    style::{Preview, focus, focus_ring},
};
use crate::theme::ActiveTheme;

type SelectHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A column of rows with one selected. It is one stop for Tab; Up and Down move the
/// selection and Enter confirms it. Rows are [`ListItem`]s that select themselves on click.
#[derive(IntoElement)]
pub struct List {
    id: ElementId,
    count: usize,
    selected: Option<usize>,
    preview: Option<Preview>,
    on_select: Option<SelectHandler>,
    on_confirm: Option<SelectHandler>,
    children: Vec<AnyElement>,
}

impl List {
    /// `count` is the number of rows the keyboard can move between.
    pub fn new(id: impl Into<ElementId>, count: usize) -> Self {
        Self {
            id: id.into(),
            count,
            selected: None,
            preview: None,
            on_select: None,
            on_confirm: None,
            children: Vec::new(),
        }
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Called with the index of the row the keyboard moved to.
    pub fn on_select(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Called with the index of the selected row when the person presses Enter.
    pub fn on_confirm(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }

    pub fn preview(mut self, state: Preview) -> Self {
        self.preview = Some(state);
        self
    }
}

impl ParentElement for List {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for List {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let interactive = self.preview.is_none();
        let (focus_handle, focused) = focus(&self.id, interactive, self.preview, window, cx);
        let theme = cx.theme();
        let selected = self.selected;
        let last = self.count.checked_sub(1);

        let move_to = |to: Option<usize>| {
            let on_select = self.on_select.clone();
            move |window: &mut Window, cx: &mut App| {
                if let (Some(to), Some(on_select)) = (to, &on_select)
                    && Some(to) != selected
                {
                    on_select(&to, window, cx);
                }
            }
        };
        let next = move_to(last.map(|last| selected.map_or(0, |ix| (ix + 1).min(last))));
        let previous = move_to(last.map(|_| selected.map_or(0, |ix| ix.saturating_sub(1))));
        let on_confirm = self.on_confirm;

        div()
            .id(self.id)
            .flex()
            .flex_col()
            .gap(theme.spacing.xxs)
            .rounded(theme.radius.md)
            .on_action(move |_: &Next, window, cx| next(window, cx))
            .on_action(move |_: &Previous, window, cx| previous(window, cx))
            .on_action(
                move |_: &Confirm, window, cx| match (selected, &on_confirm) {
                    (Some(selected), Some(on_confirm)) => on_confirm(&selected, window, cx),
                    _ => cx.propagate(),
                },
            )
            .when_some(focus_handle, |this, handle| this.track_focus(&handle))
            .when(focused, |this| {
                this.child(focus_ring(theme.radius.md, theme))
            })
            .children(self.children)
    }
}

/// A row in a list or sidebar. It shares the fill of its surface until hovered or selected.
#[derive(IntoElement)]
pub struct ListItem {
    id: ElementId,
    selected: bool,
    disabled: bool,
    preview: Option<Preview>,
    on_click: Option<ClickHandler>,
    children: Vec<AnyElement>,
}

impl ListItem {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            selected: false,
            disabled: false,
            preview: None,
            on_click: None,
            children: Vec::new(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub fn preview(mut self, state: Preview) -> Self {
        self.preview = Some(state);
        self
    }
}

impl ParentElement for ListItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ListItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let fill = match (self.selected, self.preview) {
            (true, _) => colors.ghost_selected,
            (false, Some(Preview::Hover)) => colors.ghost_hover,
            (false, Some(Preview::Pressed)) => colors.ghost_active,
            (false, _) => transparent_black(),
        };
        let (hover, pressed) = (colors.ghost_hover, colors.ghost_active);
        let interactive = !self.disabled && self.preview.is_none();

        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(theme.spacing.sm)
            .px(theme.spacing.sm)
            .py(theme.spacing.xs)
            .rounded(theme.radius.md)
            .bg(fill)
            .when(self.disabled, |this| this.text_color(colors.text_disabled))
            .when(interactive, |this| {
                this.when(!self.selected, |this| this.hover(|style| style.bg(hover)))
                    .active(|style| style.bg(pressed))
                    .when_some(self.on_click, |this, on_click| {
                        this.on_click(move |event, window, cx| on_click(event, window, cx))
                    })
            })
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext, point};

    use super::*;
    use crate::ui::{
        style::focus_visible,
        test::{focus_is_on_root, open, press},
    };

    #[derive(Default)]
    struct Host {
        selected: Option<usize>,
        opened: Vec<usize>,
        confirmed_outside: usize,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let rows = (0..3).map(|ix| {
                ListItem::new(ix)
                    .selected(self.selected == Some(ix))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(ix);
                        cx.notify();
                    }))
                    .child(format!("Row {ix}"))
            });
            div()
                .flex()
                .on_action(cx.listener(|this, _: &Confirm, _, _| this.confirmed_outside += 1))
                .child(
                    div().debug_selector(|| "rows".into()).child(
                        List::new("rows", 3)
                            .selected(self.selected)
                            .on_select(cx.listener(|this, ix: &usize, _, cx| {
                                this.selected = Some(*ix);
                                cx.notify();
                            }))
                            .on_confirm(cx.listener(|this, ix: &usize, _, _| this.opened.push(*ix)))
                            .children(rows),
                    ),
                )
        }
    }

    fn selected(host: &gpui::Entity<Host>, cx: &mut gpui::VisualTestContext) -> Option<usize> {
        host.read_with(cx, |host, _| host.selected)
    }

    #[gpui::test]
    fn up_and_down_move_the_selection_and_stop_at_the_ends(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        press(cx, "tab down");
        assert_eq!(selected(&host, cx), Some(0), "Down selects the first row");

        press(cx, "down down down");
        assert_eq!(selected(&host, cx), Some(2), "stops at the last row");

        press(cx, "up up up");
        assert_eq!(selected(&host, cx), Some(0), "stops at the first row");
    }

    #[gpui::test]
    fn enter_confirms_the_selected_row_or_passes_through(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());

        press(cx, "tab enter");
        assert_eq!(
            host.read_with(cx, |host, _| (host.opened.clone(), host.confirmed_outside)),
            (vec![], 1),
            "with nothing selected, Enter goes to the container"
        );

        press(cx, "down down enter");
        assert_eq!(
            host.read_with(cx, |host, _| (host.opened.clone(), host.confirmed_outside)),
            (vec![1], 1)
        );
    }

    #[gpui::test]
    fn clicking_a_row_selects_it_and_focuses_the_list_without_a_ring(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host::default());
        let bounds = cx.debug_bounds("rows").unwrap();
        let last_row = bounds.origin + point(bounds.size.width / 2., bounds.size.height * 0.9);

        cx.simulate_click(last_row, Modifiers::none());
        assert_eq!(selected(&host, cx), Some(2));
        assert!(!focus_is_on_root(cx), "the list took focus");
        assert!(
            !cx.update(|_, cx| focus_visible(cx)),
            "a click shows no ring"
        );

        press(cx, "up");
        assert_eq!(
            selected(&host, cx),
            Some(1),
            "arrow keys work after a click"
        );
    }
}
