use gpui::{App, KeyBinding, NoAction, actions};

actions!(
    ui,
    [
        /// Accept the current choice.
        Confirm,
        /// Dismiss or back out.
        Cancel,
        /// Move to the next item.
        Next,
        /// Move to the previous item.
        Previous,
        /// Move keyboard focus to the next control.
        FocusNext,
        /// Move keyboard focus to the previous control.
        FocusPrevious,
    ]
);

/// The key context of a control that Enter presses, such as a button.
pub(super) const PRESSABLE: &str = "Pressable";

/// Binds the default keys. Bindings added later take precedence, so call this first.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, None),
        // Enter presses the focused control as it is released, and so confirms nothing else.
        KeyBinding::new("enter", NoAction, Some(PRESSABLE)),
        KeyBinding::new("escape", Cancel, None),
        KeyBinding::new("down", Next, None),
        KeyBinding::new("up", Previous, None),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
        KeyBinding::new("right", Next, Some("SegmentedControl")),
        KeyBinding::new("left", Previous, Some("SegmentedControl")),
    ]);
}
