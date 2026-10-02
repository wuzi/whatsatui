use super::{Notifier, Popup, plain};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use windows::{
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::HSTRING,
};

const APP_ID: &str = "whatsapp-tui";

#[derive(Clone, Default)]
pub struct NativeNotifier {
    registered: Arc<OnceLock<()>>,
}

#[async_trait::async_trait]
impl Notifier for NativeNotifier {
    async fn show(&self, popup: &Popup) -> Result<(), String> {
        let title = plain(&popup.title, 96);
        let body = popup
            .body
            .lines()
            .take(4)
            .map(|line| plain(line, 240))
            .collect::<Vec<_>>()
            .join("\n");
        let registered = self.registered.clone();
        tokio::time::timeout(
            Duration::from_secs(3),
            tokio::task::spawn_blocking(move || {
                send(&title, &body, &registered).map_err(|_| {
                    "Desktop notifications unavailable; retrying after 60 seconds".to_owned()
                })
            }),
        )
        .await
        .map_err(|_| "Desktop notification request timed out; retrying after 60 seconds")?
        .map_err(|_| "Desktop notification worker stopped")?
    }
}

fn send(title: &str, body: &str, registered: &OnceLock<()>) -> windows::core::Result<()> {
    // Register the unpackaged application for Windows 10/11 toast attribution.
    if registered.get().is_none() {
        let key = windows_registry::CURRENT_USER
            .create(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))?;
        key.set_string("DisplayName", "WhatsAppTUI")?;
        let _ = registered.set(());
    }
    // SAFETY: This blocking worker initializes its own apartment; the guard
    // balances successful initialization after the last WinRT object is dropped.
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED)?;
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                RoUninitialize();
            }
        }
    }
    let _apartment = Apartment;
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual><audio silent=\"true\"/></toast>",
        escape(title), escape(body),
    )))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
}
