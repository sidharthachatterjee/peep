//! Banners from macOS when a message arrives, grouped by conversation.

use std::sync::mpsc::{Sender, channel};

use block2::RcBlock;
use futures::channel::mpsc::UnboundedSender;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{Bool, ProtocolObject},
};
use objc2_app_kit::NSSound;
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

/// The sound a message arrives with: a file in the app's resources, by its name there and
/// by the name it is played under.
const SOUND_FILE: &str = "Peep.caf";
const SOUND: &str = "Peep";

/// One banner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// The message it is about. A later notice with the same id takes this one's place.
    pub id: String,
    /// The conversation it is in. macOS stacks the banners of one conversation together.
    pub conversation: String,
    pub title: String,
    /// A line under the title, or nothing.
    pub subtitle: String,
    pub body: String,
    /// Whether it arrives with the sound.
    pub sound: bool,
}

enum Command {
    Show(Notice),
    /// Takes down the banners with these ids.
    Clear(Vec<String>),
}

/// Peep's banners. macOS asks the person once whether Peep may show them.
pub struct Notifications {
    commands: Sender<Command>,
    /// The notification centre does not keep its delegate alive.
    _delegate: Retained<Delegate>,
}

impl Notifications {
    /// Starts showing banners. The conversation of a banner that is clicked is sent to
    /// `opened`. `None` when Peep is not running from its app bundle, since macOS lets
    /// only bundled apps show banners.
    pub fn new(opened: UnboundedSender<String>) -> Option<Self> {
        NSBundle::mainBundle().bundleIdentifier()?;
        let delegate = Delegate::new(opened);
        UNUserNotificationCenter::currentNotificationCenter()
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        // Calls to the notification centre can wait on a system service, so they are made
        // off the main thread, one after another.
        let (commands, received) = channel();
        let worker = std::thread::Builder::new().name("notifications".into());
        let started = worker.spawn(move || {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            ask_leave(&center);
            for command in received {
                autoreleasepool(|_| match command {
                    Command::Show(notice) => show(&center, &notice),
                    Command::Clear(ids) => {
                        let ids: Vec<_> = ids.iter().map(|id| NSString::from_str(id)).collect();
                        let ids = NSArray::from_retained_slice(&ids);
                        center.removeDeliveredNotificationsWithIdentifiers(&ids);
                    }
                });
            }
        });
        started.ok()?;
        Some(Self {
            commands,
            _delegate: delegate,
        })
    }

    pub fn show(&self, notice: Notice) {
        // The worker lives as long as this does.
        self.commands.send(Command::Show(notice)).ok();
    }

    /// Takes down the banners with these ids, as when their conversation has been read.
    pub fn clear(&self, ids: Vec<String>) {
        self.commands.send(Command::Clear(ids)).ok();
    }
}

/// Plays the sound a message arrives with, without a banner. Does nothing off the main
/// thread or when the sound is not in the app's resources.
pub fn chirp() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    if let Some(sound) = NSSound::soundNamed(&NSString::from_str(SOUND)) {
        // A sound still playing from the message before starts over.
        sound.stop();
        sound.play();
    }
}

/// Asks the person, the first time, whether Peep may show banners.
fn ask_leave(center: &UNUserNotificationCenter) {
    let answered = RcBlock::new(|_granted: Bool, error: *mut NSError| {
        // SAFETY: when it is not null, `error` is a valid error for as long as this runs.
        if let Some(error) = unsafe { error.as_ref() } {
            eprintln!(
                "Peep may not show notifications: {}",
                error.localizedDescription()
            );
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &answered,
    );
}

fn show(center: &UNUserNotificationCenter, notice: &Notice) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&notice.title));
    content.setSubtitle(&NSString::from_str(&notice.subtitle));
    content.setBody(&NSString::from_str(&notice.body));
    content.setThreadIdentifier(&NSString::from_str(&notice.conversation));
    if notice.sound {
        // macOS plays its own sound instead if this one is missing.
        let sound = UNNotificationSound::soundNamed(&NSString::from_str(SOUND_FILE));
        content.setSound(Some(&sound));
    }
    // No trigger shows it at once.
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&notice.id),
        &content,
        None,
    );
    let shown = RcBlock::new(|error: *mut NSError| {
        // SAFETY: when it is not null, `error` is a valid error for as long as this runs.
        if let Some(error) = unsafe { error.as_ref() } {
            eprintln!(
                "Peep could not show a notification: {}",
                error.localizedDescription()
            );
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&shown));
}

struct Opened(UnboundedSender<String>);

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and `Delegate` has no `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "PeepNotificationDelegate"]
    #[ivars = Opened]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// A banner was clicked. macOS may call this off the main thread, so the
        /// conversation is passed on rather than opened here.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn opened(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &block2::DynBlock<dyn Fn()>,
        ) {
            let content = response.notification().request().content();
            let conversation = content.threadIdentifier().to_string();
            // Nothing listens once Peep is quitting.
            self.ivars().0.unbounded_send(conversation).ok();
            done.call(());
        }

        /// macOS asks how to show a banner while Peep is in front: without this it shows
        /// none. Whether to show one is decided before it is asked for.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn presenting(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            done: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            done.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }
    }
);

impl Delegate {
    fn new(opened: UnboundedSender<String>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Opened(opened));
        // SAFETY: `NSObject`'s `init` takes no arguments and returns the object.
        unsafe { msg_send![super(this), init] }
    }
}
