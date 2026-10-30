//! Native macOS notifications via UNUserNotificationCenter.
//!
//! The notification plugin uses the deprecated NSUserNotificationCenter, which recent
//! macOS versions silently ignore. This also lets banners show while the app is in the
//! foreground and opens the message's link when a notification is clicked.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_foundation::{ns_string, NSBundle, NSDictionary, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSettings, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "NtfyUiNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Show banners even when ntfyui is the frontmost app.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        /// Clicking a notification opens its link, or the app window if it has none.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let info = response.notification().request().content().userInfo();
            let url = info
                .objectForKey(ns_string!("url"))
                .and_then(|o| o.downcast::<NSString>().ok())
                .map(|s| s.to_string());
            if let Some(app) = APP.get() {
                crate::on_notification_click(app, url);
            }
            handler.call(());
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

/// UNUserNotificationCenter throws if the process isn't a bundled app (e.g. `tauri dev`).
pub fn is_supported() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
        && NSBundle::mainBundle().bundlePath().to_string().ends_with(".app")
}

pub fn init(app: &AppHandle) {
    if !is_supported() {
        return;
    }
    let _ = APP.set(app.clone());
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let delegate = Delegate::new();
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center only keeps a weak reference; keep the delegate alive for the app's lifetime.
    std::mem::forget(delegate);

    let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
    let done = RcBlock::new(|granted: Bool, err: *mut NSError| {
        if !granted.as_bool() {
            let msg = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string());
            eprintln!("notification permission not granted: {msg:?}");
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(options, &done);
}

pub fn show(id: &str, title: &str, body: &str, url: Option<&str>) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    if let Some(url) = url {
        let value = NSString::from_str(url);
        let dict = NSDictionary::<NSString, NSString>::from_slices(&[ns_string!("url")], &[&*value]);
        let dict: Retained<NSDictionary> = unsafe { Retained::cast_unchecked(dict) };
        unsafe { content.setUserInfo(&dict) };
    }
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(id),
        &content,
        None,
    );
    let done = RcBlock::new(|err: *mut NSError| {
        if let Some(e) = unsafe { err.as_ref() } {
            eprintln!("failed to show notification: {}", e.localizedDescription());
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&done));
}

/// "authorized", "denied", "not_determined" or "unknown".
pub fn permission() -> &'static str {
    if !is_supported() {
        return "unknown";
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        let _ = tx.send(status);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .getNotificationSettingsWithCompletionHandler(&block);
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(UNAuthorizationStatus::Denied) => "denied",
        Ok(UNAuthorizationStatus::NotDetermined) => "not_determined",
        Ok(_) => "authorized",
        Err(_) => "unknown",
    }
}
