#![cfg(all(not(target_os = "macos"), not(windows)))]
//! See <https://developer.gnome.org/notification-spec/>

use crate::ToastNotification;
use futures_util::stream::{abortable, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zbus::proxy;
use zvariant::{Type, Value};

#[derive(Debug, Type, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ServerInformation {
    /// The product name of the server.
    pub name: String,

    /// The vendor name. For example "KDE," "GNOME," "freedesktop.org" or "Microsoft".
    pub vendor: String,

    /// The server's version number.
    pub version: String,

    /// The specification version the server is compliant with.
    pub spec_version: String,
}

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    /// Get server information.
    ///
    /// This message returns the information on the server.
    fn get_server_information(&self) -> zbus::Result<ServerInformation>;

    /// GetCapabilities method
    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

    /// CloseNotification method
    fn close_notification(&self, nid: u32) -> zbus::Result<()>;

    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, nid: u32, action_key: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, nid: u32, reason: u32) -> zbus::Result<()>;
}

/// Timeout/expiration was reached
const REASON_EXPIRED: u32 = 1;
/// User dismissed it
const REASON_USER_DISMISSED: u32 = 2;
/// CloseNotification was called with the nid
const REASON_CLOSE_NOTIFICATION: u32 = 3;

#[derive(Debug)]
enum Reason {
    Expired,
    Dismissed,
    Closed,
    #[allow(dead_code)]
    Unknown(u32),
}

impl Reason {
    fn new(n: u32) -> Self {
        match n {
            REASON_EXPIRED => Self::Expired,
            REASON_USER_DISMISSED => Self::Dismissed,
            REASON_CLOSE_NOTIFICATION => Self::Closed,
            _ => Self::Unknown(n),
        }
    }
}

async fn show_notif_impl(notif: ToastNotification) -> Result<(), Box<dyn std::error::Error>> {
    let connection = zbus::ConnectionBuilder::session()?.build().await?;

    let proxy = NotificationsProxy::new(&connection).await?;
    let caps = proxy.get_capabilities().await?;
    let has_actions = caps.iter().any(|cap| cap == "actions");

    // --- weezterm remote features ---
    // If the server doesn't support actions, still show the notification
    // (just without the clickable button) instead of silently dropping it.
    // --- end weezterm remote features ---

    let mut hints = HashMap::new();
    hints.insert("urgency", Value::U8(2 /* Critical */));
    let notification = proxy
        .notify(
            // --- weezterm remote features ---
            "weezterm",
            0,
            "com.vicondoa.weezterm",
            // --- end weezterm remote features ---
            &notif.title,
            &notif.message,
            if notif.url.is_some() && has_actions {
                &["show", "Show"]
            } else {
                &[]
            },
            &hints,
            notif.timeout.map(|d| d.as_millis() as _).unwrap_or(0),
        )
        .await?;

    // --- weezterm remote features ---
    // Only listen for action invocations if we actually added actions.
    // Without actions (no URL or server doesn't support them), we're done.
    if notif.url.is_none() || !has_actions {
        return Ok(());
    }
    // --- end weezterm remote features ---

    let (mut invoked_stream, abort_invoked) = abortable(proxy.receive_action_invoked().await?);
    let (mut closed_stream, abort_closed) = abortable(proxy.receive_notification_closed().await?);

    // --- weezterm remote features ---
    // Spawn a task to handle cancel_flag and timeout: close the notification
    // when either fires, which will trigger the closed_stream signal.
    {
        let cancel_flag = notif.cancel_flag.clone();
        let timeout_duration = notif.timeout;
        let nid = notification;
        let conn = connection.clone();
        std::thread::spawn(move || {
            let deadline = timeout_duration
                .map(|d| std::time::Instant::now() + d)
                .unwrap_or_else(|| std::time::Instant::now() + std::time::Duration::from_secs(120));
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if std::time::Instant::now() >= deadline {
                    break;
                }
                if let Some(ref flag) = cancel_flag {
                    if flag.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                }
            }
            // Close the notification (will trigger NotificationClosed signal)
            async_io::block_on(async {
                let proxy = NotificationsProxy::new(&conn).await.ok();
                if let Some(proxy) = proxy {
                    proxy.close_notification(nid).await.ok();
                }
            });
        });
    }
    // --- end weezterm remote features ---

    futures_util::try_join!(
        async {
            while let Some(signal) = invoked_stream.next().await {
                let args = signal.args()?;
                if args.nid == notification {
                    if let Some(url) = notif.url.as_ref() {
                        wezterm_open_url::open_url(url);
                        abort_closed.abort();
                        break;
                    }
                }
            }
            Ok::<(), zbus::Error>(())
        },
        async {
            while let Some(signal) = closed_stream.next().await {
                let args = signal.args()?;
                let _reason = Reason::new(args.reason);
                if args.nid == notification {
                    abort_invoked.abort();
                    break;
                }
            }
            Ok(())
        }
    )?;

    Ok(())
}

pub fn show_notif(notif: ToastNotification) -> Result<(), Box<dyn std::error::Error>> {
    // Run this in a separate thread as we don't know if dbus or the notification
    // service on the other end are up, and we'd otherwise block for some time.
    std::thread::spawn(move || {
        let res = async_io::block_on(async move { show_notif_impl(notif).await });
        if let Err(err) = res {
            log::error!("while showing notification: {:#}", err);
        }
    });
    Ok(())
}
