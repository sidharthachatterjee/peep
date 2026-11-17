use std::rc::Rc;

use gpui::{
    App, ElementId, IntoElement, MouseButton, RenderOnce, SharedString, Window, div, prelude::*,
    transparent_black,
};

use super::{
    actions::{Next, Previous},
    style::{Preview, focus, focus_ring, text_style},
};
use crate::theme::ActiveTheme;

type SelectHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// A row of mutually exclusive choices. It is one stop for Tab; the arrow keys move the choice.
#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    segments: Vec<SharedString>,
    selected: usize,
    preview: Option<Preview>,
    on_change: Option<SelectHandler>,
}

impl SegmentedControl {
    pub fn new(id: impl Into<ElementId>, selected: usize) -> Self {
        Self {
            id: id.into(),
            segments: Vec::new(),
            selected,
            preview: None,
            on_change: None,
        }
    }

    pub fn segment(mut self, label: impl Into<SharedString>) -> Self {
        self.segments.push(label.into());
        self
    }

    /// Called with the index of the segment the person chose.
    pub fn on_change(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn preview(mut self, state: Preview) -> Self {
        self.preview = Some(state);
        self
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let interactive = self.preview.is_none();
        let (focus_handle, focused) = focus(&self.id, interactive, self.preview, window, cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let selected = self.selected;
        let last = self.segments.len().saturating_sub(1);
        let mut label = theme.text.body;
        label.weight = theme.text.emphasis;

        let segments = self.segments.into_iter().enumerate().map(|(ix, text)| {
            let on_change = self.on_change.clone();
            div()
                .id(ix)
                .flex()
                .items_center()
                .justify_center()
                .h_full()
                .px(theme.spacing.md)
                .rounded(theme.radius.sm)
                .border(theme.size.border)
                .whitespace_nowrap()
                .map(|this| {
                    if ix == selected {
                        this.bg(colors.segment_selected)
                            .border_color(colors.border)
                            .text_color(colors.text)
                            .shadow(theme.shadow.sm.clone())
                    } else {
                        this.border_color(transparent_black())
                            .text_color(colors.text_muted)
                            .hover(|style| style.text_color(colors.text))
                    }
                })
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .when_some(on_change, |this, on_change| {
                    this.on_click(move |_, window, cx| on_change(&ix, window, cx))
                })
                .child(text)
        });

        let step = |to: Option<usize>| {
            let on_change = self.on_change.clone();
            move |window: &mut Window, cx: &mut App| {
                if let (Some(to), Some(on_change)) = (to, &on_change) {
                    on_change(&to, window, cx);
                }
            }
        };
        let next = step((selected < last).then(|| selected + 1));
        let previous = step(selected.checked_sub(1));

        text_style(div(), label)
            .id(self.id)
            .flex()
            .flex_none()
            .h(theme.size.control_md)
            .p(theme.spacing.xxs)
            .gap(theme.spacing.xxs)
            .rounded(theme.radius.md)
            .bg(colors.element)
            .key_context("SegmentedControl")
            .on_action(move |_: &Next, window, cx| next(window, cx))
            .on_action(move |_: &Previous, window, cx| previous(window, cx))
            .when_some(focus_handle, |this, handle| this.track_focus(&handle))
            .when(focused, |this| {
                this.child(focus_ring(theme.radius.md, theme))
            })
            .children(segments)
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext, point};

    use super::*;
    use crate::ui::test::{open, press};

    struct Host {
        selected: usize,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().flex().child(
                div().debug_selector(|| "mode".into()).child(
                    SegmentedControl::new("mode", self.selected)
                        .segment("System")
                        .segment("Light")
                        .segment("Dark")
                        .on_change(cx.listener(|this, selected: &usize, _, cx| {
                            this.selected = *selected;
                            cx.notify();
                        })),
                ),
            )
        }
    }

    fn selected(host: &gpui::Entity<Host>, cx: &mut gpui::VisualTestContext) -> usize {
        host.read_with(cx, |host, _| host.selected)
    }

    #[gpui::test]
    fn clicking_a_segment_selects_it(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host { selected: 0 });
        let bounds = cx.debug_bounds("mode").unwrap();
        let last = bounds.origin + point(bounds.size.width * 0.9, bounds.size.height / 2.);

        cx.simulate_click(last, Modifiers::none());

        assert_eq!(selected(&host, cx), 2);
    }

    #[gpui::test]
    fn arrow_keys_move_the_selection_and_stop_at_the_ends(cx: &mut TestAppContext) {
        let (host, cx) = open(cx, |_, _| Host { selected: 1 });

        press(cx, "tab right");
        assert_eq!(selected(&host, cx), 2);

        press(cx, "right");
        assert_eq!(selected(&host, cx), 2, "stops at the last segment");

        press(cx, "left left");
        assert_eq!(selected(&host, cx), 0);

        press(cx, "left up");
        assert_eq!(selected(&host, cx), 0, "stops at the first segment");

        press(cx, "down");
        assert_eq!(selected(&host, cx), 1, "Up and Down move it too");
    }
}
