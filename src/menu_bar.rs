//! Peep's icon in the menu bar, with the count of what is unread beside it.

use std::cell::Cell;

use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_app_kit::{
    NSCellImagePosition, NSFont, NSFontWeightMedium, NSImage, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString};

/// The icon: Peep's ghost as a shape, which the menu bar draws in its own black or white.
const ICON: &[u8] = include_bytes!("../assets/menu-bar/ghost.svg");
/// How large the menu bar's own icons are, in points.
const ICON_SIZE: f64 = 18.;
/// The size of the count, a little smaller than the menu bar's text.
const COUNT_SIZE: f64 = 12.;
/// The largest count shown as a number.
const MOST_SHOWN: u32 = 99;

/// The count as shown beside the icon: nothing when nothing is unread.
fn label(unread: u32) -> String {
    match unread {
        0 => String::new(),
        unread if unread > MOST_SHOWN => format!("{MOST_SHOWN}+"),
        unread => unread.to_string(),
    }
}

/// Peep's item in the menu bar. It is there for as long as this is kept.
pub struct MenuBarItem {
    item: Retained<NSStatusItem>,
    /// What is called when the item is clicked. The item does not keep it alive itself.
    _target: Retained<Target>,
    unread: Cell<u32>,
    main_thread: MainThreadMarker,
}

impl MenuBarItem {
    /// Adds the item to the menu bar. `on_click` runs when it is clicked. `None` off the
    /// main thread, where the menu bar cannot be changed.
    pub fn new(on_click: impl Fn() + 'static) -> Option<Self> {
        let main_thread = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let target = Target::new(main_thread, Box::new(on_click));

        if let Some(button) = item.button(main_thread) {
            let icon = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(ICON));
            if let Some(icon) = &icon {
                icon.setSize(NSSize::new(ICON_SIZE, ICON_SIZE));
                // Drawn as a shape in the menu bar's colour, light or dark.
                icon.setTemplate(true);
            }
            button.setImage(icon.as_deref());
            button.setImagePosition(NSCellImagePosition::ImageLeading);
            // SAFETY: the weight is one of AppKit's own constants.
            let font = unsafe {
                NSFont::monospacedDigitSystemFontOfSize_weight(COUNT_SIZE, NSFontWeightMedium)
            };
            button.setFont(Some(&font));
            // SAFETY: the target answers `clicked:`, and this item keeps it alive for as
            // long as the button can send it.
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(clicked:)));
            }
        }
        Some(Self {
            item,
            _target: target,
            unread: Cell::new(0),
            main_thread,
        })
    }

    /// Shows how much is unread beside the icon.
    pub fn set_unread(&self, unread: u32) {
        if self.unread.replace(unread) == unread {
            return;
        }
        if let Some(button) = self.item.button(self.main_thread) {
            button.setTitle(&NSString::from_str(&label(unread)));
        }
    }
}

impl Drop for MenuBarItem {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

struct Clicked(Box<dyn Fn()>);

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and `Target` has no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PeepMenuBarTarget"]
    #[ivars = Clicked]
    struct Target;

    impl Target {
        #[unsafe(method(clicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            (self.ivars().0)();
        }
    }
);

impl Target {
    fn new(main_thread: MainThreadMarker, on_click: Box<dyn Fn()>) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(Clicked(on_click));
        // SAFETY: `NSObject`'s `init` takes no arguments and returns the object.
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_is_shown_only_when_something_is_unread() {
        assert_eq!(label(0), "");
        assert_eq!(label(1), "1");
        assert_eq!(label(99), "99");
        assert_eq!(label(100), "99+");
    }

    #[test]
    fn the_icon_is_a_shape_with_no_colour_of_its_own() {
        let icon = std::str::from_utf8(ICON).unwrap();
        assert!(icon.starts_with("<svg"));
        assert!(!icon.contains("fill=\"#") && !icon.contains("stroke="));
    }
}
