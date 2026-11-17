use std::time::Duration;

use gpui::{BoxShadow, FontWeight, Hsla, Pixels, Point, SharedString, Size, point, px, rgba, size};

/// Corner radii.
#[derive(Clone, Copy, Debug)]
pub struct Radius {
    /// Checkboxes.
    pub xs: Pixels,
    /// Segments inside a segmented control.
    pub sm: Pixels,
    /// Buttons, list rows, segmented controls.
    pub md: Pixels,
    /// Cards, popovers.
    pub lg: Pixels,
    /// Dialogs.
    pub xl: Pixels,
    /// Pills and circles.
    pub full: Pixels,
}

/// Distances between and inside elements.
#[derive(Clone, Copy, Debug)]
pub struct Spacing {
    pub xxs: Pixels,
    pub xs: Pixels,
    pub sm: Pixels,
    pub md: Pixels,
    pub lg: Pixels,
    pub xl: Pixels,
    pub xxl: Pixels,
}

/// One step of the type scale.
#[derive(Clone, Copy, Debug)]
pub struct TextToken {
    pub size: Pixels,
    pub line_height: Pixels,
    pub weight: FontWeight,
}

/// The type scale.
#[derive(Clone, Debug)]
pub struct Typography {
    pub family: SharedString,
    /// Timestamps, badges.
    pub caption: TextToken,
    /// Secondary lines, dense controls.
    pub small: TextToken,
    /// Default text and controls.
    pub body: TextToken,
    /// Large controls, emphasised lines.
    pub large: TextToken,
    /// Section and dialog titles.
    pub title: TextToken,
    /// Page titles.
    pub display: TextToken,
    /// Weight for emphasis within a size.
    pub emphasis: FontWeight,
}

/// Fixed dimensions of controls.
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub control_sm: Pixels,
    pub control_md: Pixels,
    pub control_lg: Pixels,
    /// Border and divider thickness.
    pub border: Pixels,
    /// Thickness of the focus ring drawn outside a control.
    pub focus_ring: Pixels,
    /// Stroke of drawn glyphs such as the check mark.
    pub stroke: Pixels,
    pub checkbox: Pixels,
    pub switch_width: Pixels,
    pub switch_height: Pixels,
    pub switch_thumb: Pixels,
    pub avatar_sm: Pixels,
    pub avatar_md: Pixels,
    pub avatar_lg: Pixels,
    pub badge: Pixels,
    /// The strip at the top of a window that holds the window buttons and drags the window.
    pub titlebar: Pixels,
    /// Where the window buttons sit inside that strip.
    pub window_buttons: Point<Pixels>,
    /// The conversation list beside the open conversation.
    pub sidebar: Pixels,
    /// How far past its edges a long list lays rows out, so scrolling never shows a gap.
    pub list_overdraw: Pixels,
    /// The Chrome window Google's sign-in page opens in.
    pub sign_in_window: Size<Pixels>,
    /// A window when it first opens.
    pub window: Size<Pixels>,
    /// The smallest a window can be made.
    pub window_min: Size<Pixels>,
}

/// Shadows that lift a surface off what is behind it.
#[derive(Clone, Debug)]
pub struct Shadows {
    /// Cards, the selected segment, switch thumbs.
    pub sm: Vec<BoxShadow>,
    /// Popovers and menus.
    pub md: Vec<BoxShadow>,
    /// Dialogs.
    pub lg: Vec<BoxShadow>,
}

/// Durations and easing of transitions.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// State changes inside a control.
    pub fast: Duration,
    /// Popovers and menus appearing.
    pub normal: Duration,
    /// Dialogs and large surfaces appearing.
    pub slow: Duration,
    pub easing: fn(f32) -> f32,
}

impl Radius {
    pub fn standard() -> Self {
        Self {
            xs: px(4.),
            sm: px(6.),
            md: px(8.),
            lg: px(12.),
            xl: px(16.),
            full: px(9999.),
        }
    }
}

impl Spacing {
    pub fn standard() -> Self {
        Self {
            xxs: px(2.),
            xs: px(4.),
            sm: px(8.),
            md: px(12.),
            lg: px(16.),
            xl: px(24.),
            xxl: px(32.),
        }
    }
}

impl Typography {
    pub fn standard() -> Self {
        let step = |size, line_height, weight| TextToken {
            size: px(size),
            line_height: px(line_height),
            weight,
        };
        Self {
            family: ".SystemUIFont".into(),
            caption: step(11., 14., FontWeight::NORMAL),
            small: step(12., 16., FontWeight::NORMAL),
            body: step(13., 18., FontWeight::NORMAL),
            large: step(15., 20., FontWeight::NORMAL),
            title: step(20., 26., FontWeight::MEDIUM),
            display: step(28., 34., FontWeight::SEMIBOLD),
            emphasis: FontWeight::MEDIUM,
        }
    }
}

impl Sizes {
    pub fn standard() -> Self {
        Self {
            control_sm: px(24.),
            control_md: px(30.),
            control_lg: px(36.),
            border: px(1.),
            focus_ring: px(2.),
            stroke: px(1.5),
            checkbox: px(16.),
            switch_width: px(32.),
            switch_height: px(18.),
            switch_thumb: px(14.),
            avatar_sm: px(24.),
            avatar_md: px(32.),
            avatar_lg: px(40.),
            badge: px(18.),
            titlebar: px(38.),
            window_buttons: point(px(13.), px(12.)),
            sidebar: px(280.),
            list_overdraw: px(600.),
            sign_in_window: size(px(480.), px(700.)),
            window: size(px(1040.), px(720.)),
            window_min: size(px(720.), px(480.)),
        }
    }
}

impl Shadows {
    pub fn light() -> Self {
        Self {
            sm: vec![drop(1., 2., 0., 0x0000000d)],
            md: vec![drop(4., 6., -1., 0x0000001a), drop(2., 4., -2., 0x0000001a)],
            lg: vec![
                drop(20., 25., -5., 0x0000001a),
                drop(8., 10., -6., 0x0000001a),
            ],
        }
    }

    pub fn dark() -> Self {
        Self {
            sm: vec![drop(1., 2., 0., 0x0000004d)],
            md: vec![drop(4., 6., -1., 0x00000066), drop(2., 4., -2., 0x00000066)],
            lg: vec![
                drop(20., 25., -5., 0x00000080),
                drop(8., 10., -6., 0x00000080),
            ],
        }
    }
}

impl Motion {
    pub fn standard() -> Self {
        Self {
            fast: Duration::from_millis(100),
            normal: Duration::from_millis(180),
            slow: Duration::from_millis(280),
            easing: gpui::ease_in_out,
        }
    }
}

fn drop(offset_y: f32, blur: f32, spread: f32, color: u32) -> BoxShadow {
    BoxShadow {
        color: Hsla::from(rgba(color)),
        offset: point(px(0.), px(offset_y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
    }
}
