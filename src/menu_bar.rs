//! Peep's icon in the menu bar. The icon shows what the signed-in person is showing others
//! and whether notifications are paused, the count beside it shows how much is unread, and
//! clicking it opens a menu.

use std::{cell::RefCell, time::Duration};

use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, runtime::NSObject, sel,
};
use objc2_app_kit::{
    NSCellImagePosition, NSControlStateValueOn, NSFont, NSFontWeightMedium, NSImage, NSMenu,
    NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString};

use crate::chat::Showing;

/// Peep's ghost as a shape, which the menu bar draws in its own black or white: whole when
/// the person shows as they are, with a badge for do not disturb, an outline for away, and
/// struck through while notifications are paused.
const ICONS: [[&[u8]; 2]; 3] = [
    [
        include_bytes!("../assets/menu-bar/automatic.svg"),
        include_bytes!("../assets/menu-bar/automatic-paused.svg"),
    ],
    [
        include_bytes!("../assets/menu-bar/dnd.svg"),
        include_bytes!("../assets/menu-bar/dnd-paused.svg"),
    ],
    [
        include_bytes!("../assets/menu-bar/away.svg"),
        include_bytes!("../assets/menu-bar/away-paused.svg"),
    ],
];
/// How large the menu bar's own icons are, in points.
const ICON_SIZE: f64 = 18.;
/// The size of the count, a little smaller than the menu bar's text.
const COUNT_SIZE: f64 = 12.;
/// The largest count shown as a number.
const MOST_SHOWN: u32 = 99;
/// How long do not disturb can be turned on for, as the menu words it.
const QUIET_FOR: [(&str, u64); 4] = [
    ("For 30 Minutes", 30),
    ("For 1 Hour", 60),
    ("For 2 Hours", 120),
    ("For 4 Hours", 240),
];

/// What the item shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Content {
    /// What the signed-in person is showing others.
    pub showing: Showing,
    /// Whether notifications are paused.
    pub paused: bool,
    /// How much is unread.
    pub unread: u32,
    /// Conversations with something unread, as an id and a name, newest first.
    pub conversations: Vec<(String, String)>,
}

/// What was chosen from the menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Show(Choice),
    /// Pause notifications, or stop pausing them.
    Pause(bool),
    /// Open the conversation with this id.
    OpenConversation(String),
    Quit,
}

/// What to show others from now on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Choice {
    Automatic,
    DoNotDisturbFor(Duration),
    DoNotDisturbUntilTomorrow,
    Away,
}

/// One line of the menu.
#[derive(Clone, Debug, PartialEq)]
enum Line {
    Heading(&'static str),
    Separator,
    Item {
        title: String,
        ticked: bool,
        command: Command,
    },
    Submenu {
        title: &'static str,
        ticked: bool,
        lines: Vec<Line>,
    },
}

/// The menu for `content`.
fn lines(content: &Content) -> Vec<Line> {
    let item = |title: &str, ticked: bool, command: Command| Line::Item {
        title: title.to_owned(),
        ticked,
        command,
    };
    let quiet = matches!(content.showing, Showing::DoNotDisturb { .. });
    let mut quiet_for: Vec<Line> = QUIET_FOR
        .iter()
        .map(|(title, minutes)| {
            let choice = Choice::DoNotDisturbFor(Duration::from_secs(minutes * 60));
            item(title, false, Command::Show(choice))
        })
        .collect();
    quiet_for.push(item(
        "Until Tomorrow",
        false,
        Command::Show(Choice::DoNotDisturbUntilTomorrow),
    ));

    let mut lines = vec![
        Line::Heading("Status"),
        item(
            "Automatic",
            content.showing == Showing::Automatic,
            Command::Show(Choice::Automatic),
        ),
        Line::Submenu {
            title: "Do Not Disturb",
            ticked: quiet,
            lines: quiet_for,
        },
        item(
            "Away",
            content.showing == Showing::Away,
            Command::Show(Choice::Away),
        ),
        Line::Separator,
        item(
            "Pause Notifications",
            content.paused,
            Command::Pause(!content.paused),
        ),
        Line::Separator,
    ];
    if !content.conversations.is_empty() {
        lines.push(Line::Heading("Unread"));
        for (id, name) in &content.conversations {
            lines.push(item(name, false, Command::OpenConversation(id.clone())));
        }
        lines.push(Line::Separator);
    }
    lines.push(item("Quit", false, Command::Quit));
    lines
}

/// The count as shown beside the icon: nothing when nothing is unread.
fn label(unread: u32) -> String {
    match unread {
        0 => String::new(),
        unread if unread > MOST_SHOWN => format!("{MOST_SHOWN}+"),
        unread => unread.to_string(),
    }
}

/// The icon for what the person is showing, struck through while notifications are paused.
fn icon(showing: Showing, paused: bool) -> &'static [u8] {
    let shape = match showing {
        Showing::Automatic => 0,
        Showing::DoNotDisturb { .. } => 1,
        Showing::Away => 2,
    };
    ICONS[shape][usize::from(paused)]
}

/// Peep's item in the menu bar. It is there for as long as this is kept.
pub struct MenuBarItem {
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    /// Receives what is chosen. Menu items do not keep it alive themselves.
    target: Retained<Target>,
    /// What is shown now, so only what changes is redrawn.
    content: RefCell<Option<Content>>,
    main_thread: MainThreadMarker,
}

impl MenuBarItem {
    /// Adds the item to the menu bar. `on_command` runs with what is chosen from its menu.
    /// `None` off the main thread, where the menu bar cannot be changed.
    pub fn new(on_command: impl Fn(Command) + 'static) -> Option<Self> {
        let main_thread = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let menu = NSMenu::new(main_thread);
        // Every line says for itself whether it can be chosen.
        menu.setAutoenablesItems(false);
        item.setMenu(Some(&menu));
        if let Some(button) = item.button(main_thread) {
            button.setImagePosition(NSCellImagePosition::ImageLeading);
            // SAFETY: the weight is one of AppKit's own constants.
            let font = unsafe {
                NSFont::monospacedDigitSystemFontOfSize_weight(COUNT_SIZE, NSFontWeightMedium)
            };
            button.setFont(Some(&font));
        }
        let this = Self {
            item,
            menu,
            target: Target::new(main_thread, Box::new(on_command)),
            content: RefCell::new(None),
            main_thread,
        };
        this.show(Content::default());
        Some(this)
    }

    /// Shows `content`. Only what differs from what is showing is redrawn.
    pub fn show(&self, content: Content) {
        let before = self.content.replace(Some(content.clone()));
        if before.as_ref() == Some(&content) {
            return;
        }
        let was = |same: fn(&Content, &Content) -> bool| {
            before.as_ref().is_some_and(|before| same(before, &content))
        };
        if let Some(button) = self.item.button(self.main_thread) {
            if !was(|a, b| icon(a.showing, a.paused) == icon(b.showing, b.paused)) {
                let data = NSData::with_bytes(icon(content.showing, content.paused));
                let image = NSImage::initWithData(NSImage::alloc(), &data);
                if let Some(image) = &image {
                    image.setSize(NSSize::new(ICON_SIZE, ICON_SIZE));
                    // Drawn as a shape in the menu bar's colour, light or dark.
                    image.setTemplate(true);
                }
                button.setImage(image.as_deref());
            }
            if !was(|a, b| a.unread == b.unread) {
                button.setTitle(&NSString::from_str(&label(content.unread)));
            }
        }
        let lines = lines(&content);
        if !before.is_some_and(|before| self::lines(&before) == lines) {
            self.menu.removeAllItems();
            let mut commands = Vec::new();
            self.fill(&self.menu, &lines, &mut commands);
            *self.target.ivars().commands.borrow_mut() = commands;
        }
    }

    /// Adds `lines` to `menu`. Each item is tagged with where its command is in `commands`.
    fn fill(&self, menu: &NSMenu, lines: &[Line], commands: &mut Vec<Command>) {
        let main_thread = self.main_thread;
        let titled = |title: &str, ticked: bool| {
            let item = NSMenuItem::new(main_thread);
            item.setTitle(&NSString::from_str(title));
            if ticked {
                item.setState(NSControlStateValueOn);
            }
            item
        };
        for line in lines {
            let item = match line {
                Line::Heading(title) => {
                    NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), main_thread)
                }
                Line::Separator => NSMenuItem::separatorItem(main_thread),
                Line::Item {
                    title,
                    ticked,
                    command,
                } => {
                    let item = titled(title, *ticked);
                    item.setTag(commands.len() as isize);
                    commands.push(command.clone());
                    // SAFETY: the target answers `chosen:`, and this item keeps it alive for
                    // as long as the menu can send it.
                    unsafe {
                        item.setTarget(Some(&self.target));
                        item.setAction(Some(sel!(chosen:)));
                    }
                    item
                }
                Line::Submenu {
                    title,
                    ticked,
                    lines,
                } => {
                    let item = titled(title, *ticked);
                    let submenu = NSMenu::new(main_thread);
                    submenu.setAutoenablesItems(false);
                    self.fill(&submenu, lines, commands);
                    item.setSubmenu(Some(&submenu));
                    item
                }
            };
            menu.addItem(&item);
        }
    }
}

impl Drop for MenuBarItem {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

struct Chosen {
    on_command: Box<dyn Fn(Command)>,
    /// The command of each menu item, by the item's tag.
    commands: RefCell<Vec<Command>>,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and `Target` has no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PeepMenuBarTarget"]
    #[ivars = Chosen]
    struct Target;

    impl Target {
        #[unsafe(method(chosen:))]
        fn chosen(&self, item: &NSMenuItem) {
            let chosen = self.ivars();
            let command = usize::try_from(item.tag())
                .ok()
                .and_then(|tag| chosen.commands.borrow().get(tag).cloned());
            if let Some(command) = command {
                (chosen.on_command)(command);
            }
        }
    }
);

impl Target {
    fn new(main_thread: MainThreadMarker, on_command: Box<dyn Fn(Command)>) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(Chosen {
            on_command,
            commands: RefCell::new(Vec::new()),
        });
        // SAFETY: `NSObject`'s `init` takes no arguments and returns the object.
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| match line {
                Line::Heading(title) => format!("[{title}]"),
                Line::Separator => "-".into(),
                Line::Item { title, ticked, .. } => format!("{}{title}", tick(*ticked)),
                Line::Submenu { title, ticked, .. } => format!("{}{title} >", tick(*ticked)),
            })
            .collect()
    }

    fn tick(ticked: bool) -> &'static str {
        if ticked { "✓ " } else { "" }
    }

    #[test]
    fn the_count_is_shown_only_when_something_is_unread() {
        assert_eq!(label(0), "");
        assert_eq!(label(1), "1");
        assert_eq!(label(99), "99");
        assert_eq!(label(100), "99+");
    }

    #[test]
    fn the_menu_ticks_what_is_in_effect_and_lists_what_is_unread() {
        assert_eq!(
            titles(&lines(&Content::default())),
            [
                "[Status]",
                "✓ Automatic",
                "Do Not Disturb >",
                "Away",
                "-",
                "Pause Notifications",
                "-",
                "Quit"
            ]
        );

        let content = Content {
            showing: Showing::DoNotDisturb { until: 1 },
            paused: true,
            unread: 2,
            conversations: vec![
                ("dm/a".into(), "Ada".into()),
                ("dm/g".into(), "Grace".into()),
            ],
        };
        let lines = lines(&content);
        assert_eq!(
            titles(&lines),
            [
                "[Status]",
                "Automatic",
                "✓ Do Not Disturb >",
                "Away",
                "-",
                "✓ Pause Notifications",
                "-",
                "[Unread]",
                "Ada",
                "Grace",
                "-",
                "Quit"
            ]
        );
        assert!(lines.contains(&Line::Item {
            title: "Pause Notifications".into(),
            ticked: true,
            command: Command::Pause(false),
        }));
        assert!(lines.contains(&Line::Item {
            title: "Grace".into(),
            ticked: false,
            command: Command::OpenConversation("dm/g".into()),
        }));
        let Line::Submenu { lines: quiet, .. } = &lines[2] else {
            panic!("do not disturb opens a submenu");
        };
        assert_eq!(
            titles(quiet),
            [
                "For 30 Minutes",
                "For 1 Hour",
                "For 2 Hours",
                "For 4 Hours",
                "Until Tomorrow"
            ]
        );
    }

    #[test]
    fn the_icon_has_a_shape_for_each_state_and_no_colour_of_its_own() {
        let quiet = Showing::DoNotDisturb { until: 1 };
        let states = [
            icon(Showing::Automatic, false),
            icon(Showing::Automatic, true),
            icon(quiet, false),
            icon(quiet, true),
            icon(Showing::Away, false),
            icon(Showing::Away, true),
        ];
        for (index, state) in states.iter().enumerate() {
            let svg = std::str::from_utf8(state).unwrap();
            assert!(svg.starts_with("<svg"));
            assert!(!svg.contains('#') || svg.contains("url(#"), "no colour");
            assert!(
                states[index + 1..].iter().all(|other| other != state),
                "each state looks different"
            );
        }
    }
}
