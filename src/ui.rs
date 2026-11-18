//! Components, and the types views use to ask for colour and size.
//!
//! Views build from these and from `cx.theme()`. They never name a raw colour or size.

mod actions;
mod avatar;
mod badge;
mod button;
mod label;
mod list;
mod message;
mod root;
mod segmented_control;
mod style;
mod surface;
#[cfg(test)]
mod test;
mod text_input;
mod toggle;

use gpui::App;

pub use actions::{Cancel, Confirm, FocusNext, FocusPrevious, Next, Previous};
pub use avatar::Avatar;
pub use badge::Badge;
pub use button::{Button, ButtonVariant};
pub use label::Label;
pub use list::{List, ListItem};
pub use message::MessageRow;
pub use root::{Root, window_options};
pub use segmented_control::SegmentedControl;
pub use style::{Color, ControlSize, Preview, TextSize, focus_visible};
pub use surface::{Divider, Elevation, Surface};
pub use text_input::{Choice, TextInput, TextInputEvent};
pub use toggle::{Checkbox, Switch};

/// Installs the theme and the default key bindings. Call once, before any other binding.
pub fn init(cx: &mut App) {
    crate::theme::init(cx);
    actions::bind_keys(cx);
    text_input::bind_keys(cx);
}
