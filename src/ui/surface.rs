use gpui::{AnyElement, App, IntoElement, ParentElement, RenderOnce, Window, div, prelude::*};

use crate::theme::ActiveTheme;

/// How far a surface sits above the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Elevation {
    /// Part of the window: sidebars, grouped content.
    #[default]
    Flat,
    /// Just above it: cards.
    Raised,
    /// Floating over it: popovers and menus.
    Overlay,
    /// Above everything: dialogs.
    Modal,
}

/// A bordered, rounded container at an [`Elevation`]. It does not pad its content.
#[derive(IntoElement)]
pub struct Surface {
    elevation: Elevation,
    children: Vec<AnyElement>,
}

impl Surface {
    pub fn new(elevation: Elevation) -> Self {
        Self {
            elevation,
            children: Vec::new(),
        }
    }
}

impl ParentElement for Surface {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Surface {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (fill, radius, shadow) = match self.elevation {
            Elevation::Flat => (theme.colors.surface, theme.radius.lg, None),
            Elevation::Raised => (
                theme.colors.surface_raised,
                theme.radius.lg,
                Some(&theme.shadow.sm),
            ),
            Elevation::Overlay => (
                theme.colors.surface_raised,
                theme.radius.lg,
                Some(&theme.shadow.md),
            ),
            Elevation::Modal => (
                theme.colors.surface_raised,
                theme.radius.xl,
                Some(&theme.shadow.lg),
            ),
        };

        div()
            .flex()
            .flex_col()
            .rounded(radius)
            .border(theme.size.border)
            .border_color(theme.colors.border)
            .bg(fill)
            .when_some(shadow, |this, shadow| this.shadow(shadow.clone()))
            .children(self.children)
    }
}

/// A line between groups of content.
#[derive(IntoElement)]
pub struct Divider {
    vertical: bool,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self { vertical: false }
    }

    pub fn vertical() -> Self {
        Self { vertical: true }
    }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div().flex_none().bg(theme.colors.border).map(|this| {
            if self.vertical {
                this.w(theme.size.border).h_full()
            } else {
                this.h(theme.size.border).w_full()
            }
        })
    }
}
