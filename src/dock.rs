//! Peep's icon in the Dock and the app switcher.

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;

/// Shows how much is unread as a badge on Peep's icon: no badge when nothing is.
/// Does nothing off the main thread, where the icon cannot be changed.
pub fn show_unread(unread: u32) {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let label = label(unread).map(|label| NSString::from_str(&label));
    NSApplication::sharedApplication(main_thread)
        .dockTile()
        .setBadgeLabel(label.as_deref());
}

fn label(unread: u32) -> Option<String> {
    (unread > 0).then(|| unread.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_badge_is_the_count_and_goes_when_nothing_is_unread() {
        assert_eq!(label(0), None);
        assert_eq!(label(1).as_deref(), Some("1"));
        assert_eq!(label(1234).as_deref(), Some("1234"));
    }
}
