//! macOS notifications (User Notifications framework). Clicking one brings
//! Agent Office to the front and opens the agent it is about.
//!
//! Only an app bundle (`Agent Office.app`) may use the framework: a bare
//! binary (`cargo run`, `tauri dev`) has no bundle identifier and the
//! framework would raise an exception. There the notice is only logged.
//! The first notification makes macOS ask the user for permission.

use crate::notify::Notice;
use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

type ClickHandler = Box<dyn FnMut() + Send>;

/// Click handlers of recent notifications, by request identifier (oldest
/// first; only the last [`KEEP`] are kept).
static PENDING: Mutex<Vec<(String, ClickHandler)>> = Mutex::new(Vec::new());
const KEEP: usize = 100;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

const UNKNOWN: u8 = 0;
const GRANTED: u8 = 1;
const DENIED: u8 = 2;
static PERMISSION: AtomicU8 = AtomicU8::new(UNKNOWN);
static PERMISSION_ASKED: AtomicBool = AtomicBool::new(false);
static PERMISSION_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Running as `Something.app` with a bundle identifier.
pub fn in_app_bundle() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}

/// What Diagnostics should say about the permission, if anything.
pub fn permission_note() -> Option<String> {
    if let Some(error) = PERMISSION_ERROR.lock().ok().and_then(|e| e.clone()) {
        return Some(format!("macOS refused notifications: {error}"));
    }
    (PERMISSION.load(Ordering::Relaxed) == DENIED).then(|| {
        "Notifications are turned off for Agent Office in System Settings → Notifications."
            .to_string()
    })
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `Delegate` does
    // not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "AgentOfficeNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // Also shown while Agent Office is in front: the notification rules
        // already decided that this one should be seen.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            // SAFETY: a constant string exported by the framework.
            let default_action = unsafe { UNNotificationDefaultActionIdentifier };
            if response.actionIdentifier().isEqualToString(default_action) {
                clicked(&response.notification().request().identifier().to_string());
            }
            completion.call(());
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

fn clicked(id: &str) {
    let handler = {
        let mut pending = PENDING.lock().expect("notification handlers lock");
        pending
            .iter()
            .position(|(pending_id, _)| pending_id == id)
            .map(|index| pending.remove(index).1)
    };
    if let Some(mut handler) = handler {
        handler();
    }
}

fn error_text(error: *mut NSError) -> Option<String> {
    // SAFETY: the framework passes either null or a valid NSError.
    unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string())
}

/// Registers the click handler; asks for permission when `ask` (otherwise
/// the first notification does). Called once at startup, on the main thread.
pub fn init(ask: bool) {
    if !in_app_bundle() {
        tracing::info!("not running as Agent Office.app: notifications are only logged");
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let delegate = Delegate::new();
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center keeps only a weak reference: the delegate lives as long as
    // the app.
    std::mem::forget(delegate);
    if ask {
        ask_permission();
    }
}

/// macOS asks the user once; later calls return the stored answer.
fn ask_permission() {
    if PERMISSION_ASKED.swap(true, Ordering::Relaxed) {
        return;
    }
    let answered = RcBlock::new(|granted: Bool, error: *mut NSError| {
        if let Some(error) = error_text(error) {
            tracing::warn!(%error, "notification permission request failed");
            if let Ok(mut slot) = PERMISSION_ERROR.lock() {
                *slot = Some(error);
            }
        }
        let state = if granted.as_bool() { GRANTED } else { DENIED };
        PERMISSION.store(state, Ordering::Relaxed);
        tracing::info!(granted = granted.as_bool(), "notification permission");
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &answered,
        );
}

/// Shows `notice`; `on_click` runs when the user clicks it.
pub fn show(notice: &Notice, on_click: impl FnMut() + Send + 'static) -> Result<(), String> {
    if !in_app_bundle() {
        tracing::info!(title = %notice.title, body = %notice.body, "notification (shown only by Agent Office.app)");
        return Ok(());
    }
    ask_permission();
    if PERMISSION.load(Ordering::Relaxed) == DENIED {
        return Err("notifications are turned off for Agent Office in System Settings".into());
    }
    let id = format!("agent-office-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&notice.title));
    content.setBody(&NSString::from_str(&notice.body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    if let Some(agent) = &notice.agent_key {
        // Notification Center groups the notices of one agent.
        content.setThreadIdentifier(&NSString::from_str(agent));
    }
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&id),
        &content,
        None,
    );
    {
        let mut pending = PENDING.lock().expect("notification handlers lock");
        pending.push((id, Box::new(on_click)));
        let excess = pending.len().saturating_sub(KEEP);
        pending.drain(..excess);
    }
    let delivered = RcBlock::new(|error: *mut NSError| {
        if let Some(error) = error_text(error) {
            tracing::warn!(%error, "macOS did not show a notification");
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&delivered));
    Ok(())
}
