use gpui::{Hsla, rgb, rgba};

/// Every colour in the interface, named by role and state.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    /// The window behind everything else.
    pub background: Hsla,
    /// Surfaces that sit in the window: sidebars, cards.
    pub surface: Hsla,
    /// Surfaces that float above it: popovers, dialogs, the selected segment.
    pub surface_raised: Hsla,
    /// Dims the window behind a dialog.
    pub scrim: Hsla,

    /// Dividers and outlines of surfaces.
    pub border: Hsla,
    /// Boundaries a control needs to be found by, such as an empty checkbox.
    pub border_control: Hsla,
    /// The ring around the control that has keyboard focus.
    pub focus_ring: Hsla,
    /// Behind selected text.
    pub selection: Hsla,

    pub text: Hsla,
    /// Secondary text: previews, timestamps, hints.
    pub text_muted: Hsla,
    pub text_disabled: Hsla,
    /// Links and text that marks the current choice.
    pub text_accent: Hsla,
    /// Text on `accent` fills.
    pub text_on_accent: Hsla,
    /// Text on `danger` fills.
    pub text_on_danger: Hsla,
    pub text_danger: Hsla,

    /// Neutral fills: tracks, avatars, and outlined buttons under the pointer.
    pub element: Hsla,
    pub element_hover: Hsla,
    pub element_active: Hsla,
    pub element_disabled: Hsla,

    /// Controls that share the fill of their surface until used: ghost buttons, list rows.
    pub ghost_hover: Hsla,
    pub ghost_active: Hsla,
    pub ghost_selected: Hsla,

    /// The one action or choice to notice: primary buttons, checked controls.
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_active: Hsla,

    /// Destructive actions.
    pub danger: Hsla,
    pub danger_hover: Hsla,
    pub danger_active: Hsla,

    pub success: Hsla,
    pub warning: Hsla,

    /// The track of a switch that is off.
    pub switch_off: Hsla,
    /// The knob of a switch that is off. When on, the knob takes `text_on_accent`.
    pub switch_thumb: Hsla,
    /// The chosen segment of a segmented control, on its `element` track.
    pub segment_selected: Hsla,
}

impl Colors {
    pub fn light() -> Self {
        Self {
            background: hex(0xffffff),
            surface: hex(0xfafafa),
            surface_raised: hex(0xffffff),
            scrim: hexa(0x00000066),

            border: hex(0xe5e5e5),
            border_control: hex(0x8c8c8c),
            focus_ring: hex(0x2b7fff),
            selection: hexa(0x2b7fff40),

            text: hex(0x0a0a0a),
            text_muted: hex(0x666666),
            text_disabled: hex(0xa1a1a1),
            text_accent: hex(0x155dfc),
            text_on_accent: hex(0xfafafa),
            text_on_danger: hex(0xffffff),
            text_danger: hex(0xc10007),

            element: hex(0xf5f5f5),
            element_hover: hex(0xededed),
            element_active: hex(0xe5e5e5),
            element_disabled: hex(0xf7f7f7),

            ghost_hover: hexa(0x0000000a),
            ghost_active: hexa(0x00000014),
            ghost_selected: hexa(0x0000000f),

            accent: hex(0x171717),
            accent_hover: hex(0x2e2e2e),
            accent_active: hex(0x404040),

            danger: hex(0xe7000b),
            danger_hover: hex(0xcf000a),
            danger_active: hex(0xb80009),

            success: hex(0x008236),
            warning: hex(0xa65f00),

            switch_off: hex(0xd4d4d4),
            switch_thumb: hex(0xffffff),
            segment_selected: hex(0xffffff),
        }
    }

    pub fn dark() -> Self {
        Self {
            background: hex(0x0a0a0a),
            surface: hex(0x121212),
            surface_raised: hex(0x1a1a1a),
            scrim: hexa(0x00000099),

            border: hexa(0xffffff1a),
            border_control: hex(0x707070),
            focus_ring: hex(0x2b7fff),
            selection: hexa(0x2b7fff66),

            text: hex(0xfafafa),
            text_muted: hex(0xa1a1a1),
            text_disabled: hex(0x525252),
            text_accent: hex(0x51a2ff),
            text_on_accent: hex(0x171717),
            text_on_danger: hex(0xffffff),
            text_danger: hex(0xff6467),

            element: hex(0x262626),
            element_hover: hex(0x2e2e2e),
            element_active: hex(0x383838),
            element_disabled: hex(0x1c1c1c),

            ghost_hover: hexa(0xffffff14),
            ghost_active: hexa(0xffffff21),
            ghost_selected: hexa(0xffffff1a),

            accent: hex(0xe5e5e5),
            accent_hover: hex(0xd4d4d4),
            accent_active: hex(0xbdbdbd),

            danger: hex(0xe7000b),
            danger_hover: hex(0xcf000a),
            danger_active: hex(0xb80009),

            success: hex(0x05df72),
            warning: hex(0xfdc700),

            switch_off: hex(0x404040),
            switch_thumb: hex(0xe5e5e5),
            segment_selected: hex(0x0a0a0a),
        }
    }
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

fn hexa(value: u32) -> Hsla {
    rgba(value).into()
}
