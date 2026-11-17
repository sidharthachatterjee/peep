//! Helpers for driving components the way a person would.

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, KeyUpEvent, Keystroke, Modifiers,
    Render, TestAppContext, VisualTestContext, Window,
};

use super::Root;

/// Opens a window showing `build`'s view inside a [`Root`], with the default key bindings.
pub(super) fn open<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> (Entity<V>, &mut VisualTestContext) {
    cx.update(super::init);
    let mut view = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let built = cx.new(|cx| build(window, cx));
        view = Some(built.clone());
        Root::new(built, window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (view.unwrap(), cx)
}

/// Clicks the centre of the element tagged with `.debug_selector`.
pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing is tagged {selector:?}"));
    cx.simulate_click(bounds.center(), Modifiers::none());
}

/// Presses and releases keys. `simulate_keystrokes` alone only presses them.
pub(super) fn press(cx: &mut VisualTestContext, keys: &str) {
    for key in keys.split(' ') {
        cx.simulate_keystrokes(key);
        cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse(key).unwrap(),
        });
    }
}

pub(super) fn focused(cx: &mut VisualTestContext) -> Option<FocusHandle> {
    cx.update(|window, cx| window.focused(cx))
}

/// Whether keyboard focus is on the window's [`Root`] rather than on a control.
pub(super) fn focus_is_on_root(cx: &mut VisualTestContext) -> bool {
    let root = cx.update(|window, cx| {
        window
            .root::<Root>()
            .flatten()
            .map(|root| root.focus_handle(cx))
    });
    focused(cx) == root
}
