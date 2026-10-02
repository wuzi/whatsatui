use super::{Notifier, Popup, plain};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use zbus::{Connection, zvariant::Value};

/// One connection per running TUI, shared by its notification effects.
/// GNOME groups unidentified applications by sender PID and application name;
/// a fresh notify-send process for each popup gives each one a separate group.
#[derive(Clone, Default)]
pub struct NativeNotifier {
    connection: Arc<Mutex<Option<Connection>>>,
}

#[async_trait::async_trait]
impl Notifier for NativeNotifier {
    async fn show(&self, popup: &Popup) -> Result<(), String> {
        self.send(popup, Duration::from_secs(3)).await
    }
}

impl NativeNotifier {
    async fn send(&self, popup: &Popup, deadline: Duration) -> Result<(), String> {
        let result = tokio::time::timeout(deadline, async {
            let connection = {
                let mut cached = self.connection.lock().await;
                if cached.as_ref().is_none_or(Connection::is_closed) {
                    *cached = Some(Connection::session().await?);
                }
                cached.as_ref().expect("connected notification bus").clone()
            };
            let (title, body) = content(popup);
            let hints = HashMap::from([
                ("category", Value::from("im.received")),
                ("urgency", Value::from(1u8)),
                ("suppress-sound", Value::from(true)),
            ]);
            let reply = connection
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &(
                        "whatsapp-tui",
                        0u32,
                        "mail-message-new",
                        title,
                        body,
                        Vec::<&str>::new(),
                        hints,
                        7000i32,
                    ),
                )
                .await?;
            reply.body().deserialize::<u32>()?;
            Ok::<_, zbus::Error>(())
        })
        .await;
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err("Desktop notifications unavailable: check your desktop session; retrying after 60 seconds".into()),
            Err(_) => Err("Desktop notification request timed out; retrying after 60 seconds".into()),
        }
    }
}

fn content(popup: &Popup) -> (String, String) {
    let body = popup
        .body
        .lines()
        .take(4)
        .map(|line| {
            plain(line, 240)
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        })
        .collect::<Vec<_>>()
        .join("\n");
    (plain(&popup.title, 96), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_is_literal_bounded_and_markup_escaped() {
        let (title, body) = content(&Popup {
            title: "--malicious\u{1b}title".into(),
            body: "<img src='secret'/> & $(echo bad)\u{0}\nhello".into(),
        });
        assert_eq!(title, "--malicioustitle");
        assert_eq!(body, "&lt;img src='secret'/&gt; &amp; $(echo bad)\nhello");
        let (title, body) = content(&Popup {
            title: "x".repeat(100_000),
            body: vec!["👩‍💻".repeat(100_000); 6].join("\n"),
        });
        assert!(title.len() < 400);
        assert_eq!(body.lines().count(), 4);
        assert!(body.len() < 16_384);
    }

    #[tokio::test]
    #[ignore = "requires the isolated synthetic notification D-Bus server"]
    async fn native_notifier_private_bus_smoke() {
        assert_eq!(
            std::env::var("WHATSAPP_TUI_PRIVATE_NOTIFICATION_BUS").as_deref(),
            Ok("1"),
            "Run only with tests/notification_bus.py, never on the real desktop"
        );
        let notifier = NativeNotifier::default();
        for title in ["Synthetic notification", "Another conversation"] {
            // Effects hold temporary clones; the runtime retains the owner.
            super::super::deliver(
                &Popup {
                    title: title.into(),
                    body: "<tag> & literal".into(),
                },
                &notifier.clone(),
            )
            .await
            .unwrap();
        }
        let popup = |title: &str| Popup {
            title: title.into(),
            body: "Synthetic lifecycle check".into(),
        };
        assert!(
            notifier
                .show(&popup("Failure"))
                .await
                .unwrap_err()
                .contains("desktop session")
        );
        assert!(
            notifier
                .send(&popup("Timeout"), Duration::from_millis(50))
                .await
                .unwrap_err()
                .contains("timed out")
        );
        notifier.show(&popup("After timeout")).await.unwrap();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                notifier.show(&popup("Cancelled")),
            )
            .await
            .is_err()
        );
        notifier.show(&popup("After cancellation")).await.unwrap();
        // A broken session must reconnect on the next new popup, without
        // retrying a notification that might already have been displayed.
        let closed = notifier.connection.lock().await.as_ref().unwrap().clone();
        closed.close().await.unwrap();
        notifier.show(&popup("Reconnected")).await.unwrap();
    }
}
