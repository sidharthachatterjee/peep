use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, img, prelude::*};

use super::style::{ControlSize, text_style};
use crate::{chat::Presence, photos::Photo, theme::ActiveTheme};

/// A person or space in a circle: their photo, or their initials until it has loaded or
/// when they have none. A dot at the corner can say whether the person is around.
#[derive(IntoElement)]
pub struct Avatar {
    name: SharedString,
    size: ControlSize,
    photo: Option<SharedString>,
    presence: Option<Presence>,
}

impl Avatar {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            size: ControlSize::default(),
            photo: None,
            presence: None,
        }
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// Where the photo is, as Google gives it.
    pub fn photo(mut self, photo: Option<SharedString>) -> Self {
        self.photo = photo;
        self
    }

    /// Shows whether the person is around.
    pub fn presence(mut self, presence: Option<Presence>) -> Self {
        self.presence = presence;
        self
    }
}

impl RenderOnce for Avatar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let size = cx.theme().size;
        let (diameter, dot) = match self.size {
            ControlSize::Small => (size.avatar_sm, size.status_sm),
            ControlSize::Medium => (size.avatar_md, size.status_md),
            ControlSize::Large => (size.avatar_lg, size.status_lg),
        };
        // Asked for at the pixels it covers, so it is sharp and no larger than needed.
        let pixels = (f32::from(diameter) * window.scale_factor()).round() as u32;
        let photo = self
            .photo
            .and_then(|photo| window.use_asset::<Photo>(&(photo, pixels), cx).flatten());

        let theme = cx.theme();
        let face = match photo {
            Some(photo) => img(photo)
                .size(diameter)
                .rounded(theme.radius.full)
                .into_any_element(),
            None => {
                let mut text = match self.size {
                    ControlSize::Small => theme.text.caption,
                    ControlSize::Medium => theme.text.small,
                    ControlSize::Large => theme.text.body,
                };
                text.weight = theme.text.emphasis;
                let initials: String = self
                    .name
                    .split_whitespace()
                    .filter_map(|word| word.chars().next())
                    .take(2)
                    .flat_map(char::to_uppercase)
                    .collect();
                text_style(div(), text)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(diameter)
                    .rounded(theme.radius.full)
                    .bg(theme.colors.element_active)
                    .text_color(theme.colors.text)
                    .child(initials)
                    .into_any_element()
            }
        };

        // The dot sits in a ring of the surface's colour, which sets it off from the photo.
        let ring = theme.size.status_ring;
        let dot = self.presence.map(|presence| {
            let (fill, edge) = match presence {
                Presence::Active => (theme.colors.success, theme.colors.success),
                Presence::Idle => (theme.colors.warning, theme.colors.warning),
                Presence::Away => (theme.colors.background, theme.colors.text_muted),
                Presence::DoNotDisturb => (theme.colors.danger, theme.colors.danger),
            };
            div()
                .absolute()
                .right(-ring)
                .bottom(-ring)
                .p(ring)
                .rounded(theme.radius.full)
                .bg(theme.colors.background)
                .child(
                    div()
                        .size(dot)
                        .rounded(theme.radius.full)
                        .bg(fill)
                        .border(theme.size.stroke)
                        .border_color(edge),
                )
        });

        div()
            .relative()
            .flex_none()
            .size(diameter)
            .child(face)
            .children(dot)
    }
}
