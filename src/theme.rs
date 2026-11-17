//! The design language: every colour, size, shadow and duration the interface draws with.
//!
//! Raw values live in this module only. Everything else reads them through [`ActiveTheme`].

mod colors;
mod tokens;

use gpui::{App, Global, WindowAppearance};

pub use colors::Colors;
pub use tokens::{Motion, Radius, Shadows, Sizes, Spacing, TextToken, Typography};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

/// Which appearance to use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Follow the system setting.
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub appearance: Appearance,
    pub colors: Colors,
    pub radius: Radius,
    pub spacing: Spacing,
    pub text: Typography,
    pub size: Sizes,
    pub shadow: Shadows,
    pub motion: Motion,
}

impl Theme {
    pub fn new(appearance: Appearance) -> Self {
        let (colors, shadow) = match appearance {
            Appearance::Light => (Colors::light(), Shadows::light()),
            Appearance::Dark => (Colors::dark(), Shadows::dark()),
        };
        Self {
            appearance,
            colors,
            radius: Radius::standard(),
            spacing: Spacing::standard(),
            text: Typography::standard(),
            size: Sizes::standard(),
            shadow,
            motion: Motion::standard(),
        }
    }
}

/// Reads the theme in use: `cx.theme()`.
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        &self.global::<Active>().theme
    }
}

struct Active {
    mode: Mode,
    theme: Theme,
}

impl Global for Active {}

/// Installs the theme. Call once, before opening a window.
pub fn init(cx: &mut App) {
    set_mode(Mode::System, cx);
}

pub fn mode(cx: &App) -> Mode {
    cx.global::<Active>().mode
}

/// Switches appearance and redraws every window.
pub fn set_mode(mode: Mode, cx: &mut App) {
    let appearance = match mode {
        Mode::Light => Appearance::Light,
        Mode::Dark => Appearance::Dark,
        Mode::System => match cx.window_appearance() {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
        },
    };
    cx.set_global(Active {
        mode,
        theme: Theme::new(appearance),
    });
    cx.refresh_windows();
}

/// Re-reads the system appearance when the mode follows it.
pub fn system_appearance_changed(cx: &mut App) {
    if mode(cx) == Mode::System {
        set_mode(Mode::System, cx);
    }
}

#[cfg(test)]
mod tests {
    use gpui::Hsla;

    use super::*;

    fn luminance(color: Hsla) -> f32 {
        let color = color.to_rgb();
        let linear = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    fn contrast(first: Hsla, second: Hsla) -> f32 {
        let (first, second) = (luminance(first), luminance(second));
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    /// A translucent fill as it appears on an opaque surface.
    fn over(fill: Hsla, surface: Hsla) -> Hsla {
        let (fill, surface) = (fill.to_rgb(), surface.to_rgb());
        let mix = |top: f32, bottom: f32| top * fill.a + bottom * (1. - fill.a);
        gpui::Rgba {
            r: mix(fill.r, surface.r),
            g: mix(fill.g, surface.g),
            b: mix(fill.b, surface.b),
            a: 1.,
        }
        .into()
    }

    #[test]
    fn text_and_control_boundaries_meet_contrast_minimums() {
        const TEXT: f32 = 4.5;
        const BOUNDARY: f32 = 3.;

        for appearance in [Appearance::Light, Appearance::Dark] {
            let c = Theme::new(appearance).colors;
            let surfaces = [
                ("background", c.background),
                ("surface", c.surface),
                ("surface_raised", c.surface_raised),
            ];
            let mut pairs = Vec::new();

            for (surface_name, surface) in surfaces {
                for (name, color) in [
                    ("text", c.text),
                    ("text_muted", c.text_muted),
                    ("text_accent", c.text_accent),
                    ("text_danger", c.text_danger),
                ] {
                    pairs.push((name, surface_name.to_string(), color, surface, TEXT));
                }
                for (name, color) in [
                    ("border_control", c.border_control),
                    ("focus_ring", c.focus_ring),
                    ("accent", c.accent),
                    ("success", c.success),
                    ("warning", c.warning),
                ] {
                    pairs.push((name, surface_name.to_string(), color, surface, BOUNDARY));
                }
                for (fill_name, fill) in [
                    ("ghost_hover", c.ghost_hover),
                    ("ghost_active", c.ghost_active),
                    ("ghost_selected", c.ghost_selected),
                ] {
                    let on = format!("{fill_name} over {surface_name}");
                    pairs.push(("text", on.clone(), c.text, over(fill, surface), TEXT));
                    pairs.push(("text_muted", on, c.text_muted, over(fill, surface), TEXT));
                }
            }
            for (fill_name, fill) in [
                ("element", c.element),
                ("element_hover", c.element_hover),
                ("element_active", c.element_active),
                ("segment_selected", c.segment_selected),
            ] {
                pairs.push(("text", fill_name.to_string(), c.text, fill, TEXT));
            }
            pairs.push((
                "text_muted",
                "element".to_string(),
                c.text_muted,
                c.element,
                TEXT,
            ));
            for (fill_name, fill) in [
                ("accent", c.accent),
                ("accent_hover", c.accent_hover),
                ("accent_active", c.accent_active),
            ] {
                let on = fill_name.to_string();
                pairs.push(("text_on_accent", on, c.text_on_accent, fill, TEXT));
            }
            for (fill_name, fill) in [
                ("danger", c.danger),
                ("danger_hover", c.danger_hover),
                ("danger_active", c.danger_active),
            ] {
                let on = fill_name.to_string();
                pairs.push(("text_on_danger", on, c.text_on_danger, fill, TEXT));
            }

            let failures: Vec<String> = pairs
                .into_iter()
                .filter(|(.., color, on, minimum)| contrast(*color, *on) < *minimum)
                .map(|(name, on, color, surface, minimum)| {
                    format!(
                        "{appearance:?}: {name} on {on} is {:.2}:1, needs {minimum}:1",
                        contrast(color, surface)
                    )
                })
                .collect();
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }
    }
}
