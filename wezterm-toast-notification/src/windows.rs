#![cfg(windows)]

use crate::ToastNotification as TN;
use xml::escape::escape_str_pcdata;

use windows::core::{Error as WinError, IInspectable, Interface, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::E_POINTER;
use windows::UI::Notifications::{
    ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
};

fn unwrap_arg<T>(a: &Option<T>) -> Result<&T, WinError> {
    match a {
        Some(t) => Ok(t),
        None => Err(WinError::new(E_POINTER, HSTRING::from("option is none"))),
    }
}

fn show_notif_impl(toast: TN) -> Result<(), Box<dyn std::error::Error>> {
    let xml = XmlDocument::new()?;

    let url_actions = if toast.url.is_some() {
        r#"
        <actions>
           <action content="Show" arguments="show" />
        </actions>
        "#
    } else {
        ""
    };

    // --- weezterm remote features ---
    // Use scenario="urgentMessage" for URL confirmation toasts so they
    // stay in the foreground and don't slide into Action Center.
    let scenario = if toast.url.is_some() {
        r#" scenario="urgentMessage""#
    } else {
        ""
    };
    // --- end weezterm remote features ---

    xml.LoadXml(HSTRING::from(format!(
        r#"<toast duration="long"{}>
        <visual>
            <binding template="ToastGeneric">
                <text>{}</text>
                <text>{}</text>
            </binding>
        </visual>
        {}
    </toast>"#,
        scenario,
        escape_str_pcdata(&toast.title),
        escape_str_pcdata(&toast.message),
        url_actions
    )))?;

    let notif = ToastNotification::CreateToastNotification(xml)?;

    notif.Activated(&TypedEventHandler::new({
        let url = toast.url.clone();
        let done_tx = if toast.url.is_some() {
            Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )))
        } else {
            None
        };
        let done_flag = done_tx.clone();
        move |_: &Option<ToastNotification>, result: &Option<IInspectable>| {
            let result = unwrap_arg(result)?.cast::<ToastActivatedEventArgs>()?;
            let args = result.Arguments()?;

            if args == "show" {
                if let Some(url) = url.as_ref() {
                    wezterm_open_url::open_url(url);
                }
            }
            if let Some(flag) = done_flag.as_ref() {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            Ok(())
        }
    }))?;

    // --- weezterm remote features ---
    // Track dismissal so we can keep the thread alive for click-to-open toasts.
    let dismissed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let flag = dismissed.clone();
        notif.Dismissed(&TypedEventHandler::new(
            move |_: &Option<ToastNotification>, _| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        ))?;
    }
    {
        let flag = dismissed.clone();
        notif.Failed(&TypedEventHandler::new(
            move |_: &Option<ToastNotification>, _| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        ))?;
    }
    // --- end weezterm remote features ---

    let notifier = ToastNotificationManager::CreateToastNotifierWithId(HSTRING::from(
        // --- weezterm remote features ---
        "com.vicondoa.weezterm",
        // --- end weezterm remote features ---
    ))?;

    notifier.Show(&notif)?;

    // --- weezterm remote features ---
    // If the toast has a click action (URL), keep the thread alive until
    // the notification is dismissed/clicked/failed/cancelled/timed out.
    if toast.url.is_some() {
        let wait_secs = toast.timeout.map(|d| d.as_secs()).unwrap_or(120);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait_secs);
        let cancel = toast.cancel_flag.as_ref();
        while !dismissed.load(std::sync::atomic::Ordering::SeqCst)
            && std::time::Instant::now() < deadline
        {
            // Check cancel flag (new URL confirmation replaces this one)
            if let Some(flag) = cancel {
                if flag.load(std::sync::atomic::Ordering::SeqCst) {
                    // Dismiss the toast programmatically
                    notifier.Hide(&notif).ok();
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    // --- end weezterm remote features ---

    Ok(())
}

pub fn show_notif(notif: TN) -> Result<(), Box<dyn std::error::Error>> {
    // We need to be in a different thread from the caller
    // in case we get called in the guts of a windows message
    // loop dispatch and are unable to pump messages
    std::thread::spawn(move || {
        if let Err(err) = show_notif_impl(notif) {
            log::error!("Failed to show toast notification: {:#}", err);
        }
    });

    Ok(())
}
