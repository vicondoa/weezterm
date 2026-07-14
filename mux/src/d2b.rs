use d2b_toolkit_core::WorkloadTarget;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use termwiz::cell::unicode_column_width;
use termwiz_funcs::{truncate_left, truncate_right};

pub use d2b_toolkit_core::{
    IsolationPosture, KnownFeatureFlag, ShellSessionState, WorkloadAvailability,
    WorkloadProviderKind,
};

pub const D2B_BOUND_TARGET_ENV: &str = "WEEZTERM_D2B_BOUND_TARGET";
pub const D2B_BOUND_VM_ENV: &str = "WEEZTERM_D2B_BOUND_VM";
pub const D2B_SHELL_NAME_ENV: &str = "WEEZTERM_D2B_SHELL_NAME";

pub fn normalize_d2b_target(target: &str) -> Result<String, String> {
    if target.starts_with("d2b://") || target.contains('.') {
        return WorkloadTarget::parse(target)
            .map(|target| target.to_canonical())
            .map_err(|_| {
                "d2b targets must use `<workload>.<realm>.d2b` with lowercase labels".to_string()
            });
    }

    let mut chars = target.chars();
    if !matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || target.starts_with("sys-")
        || target == "launcher"
    {
        return Err("legacy d2b target has an invalid VM name".to_string());
    }
    Ok(target.to_string())
}

pub fn resolve_bound_target_aliases(
    target: Option<&str>,
    vm: Option<&str>,
) -> Result<Option<String>, String> {
    match (target, vm) {
        (Some(target), Some(vm)) => {
            let target = normalize_d2b_target(target)?;
            let vm = normalize_d2b_target(vm)?;
            if target != vm {
                return Err(format!(
                    "{D2B_BOUND_TARGET_ENV} and compatibility alias {D2B_BOUND_VM_ENV} differ; \
                     unset the alias or make both values identical"
                ));
            }
            Ok(Some(target))
        }
        (Some(target), None) | (None, Some(target)) => normalize_d2b_target(target).map(Some),
        (None, None) => Ok(None),
    }
}

pub fn bound_target_from_env() -> Result<Option<String>, String> {
    let target = std::env::var(D2B_BOUND_TARGET_ENV)
        .map(Some)
        .or_else(|err| match err {
            std::env::VarError::NotPresent => Ok(None),
            std::env::VarError::NotUnicode(_) => {
                Err(format!("{D2B_BOUND_TARGET_ENV} must contain valid UTF-8"))
            }
        })?;
    let vm = std::env::var(D2B_BOUND_VM_ENV)
        .map(Some)
        .or_else(|err| match err {
            std::env::VarError::NotPresent => Ok(None),
            std::env::VarError::NotUnicode(_) => {
                Err(format!("{D2B_BOUND_VM_ENV} must contain valid UTF-8"))
            }
        })?;
    resolve_bound_target_aliases(target.as_deref(), vm.as_deref())
}

pub fn validate_shell_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return Err("shell names must be 1-64 ASCII bytes".to_string());
    }

    let first = bytes[0];
    if !(first.is_ascii_alphanumeric() || first == b'_') {
        return Err("shell names must start with [A-Za-z0-9_]".to_string());
    }
    if first == b'-' {
        return Err("shell names must not start with '-'".to_string());
    }

    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err("shell names may contain only [A-Za-z0-9._-]".to_string());
    }

    Ok(())
}

pub fn friendly_session_name(target: &str, existing: &[String]) -> String {
    let legacy_base = format!("{}-shell", sanitize_shell_component(target));
    let base = if !target.contains('.')
        && !target.starts_with("d2b://")
        && validate_shell_name(&legacy_base).is_ok()
    {
        legacy_base
    } else {
        format!("d2b-{}-shell", &stable_target_key(target)[..16])
    };
    if !existing.iter().any(|name| name == &base) {
        return base;
    }
    for idx in 2..=64 {
        let candidate = format!("{base}-{idx}");
        if !existing.iter().any(|name| name == &candidate)
            && validate_shell_name(&candidate).is_ok()
        {
            return candidate;
        }
    }
    "default".to_string()
}

pub fn d2b_tab_title(target: &str, session: &str, guest_osc_title: &str) -> String {
    let target = sanitize_display_label(target);
    let session = sanitize_display_label(session);
    let suffix = format!("[{target}:{session}]");
    let guest_osc_title = sanitize_display_text(guest_osc_title);
    if guest_osc_title.is_empty() || guest_osc_title == "wezterm" {
        suffix
    } else if guest_osc_title == suffix || guest_osc_title.ends_with(&format!(" {suffix}")) {
        guest_osc_title
    } else {
        format!("{guest_osc_title} {suffix}")
    }
}

pub fn d2b_tab_title_for_width(
    target: &str,
    session: &str,
    guest_osc_title: &str,
    max_width: usize,
) -> String {
    let target = sanitize_display_label(target);
    let session = sanitize_display_label(session);
    let suffix = format!("[{target}:{session}]");
    let suffix_width = unicode_column_width(&suffix, None);
    if max_width <= suffix_width {
        return truncate_left(&suffix, max_width);
    }

    let guest_osc_title = sanitize_display_text(guest_osc_title);
    if guest_osc_title.is_empty() || guest_osc_title == "wezterm" || guest_osc_title == suffix {
        return suffix;
    }

    let suffix_marker = format!(" {suffix}");
    let guest_osc_title = guest_osc_title
        .strip_suffix(&suffix_marker)
        .unwrap_or(&guest_osc_title);
    let guest_width = max_width - suffix_width - 1;
    let guest_osc_title = truncate_right(guest_osc_title, guest_width);
    if guest_osc_title.is_empty() {
        suffix
    } else {
        format!("{guest_osc_title} {suffix}")
    }
}

pub fn target_domain_key(target: &str) -> String {
    format!("d2b-{}", stable_target_key(target))
}

pub fn target_mux_socket_path(runtime_dir: &Path, target: &str) -> PathBuf {
    runtime_dir.join(format!(
        "gui-sock-d2b-{}-{}",
        stable_target_key(target),
        std::process::id()
    ))
}

pub fn vm_mux_socket_path(runtime_dir: &Path, vm: &str) -> PathBuf {
    target_mux_socket_path(runtime_dir, vm)
}

fn stable_target_key(target: &str) -> String {
    let normalized = normalize_d2b_target(target);
    let target = normalized.as_deref().unwrap_or("<invalid-d2b-target>");
    let mut hasher = Sha256::new();
    hasher.update(b"weezterm-d2b-target-key-v1");
    hasher.update((target.len() as u64).to_le_bytes());
    hasher.update(target.as_bytes());
    let digest = hasher.finalize();
    let mut key = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write as _;
        let _ = write!(key, "{byte:02x}");
    }
    key
}

fn sanitize_shell_component(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "d2b".to_string()
    } else {
        out.to_string()
    }
}

pub fn sanitize_display_label(value: &str) -> String {
    let label = sanitize_display_text(value);
    if label.is_empty() {
        "unnamed".to_string()
    } else {
        label
    }
}

fn sanitize_display_text(value: &str) -> String {
    const MAX_DISPLAY_CHARS: usize = 96;
    let mut out = String::new();
    let mut chars = value.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && matches!(chars.peek(), Some('\\')) {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if c.is_control() {
            continue;
        }
        out.push(c);
        if out.chars().count() >= MAX_DISPLAY_CHARS {
            break;
        }
    }
    out
}

#[cfg(target_os = "linux")]
mod imp {
    use crate::domain::{alloc_domain_id, Domain, DomainId, DomainState};
    use crate::pane::{alloc_pane_id, CachePolicy, CloseReason, LogicalLine, Pane, PaneId};
    use crate::renderable::{
        terminal_for_each_logical_line_in_stable_range_mut, terminal_get_cursor_position,
        terminal_get_dimensions, terminal_get_dirty_lines, terminal_get_lines,
        terminal_with_lines_mut, RenderableDimensions, StableCursorPosition,
    };
    use crate::window::WindowId;
    use crate::{Mux, MuxNotification};
    use anyhow::{anyhow, bail};
    use async_channel::{Receiver, Sender, TrySendError};
    use async_io::Async;
    use async_trait::async_trait;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use d2b_client::{AttachedShell, ClientError, FrameBounds, PublicSocketClient};
    use d2b_toolkit_core::{
        Capability, Hello, HelloResponse, KnownFeatureFlag, Redacted, ShellName, ShellSessionState,
        SocketClass, TerminalSize as D2bTerminalSize, TerminalStream, ToolkitError,
        WorkloadAvailability, WorkloadProviderKind, WorkloadPublicSummary,
    };
    use futures::io::{AsyncRead, AsyncWrite};
    use futures::{future, Future};
    use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
    use rangeset::RangeSet;
    use sha2::{Digest, Sha256};
    use socket2::{Domain as SocketDomain, SockAddr, Socket, Type};
    use std::collections::HashMap;
    use std::convert::TryInto;
    use std::future::Future as StdFuture;
    use std::io::{Error as IoError, ErrorKind, Write};
    use std::ops::Range;
    use std::os::fd::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Weak};
    use std::time::Duration;
    use termwiz::escape::parser::Parser;
    use termwiz::surface::{Line, SequenceNo};
    use url::Url;
    use wezterm_term::color::ColorPalette;
    use wezterm_term::{
        Alert, AlertHandler, KeyCode, KeyModifiers, MouseEvent, StableRowIndex,
        TerminalConfiguration, TerminalSize,
    };

    const DEFAULT_SOCKET: &str = "/run/d2b/public.sock";
    const DEFAULT_QUEUE_DEPTH: usize = 64;
    const DEFAULT_EVENT_DEPTH: usize = 64;
    const DEFAULT_READ_MAX: u64 = 64 * 1024;
    const DEFAULT_READ_WAIT_MS: u64 = 25;
    const MAX_INPUT_CHUNK: usize = 64 * 1024;

    pub type TransportFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

    #[derive(Clone, Eq, PartialEq)]
    pub struct D2bCorrelationId(String);

    impl D2bCorrelationId {
        pub fn from_sensitive(kind: &'static str, value: impl AsRef<[u8]>) -> Self {
            let value = value.as_ref();
            let mut hasher = Sha256::new();
            hasher.update(b"weezterm-d2b-correlation-v1");
            hasher.update((kind.len() as u64).to_le_bytes());
            hasher.update(kind.as_bytes());
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value);
            Self(format!("d2b:{:x}", hasher.finalize()))
        }

        pub fn from_target_session(kind: &'static str, target: &str, session: &str) -> Self {
            let mut value = Vec::with_capacity(target.len() + session.len() + 16);
            value.extend_from_slice(&(target.len() as u64).to_le_bytes());
            value.extend_from_slice(target.as_bytes());
            value.extend_from_slice(&(session.len() as u64).to_le_bytes());
            value.extend_from_slice(session.as_bytes());
            Self::from_sensitive(kind, value)
        }
    }

    impl std::fmt::Debug for D2bCorrelationId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::fmt::Display for D2bCorrelationId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    #[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
    pub enum D2bProviderError {
        #[error("d2b provider queue full while {operation}; reattach required ({correlation})")]
        Backpressure {
            operation: &'static str,
            correlation: D2bCorrelationId,
        },
        #[error("d2b provider timed out while {operation}; reattach required")]
        Timeout {
            operation: &'static str,
            correlation: Option<D2bCorrelationId>,
        },
        #[error("d2b provider disconnected while {operation}; reattach required")]
        Disconnected {
            operation: &'static str,
            correlation: Option<D2bCorrelationId>,
        },
        #[error("d2b provider stale session while {operation}; reattach required ({correlation})")]
        StaleSession {
            operation: &'static str,
            correlation: D2bCorrelationId,
        },
        #[error("d2b provider dropped terminal output; reattach required ({correlation})")]
        DroppedOutput { correlation: D2bCorrelationId },
        #[error("d2b daemon returned typed error `{kind}` while {operation}")]
        Daemon {
            operation: &'static str,
            kind: String,
            correlation: Option<D2bCorrelationId>,
        },
        #[error("d2b provider attach failed: {kind}")]
        AttachFailed { kind: String },
        #[error("d2b daemon is missing required feature `{feature}`; update d2b and retry")]
        FeatureSkew { feature: KnownFeatureFlag },
        #[error("d2b target resolution failed: {kind}")]
        TargetResolution { kind: &'static str },
        #[error("d2b target shell is unavailable: {reason}")]
        TargetUnavailable { reason: &'static str },
    }

    #[derive(Clone, Eq, PartialEq)]
    pub struct D2bTargetStatus {
        target: String,
        pub provider_kind: Option<WorkloadProviderKind>,
        pub isolation: Option<d2b_toolkit_core::IsolationPosture>,
        pub availability: Option<WorkloadAvailability>,
        pub shell_capable: bool,
        posture_known: bool,
        required_feature: Option<KnownFeatureFlag>,
    }

    impl D2bTargetStatus {
        pub fn new(
            target: impl Into<String>,
            provider_kind: WorkloadProviderKind,
            isolation: d2b_toolkit_core::IsolationPosture,
            availability: WorkloadAvailability,
            shell_capable: bool,
        ) -> Result<Self, String> {
            Ok(Self {
                target: super::normalize_d2b_target(&target.into())?,
                provider_kind: Some(provider_kind),
                isolation: Some(isolation),
                availability: Some(availability),
                shell_capable,
                posture_known: true,
                required_feature: None,
            })
        }

        fn legacy(target: String) -> Self {
            Self {
                target,
                provider_kind: None,
                isolation: None,
                availability: None,
                shell_capable: true,
                posture_known: false,
                required_feature: None,
            }
        }

        fn from_workload(workload: &WorkloadPublicSummary) -> Self {
            Self {
                target: workload.identity().target().to_canonical(),
                provider_kind: Some(workload.provider_kind()),
                isolation: Some(workload.execution_posture().isolation()),
                availability: Some(workload.availability()),
                shell_capable: workload.capabilities().has(Capability::PersistentShell)
                    && workload.capabilities().has(Capability::Pty),
                posture_known: true,
                required_feature: None,
            }
        }

        fn require_feature(&mut self, feature: KnownFeatureFlag) {
            self.required_feature = Some(feature);
        }

        pub fn target(&self) -> &str {
            &self.target
        }

        pub fn is_unsafe_local(&self) -> bool {
            self.provider_kind == Some(WorkloadProviderKind::UnsafeLocal)
                || self.isolation == Some(d2b_toolkit_core::IsolationPosture::UnsafeLocal)
        }

        pub fn is_shell_ready(&self) -> bool {
            self.shell_capable
                && self.required_feature.is_none()
                && self
                    .availability
                    .map(|availability| availability == WorkloadAvailability::Ready)
                    .unwrap_or(true)
        }

        pub fn required_feature(&self) -> Option<KnownFeatureFlag> {
            self.required_feature
        }

        pub fn warning_text(&self) -> Option<String> {
            let mut warnings = Vec::new();
            if self.is_unsafe_local() {
                warnings.push("UNSAFE LOCAL — NO ISOLATION".to_string());
            }
            if !self.posture_known {
                warnings.push("provider posture unavailable (daemon feature skew)".to_string());
            }
            if let Some(availability) = self.availability {
                if let Some(message) = availability_message(availability) {
                    warnings.push(message.to_string());
                }
            }
            if !self.shell_capable {
                warnings.push("persistent shell unavailable".to_string());
            }
            if let Some(feature) = self.required_feature {
                warnings.push(format!("daemon lacks required `{feature}` feature"));
            }
            (!warnings.is_empty()).then(|| warnings.join("; "))
        }
    }

    impl std::fmt::Debug for D2bTargetStatus {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("D2bTargetStatus")
                .field("target", &"<redacted>")
                .field("provider_kind", &self.provider_kind)
                .field("isolation", &self.isolation)
                .field("availability", &self.availability)
                .field("shell_capable", &self.shell_capable)
                .field("posture_known", &self.posture_known)
                .field("required_feature", &self.required_feature)
                .finish()
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct D2bDiscovery {
        pub status: D2bTargetStatus,
        pub sessions: Vec<D2bSession>,
    }

    #[derive(Clone, Eq, PartialEq)]
    pub struct D2bSession {
        pub id: String,
        pub label: String,
        pub target: String,
        pub workspace: Option<String>,
        pub state: ShellSessionState,
        pub attached: bool,
        pub is_default: bool,
        pub correlation_id: D2bCorrelationId,
    }

    impl std::fmt::Debug for D2bSession {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("D2bSession")
                .field("id", &"<redacted>")
                .field("label", &"<redacted>")
                .field("target", &"<redacted>")
                .field("workspace", &self.workspace.as_ref().map(|_| "<redacted>"))
                .field("state", &self.state)
                .field("attached", &self.attached)
                .field("is_default", &self.is_default)
                .field("correlation_id", &self.correlation_id)
                .finish()
        }
    }

    #[derive(Clone, Eq, PartialEq)]
    pub struct D2bPaneHandle {
        pub target: String,
        pub session_id: String,
        pub pane_id: String,
        pub correlation_id: D2bCorrelationId,
    }

    impl std::fmt::Debug for D2bPaneHandle {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("D2bPaneHandle")
                .field("target", &"<redacted>")
                .field("session_id", &"<redacted>")
                .field("pane_id", &"<redacted>")
                .field("correlation_id", &self.correlation_id)
                .finish()
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum D2bDetachReason {
        PaneClose,
        PaneKill,
        Drop,
        DomainDetach,
    }

    #[derive(Clone, Eq, PartialEq)]
    pub struct D2bAttachRequest {
        pub session_id: Option<String>,
        pub size: TerminalSize,
    }

    impl std::fmt::Debug for D2bAttachRequest {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("D2bAttachRequest")
                .field(
                    "session_id",
                    &self.session_id.as_ref().map(|_| "<redacted>"),
                )
                .field("size", &self.size)
                .finish()
        }
    }

    enum D2bPaneCommand {
        Write { bytes: Vec<u8> },
        Resize { size: TerminalSize },
        CloseAttach { reason: D2bDetachReason },
    }

    impl std::fmt::Debug for D2bPaneCommand {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Write { bytes } => f
                    .debug_struct("Write")
                    .field("bytes", &format_args!("<redacted:{}>", bytes.len()))
                    .finish(),
                Self::Resize { size } => f.debug_struct("Resize").field("size", size).finish(),
                Self::CloseAttach { reason } => f
                    .debug_struct("CloseAttach")
                    .field("reason", reason)
                    .finish(),
            }
        }
    }

    enum D2bPaneEvent {
        Output(Vec<u8>),
        Closed,
        ReattachRequired(D2bProviderError),
    }

    impl std::fmt::Debug for D2bPaneEvent {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Output(bytes) => f
                    .debug_struct("Output")
                    .field("bytes", &format_args!("<redacted:{}>", bytes.len()))
                    .finish(),
                Self::Closed => f.write_str("Closed"),
                Self::ReattachRequired(err) => {
                    f.debug_tuple("ReattachRequired").field(err).finish()
                }
            }
        }
    }

    pub struct D2bAttachedPane {
        handle: D2bPaneHandle,
        command_tx: Sender<D2bPaneCommand>,
        event_rx: Receiver<D2bPaneEvent>,
    }

    impl D2bAttachedPane {
        fn new(
            handle: D2bPaneHandle,
            command_tx: Sender<D2bPaneCommand>,
            event_rx: Receiver<D2bPaneEvent>,
        ) -> Self {
            Self {
                handle,
                command_tx,
                event_rx,
            }
        }
    }

    fn d2b_session_from_command(
        command: Option<portable_pty::CommandBuilder>,
    ) -> anyhow::Result<Option<String>> {
        let Some(command) = command else {
            return Ok(None);
        };

        let Some(name) = command
            .get_env(super::D2B_SHELL_NAME_ENV)
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())
        else {
            bail!("d2b domains attach existing sessions; command spawning is unsupported");
        };

        super::validate_shell_name(&name).map_err(|err| anyhow!(err))?;
        Ok(Some(name))
    }

    pub trait D2bTransport: Send + Sync {
        fn discover(&self) -> TransportFuture<'_, D2bDiscovery>;

        fn attach(&self, request: D2bAttachRequest) -> TransportFuture<'_, D2bAttachedPane>;
    }

    #[derive(Clone, Debug)]
    pub struct D2bRuntimeConfig {
        pub socket_path: PathBuf,
        pub connect_timeout: Duration,
        pub write_timeout: Duration,
        pub read_timeout: Duration,
        pub shell_management_timeout: Duration,
        pub command_queue_depth: usize,
        pub event_queue_depth: usize,
        pub output_read_max: u64,
        pub output_wait_ms: u64,
    }

    impl Default for D2bRuntimeConfig {
        fn default() -> Self {
            Self {
                socket_path: PathBuf::from(DEFAULT_SOCKET),
                connect_timeout: Duration::from_secs(2),
                write_timeout: Duration::from_secs(2),
                read_timeout: Duration::from_secs(2),
                shell_management_timeout: Duration::from_secs(15),
                command_queue_depth: DEFAULT_QUEUE_DEPTH,
                event_queue_depth: DEFAULT_EVENT_DEPTH,
                output_read_max: DEFAULT_READ_MAX,
                output_wait_ms: DEFAULT_READ_WAIT_MS,
            }
        }
    }

    const MAX_PUBLIC_PACKET: usize = 1024 * 1024 + 4;

    pub struct D2bSocket {
        fd: Async<Socket>,
        read_buf: Vec<u8>,
        read_len: usize,
        read_pos: usize,
        packet_limit: usize,
        write_buf: Vec<u8>,
    }

    impl D2bSocket {
        fn new(socket: Socket) -> std::io::Result<Self> {
            Self::with_packet_limit(socket, MAX_PUBLIC_PACKET)
        }

        fn with_packet_limit(socket: Socket, packet_limit: usize) -> std::io::Result<Self> {
            if packet_limit == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "d2bd public socket packet bound must be non-zero",
                ));
            }
            Ok(Self {
                fd: Async::new(socket)?,
                read_buf: vec![0_u8; packet_limit],
                read_len: 0,
                read_pos: 0,
                packet_limit,
                write_buf: Vec::new(),
            })
        }
    }

    impl AsyncRead for D2bSocket {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if self.read_pos >= self.read_len {
                loop {
                    let fd = self.fd.get_ref().as_raw_fd();
                    match nix::sys::socket::recv(
                        fd,
                        &mut self.read_buf,
                        nix::sys::socket::MsgFlags::MSG_DONTWAIT
                            | nix::sys::socket::MsgFlags::MSG_TRUNC,
                    ) {
                        Ok(len) if len > self.packet_limit => {
                            return std::task::Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "d2bd public socket packet exceeded the frame bound",
                            )));
                        }
                        Ok(len) => {
                            self.read_len = len;
                            self.read_pos = 0;
                            if self.read_len == 0 {
                                return std::task::Poll::Ready(Ok(0));
                            }
                            break;
                        }
                        Err(nix::errno::Errno::EAGAIN) => match self.fd.poll_readable(cx) {
                            std::task::Poll::Pending => return std::task::Poll::Pending,
                            std::task::Poll::Ready(Ok(())) => continue,
                            std::task::Poll::Ready(Err(error)) => {
                                return std::task::Poll::Ready(Err(error));
                            }
                        },
                        Err(nix::errno::Errno::EINTR) => continue,
                        Err(error) => {
                            return std::task::Poll::Ready(Err(errno_to_io(error)));
                        }
                    }
                }
            }
            let available = &self.read_buf[self.read_pos..self.read_len];
            let len = available.len().min(buf.len());
            buf[..len].copy_from_slice(&available[..len]);
            self.read_pos += len;
            std::task::Poll::Ready(Ok(len))
        }
    }

    impl AsyncWrite for D2bSocket {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if self.write_buf.len().saturating_add(buf.len()) > MAX_PUBLIC_PACKET {
                return std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "d2bd public socket packet exceeded the frame bound",
                )));
            }
            self.write_buf.extend_from_slice(buf);
            std::task::Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            while !self.write_buf.is_empty() {
                match nix::sys::socket::send(
                    self.fd.get_ref().as_raw_fd(),
                    &self.write_buf,
                    nix::sys::socket::MsgFlags::MSG_DONTWAIT,
                ) {
                    Ok(sent) if sent == self.write_buf.len() => self.write_buf.clear(),
                    Ok(_) => {
                        return std::task::Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::WriteZero,
                            "short write on public seqpacket socket",
                        )));
                    }
                    Err(nix::errno::Errno::EAGAIN) => match self.fd.poll_writable(cx) {
                        std::task::Poll::Pending => return std::task::Poll::Pending,
                        std::task::Poll::Ready(Ok(())) => continue,
                        std::task::Poll::Ready(Err(error)) => {
                            return std::task::Poll::Ready(Err(error));
                        }
                    },
                    Err(nix::errno::Errno::EINTR) => continue,
                    Err(error) => {
                        return std::task::Poll::Ready(Err(errno_to_io(error)));
                    }
                }
            }
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            self.poll_flush(cx)
        }
    }

    type D2bClient = PublicSocketClient<D2bSocket>;
    type D2bShell = AttachedShell<D2bSocket>;

    pub struct NativeD2bTransport {
        target: String,
        config: D2bRuntimeConfig,
        bounds: FrameBounds,
    }

    impl NativeD2bTransport {
        pub fn new(
            target: impl Into<String>,
            config: D2bRuntimeConfig,
        ) -> Result<Self, D2bProviderError> {
            let target = super::normalize_d2b_target(&target.into()).map_err(|_| {
                D2bProviderError::TargetResolution {
                    kind: "invalid-target",
                }
            })?;
            Ok(Self {
                target,
                config,
                bounds: FrameBounds::default_public_daemon(),
            })
        }

        async fn connect_client(&self) -> Result<D2bClient, D2bProviderError> {
            d2b_client::ensure_allowed_socket(classify_socket_path(&self.config.socket_path))
                .map_err(|err| D2bProviderError::AttachFailed {
                    kind: err.to_string(),
                })?;
            let mut socket = timeout_result(
                "connecting to d2b public socket",
                None,
                self.config.connect_timeout,
                async_unix_connect(&self.config.socket_path),
            )
            .await?;

            client_op(
                "sending d2b hello",
                None,
                self.config.write_timeout,
                d2b_client::send_hello(
                    &mut socket,
                    &Hello::toolkit_client(vec![
                        KnownFeatureFlag::TypedErrors.wire_value(),
                        KnownFeatureFlag::ConfiguredLaunchV1.wire_value(),
                        KnownFeatureFlag::UnsafeLocalProviderV1.wire_value(),
                        KnownFeatureFlag::UnsafeLocalShellV1.wire_value(),
                    ]),
                    self.bounds,
                ),
            )
            .await?;
            let hello = client_op(
                "reading d2b hello",
                None,
                self.config.read_timeout,
                d2b_client::read_hello_response(&mut socket, self.bounds),
            )
            .await?;
            let HelloResponse::HelloOk(hello) = hello else {
                return Err(D2bProviderError::AttachFailed {
                    kind: "hello-rejected".to_string(),
                });
            };

            Ok(PublicSocketClient::with_bounds_and_negotiated_capabilities(
                socket,
                self.bounds,
                hello.negotiated_capabilities(),
            ))
        }

        async fn resolve_target(
            &self,
            client: &mut D2bClient,
        ) -> Result<D2bTargetStatus, D2bProviderError> {
            match client_op(
                "reading d2b workload metadata",
                None,
                self.config.read_timeout,
                client.workload_inventory(),
            )
            .await
            {
                Ok(inventory) => {
                    let workload = select_workload(&self.target, &inventory.workloads)?;
                    Ok(D2bTargetStatus::from_workload(workload))
                }
                Err(D2bProviderError::FeatureSkew { .. }) if is_legacy_target(&self.target) => {
                    Ok(D2bTargetStatus::legacy(self.target.clone()))
                }
                Err(err) => Err(err),
            }
        }

        fn apply_unsafe_shell_requirement<T>(
            client: &mut PublicSocketClient<T>,
            status: &mut D2bTargetStatus,
        ) -> Result<(), D2bProviderError>
        where
            T: AsyncRead + AsyncWrite + Unpin,
        {
            if !status.is_unsafe_local() {
                return Ok(());
            }
            match client.require_unsafe_local_shell() {
                Ok(()) => Ok(()),
                Err(err) => {
                    let err =
                        classify_client_error("checking unsafe-local shell support", None, err);
                    if let D2bProviderError::FeatureSkew { feature } = err {
                        status.require_feature(feature);
                        Ok(())
                    } else {
                        Err(err)
                    }
                }
            }
        }
    }

    fn is_legacy_target(target: &str) -> bool {
        !target.contains('.') && !target.starts_with("d2b://")
    }

    fn select_workload<'a>(
        target: &str,
        workloads: &'a [WorkloadPublicSummary],
    ) -> Result<&'a WorkloadPublicSummary, D2bProviderError> {
        let exact = workloads
            .iter()
            .filter(|workload| workload.identity().target().as_str() == target)
            .collect::<Vec<_>>();
        match exact.as_slice() {
            [workload] => return Ok(*workload),
            [] => {}
            _ => {
                return Err(D2bProviderError::TargetResolution {
                    kind: "duplicate-canonical-target",
                })
            }
        }

        if !is_legacy_target(target) {
            return Err(D2bProviderError::TargetResolution {
                kind: "canonical-target-not-found",
            });
        }

        let legacy = workloads
            .iter()
            .filter(|workload| {
                workload
                    .identity()
                    .legacy_vm_name()
                    .map(|name| name.as_str() == target)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        match legacy.as_slice() {
            [workload] => return Ok(*workload),
            [] => {}
            _ => {
                return Err(D2bProviderError::TargetResolution {
                    kind: "ambiguous-legacy-vm-alias",
                })
            }
        }

        let workload_id = workloads
            .iter()
            .filter(|workload| workload.identity().workload_id().as_str() == target)
            .collect::<Vec<_>>();
        match workload_id.as_slice() {
            [workload] => Ok(*workload),
            [] => Err(D2bProviderError::TargetResolution {
                kind: "legacy-target-not-found",
            }),
            _ => Err(D2bProviderError::TargetResolution {
                kind: "ambiguous-workload-id-alias",
            }),
        }
    }

    fn ensure_target_shell_ready(status: &D2bTargetStatus) -> Result<(), D2bProviderError> {
        if let Some(feature) = status.required_feature() {
            return Err(D2bProviderError::FeatureSkew { feature });
        }
        if !status.shell_capable {
            return Err(D2bProviderError::TargetUnavailable {
                reason: "persistent-shell-capability-missing",
            });
        }
        if let Some(availability) = status.availability {
            if availability != WorkloadAvailability::Ready {
                return Err(D2bProviderError::TargetUnavailable {
                    reason: availability_slug(availability),
                });
            }
        }
        Ok(())
    }

    fn availability_slug(availability: WorkloadAvailability) -> &'static str {
        match availability {
            WorkloadAvailability::Ready => "ready",
            WorkloadAvailability::HelperUnavailable => "helper-unavailable",
            WorkloadAvailability::HelperStale => "helper-stale",
            WorkloadAvailability::UserManagerUnavailable => "user-manager-unavailable",
            WorkloadAvailability::GraphicalSessionInactive => "graphical-session-inactive",
            WorkloadAvailability::WaylandUnavailable => "wayland-unavailable",
            WorkloadAvailability::ProxyUnavailable => "proxy-unavailable",
            WorkloadAvailability::Degraded => "degraded",
        }
    }

    fn availability_message(availability: WorkloadAvailability) -> Option<&'static str> {
        match availability {
            WorkloadAvailability::Ready => None,
            WorkloadAvailability::HelperUnavailable => Some("user-session helper unavailable"),
            WorkloadAvailability::HelperStale => Some("user-session helper stale"),
            WorkloadAvailability::UserManagerUnavailable => {
                Some("systemd user manager unavailable")
            }
            WorkloadAvailability::GraphicalSessionInactive => Some("graphical session inactive"),
            WorkloadAvailability::WaylandUnavailable => Some("Wayland unavailable"),
            WorkloadAvailability::ProxyUnavailable => Some("Wayland proxy unavailable"),
            WorkloadAvailability::Degraded => Some("provider degraded"),
        }
    }

    impl D2bTransport for NativeD2bTransport {
        fn discover(&self) -> TransportFuture<'_, D2bDiscovery> {
            Box::pin(async move {
                let mut client = self.connect_client().await?;
                let mut status = self.resolve_target(&mut client).await?;
                Self::apply_unsafe_shell_requirement(&mut client, &mut status)?;
                if !status.is_shell_ready() {
                    return Ok(D2bDiscovery {
                        status,
                        sessions: Vec::new(),
                    });
                }
                let result = client_op(
                    "listing d2b shells",
                    None,
                    self.config.shell_management_timeout,
                    client.shell_list(status.target().to_string()),
                )
                .await?;
                let sessions = result
                    .sessions
                    .into_iter()
                    .map(|entry| {
                        let name = entry.name.as_str().to_string();
                        D2bSession {
                            id: name.clone(),
                            label: if entry.is_default {
                                format!("{name} (default)")
                            } else {
                                name.clone()
                            },
                            target: status.target().to_string(),
                            workspace: None,
                            state: entry.state,
                            attached: entry.attached,
                            is_default: entry.is_default,
                            correlation_id: D2bCorrelationId::from_target_session(
                                "shell",
                                status.target(),
                                &name,
                            ),
                        }
                    })
                    .collect();
                Ok(D2bDiscovery { status, sessions })
            })
        }

        fn attach(&self, request: D2bAttachRequest) -> TransportFuture<'_, D2bAttachedPane> {
            Box::pin(async move {
                let mut client = self.connect_client().await?;
                let mut status = self.resolve_target(&mut client).await?;
                Self::apply_unsafe_shell_requirement(&mut client, &mut status)?;
                ensure_target_shell_ready(&status)?;
                if let Some(name) = request.session_id.as_deref() {
                    super::validate_shell_name(name)
                        .map_err(|kind| D2bProviderError::AttachFailed { kind })?;
                }
                let name = request
                    .session_id
                    .clone()
                    .map(ShellName::new)
                    .transpose()
                    .map_err(|err| D2bProviderError::AttachFailed {
                        kind: err.to_string(),
                    })?;
                let size = to_d2b_size(request.size);
                let shell = client_op(
                    "attaching d2b shell",
                    None,
                    self.config.shell_management_timeout,
                    client.attach_shell(status.target().to_string(), name, false, size),
                )
                .await?;
                let resolved_name = shell.resolved_name().as_str().to_string();
                let correlation_id = D2bCorrelationId::from_target_session(
                    "attached-shell",
                    status.target(),
                    &resolved_name,
                );
                let handle = D2bPaneHandle {
                    target: status.target().to_string(),
                    session_id: resolved_name.clone(),
                    pane_id: D2bCorrelationId::from_target_session(
                        "pane",
                        status.target(),
                        &resolved_name,
                    )
                    .to_string(),
                    correlation_id: correlation_id.clone(),
                };
                let (command_tx, command_rx) =
                    async_channel::bounded(self.config.command_queue_depth);
                let (event_tx, event_rx) = async_channel::bounded(self.config.event_queue_depth);
                spawn_native_actor(
                    shell,
                    command_rx,
                    event_tx,
                    self.config.clone(),
                    correlation_id,
                );
                Ok(D2bAttachedPane::new(handle, command_tx, event_rx))
            })
        }
    }

    pub fn native_domain(
        target: impl Into<String>,
        config: D2bRuntimeConfig,
    ) -> anyhow::Result<D2bDomain> {
        let target = super::normalize_d2b_target(&target.into()).map_err(anyhow::Error::msg)?;
        Ok(D2bDomain::new(
            super::target_domain_key(&target),
            target.clone(),
            Arc::new(NativeD2bTransport::new(target, config)?),
        ))
    }

    pub fn native_domain_with_name(
        name: impl Into<String>,
        target: impl Into<String>,
        config: D2bRuntimeConfig,
    ) -> anyhow::Result<D2bDomain> {
        let target = super::normalize_d2b_target(&target.into()).map_err(anyhow::Error::msg)?;
        Ok(D2bDomain::new(
            name,
            target.clone(),
            Arc::new(NativeD2bTransport::new(target, config)?),
        ))
    }

    async fn async_unix_connect(path: &Path) -> std::io::Result<D2bSocket> {
        let sockaddr = SockAddr::unix(path)?;
        let socket = Socket::new(
            SocketDomain::UNIX,
            Type::from(libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK),
            None,
        )?;
        let connecting = match socket.connect(&sockaddr) {
            Ok(()) => false,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.raw_os_error().is_some_and(|code| {
                        matches!(
                            code,
                            libc::EINPROGRESS | libc::EALREADY | libc::EAGAIN | libc::EINTR
                        )
                    }) =>
            {
                true
            }
            Err(error) => return Err(error),
        };
        let socket = D2bSocket::new(socket)?;
        if connecting {
            socket.fd.writable().await?;
            if let Some(error) = socket.fd.get_ref().take_error()? {
                return Err(error);
            }
        }
        Ok(socket)
    }

    fn errno_to_io(error: nix::errno::Errno) -> std::io::Error {
        std::io::Error::from_raw_os_error(error as i32)
    }

    fn classify_socket_path(path: &Path) -> SocketClass {
        let rendered = path.to_string_lossy();
        let trimmed = rendered.trim();
        if trimmed == "/run/d2b/public.sock" {
            return SocketClass::PublicDaemon;
        }
        if trimmed.is_empty() || trimmed == "/run/d2b/priv.sock" {
            return SocketClass::PrivilegedBroker;
        }
        match path.file_name().and_then(|name| name.to_str()) {
            Some("priv.sock" | "broker.sock" | "priv-broker.sock") => SocketClass::PrivilegedBroker,
            _ => SocketClass::Other,
        }
    }

    async fn timeout_result<F, T>(
        operation: &'static str,
        correlation: Option<D2bCorrelationId>,
        timeout: Duration,
        fut: F,
    ) -> Result<T, D2bProviderError>
    where
        F: StdFuture<Output = std::io::Result<T>>,
    {
        futures::pin_mut!(fut);
        let timer = smol::Timer::after(timeout);
        futures::pin_mut!(timer);
        match future::select(fut, timer).await {
            future::Either::Left((result, _)) => {
                result.map_err(|_| D2bProviderError::Disconnected {
                    operation,
                    correlation,
                })
            }
            future::Either::Right((_, _)) => Err(D2bProviderError::Timeout {
                operation,
                correlation,
            }),
        }
    }

    async fn client_op<F, T>(
        operation: &'static str,
        correlation: Option<D2bCorrelationId>,
        timeout: Duration,
        fut: F,
    ) -> Result<T, D2bProviderError>
    where
        F: StdFuture<Output = Result<T, ClientError>>,
    {
        futures::pin_mut!(fut);
        let timer = smol::Timer::after(timeout);
        futures::pin_mut!(timer);
        match future::select(fut, timer).await {
            future::Either::Left((result, _)) => {
                result.map_err(|err| classify_client_error(operation, correlation, err))
            }
            future::Either::Right((_, _)) => Err(D2bProviderError::Timeout {
                operation,
                correlation,
            }),
        }
    }

    fn classify_client_error(
        operation: &'static str,
        correlation: Option<D2bCorrelationId>,
        err: ClientError,
    ) -> D2bProviderError {
        match err {
            ClientError::Daemon { kind } if kind.contains("stale-session") => {
                D2bProviderError::StaleSession {
                    operation,
                    correlation: correlation.unwrap_or_else(|| {
                        D2bCorrelationId::from_sensitive("unknown-stale-session", operation)
                    }),
                }
            }
            ClientError::Daemon { kind } if kind.contains("timeout") => D2bProviderError::Timeout {
                operation,
                correlation,
            },
            ClientError::Daemon { kind } => D2bProviderError::Daemon {
                operation,
                kind,
                correlation,
            },
            ClientError::Core(ToolkitError::FeatureUnavailable { feature }) => {
                D2bProviderError::FeatureSkew { feature }
            }
            ClientError::Core(ToolkitError::InvalidTarget { .. }) => {
                D2bProviderError::TargetResolution {
                    kind: "invalid-target",
                }
            }
            ClientError::Core(_) => D2bProviderError::Disconnected {
                operation,
                correlation,
            },
            ClientError::Codec { .. }
            | ClientError::Hello { .. }
            | ClientError::UnexpectedResponse { .. }
            | ClientError::CorrelationMismatch => D2bProviderError::AttachFailed {
                kind: err.to_string(),
            },
        }
    }

    fn spawn_native_actor(
        shell: D2bShell,
        command_rx: Receiver<D2bPaneCommand>,
        event_tx: Sender<D2bPaneEvent>,
        config: D2bRuntimeConfig,
        correlation_id: D2bCorrelationId,
    ) {
        smol::spawn(async move {
            run_native_actor(shell, command_rx, event_tx, config, correlation_id).await;
        })
        .detach();
    }

    async fn run_native_actor(
        mut shell: D2bShell,
        command_rx: Receiver<D2bPaneCommand>,
        event_tx: Sender<D2bPaneEvent>,
        config: D2bRuntimeConfig,
        correlation_id: D2bCorrelationId,
    ) {
        loop {
            while let Ok(command) = command_rx.try_recv() {
                if handle_actor_command(&mut shell, command, &event_tx, &config, &correlation_id)
                    .await
                    .is_break()
                {
                    let _ = client_op(
                        "closing d2b shell attach",
                        Some(correlation_id.clone()),
                        config.shell_management_timeout,
                        shell.close_attach(),
                    )
                    .await;
                    let _ = event_tx.try_send(D2bPaneEvent::Closed);
                    return;
                }
            }

            if command_rx.is_closed() {
                let _ = client_op(
                    "closing detached d2b shell",
                    Some(correlation_id.clone()),
                    config.shell_management_timeout,
                    shell.close_attach(),
                )
                .await;
                let _ = event_tx.try_send(D2bPaneEvent::Closed);
                return;
            }

            for stream in [TerminalStream::Stdout] {
                match client_op(
                    "reading d2b shell output",
                    Some(correlation_id.clone()),
                    config.read_timeout,
                    shell.read_output(stream, config.output_read_max, true, config.output_wait_ms),
                )
                .await
                {
                    Ok(chunk) => {
                        if chunk.dropped_bytes > 0 || chunk.truncated {
                            let err = D2bProviderError::DroppedOutput {
                                correlation: correlation_id.clone(),
                            };
                            send_terminal_event(&event_tx, D2bPaneEvent::ReattachRequired(err))
                                .await;
                            let _ = shell.close_attach().await;
                            return;
                        }
                        let encoded = chunk.data_base64.into_inner_for_wire();
                        if !encoded.is_empty() {
                            match STANDARD.decode(encoded) {
                                Ok(bytes) if !bytes.is_empty() => {
                                    if event_tx.try_send(D2bPaneEvent::Output(bytes)).is_err() {
                                        let err = D2bProviderError::Backpressure {
                                            operation: "forwarding d2b shell output",
                                            correlation: correlation_id.clone(),
                                        };
                                        send_terminal_event(
                                            &event_tx,
                                            D2bPaneEvent::ReattachRequired(err),
                                        )
                                        .await;
                                        let _ = shell.close_attach().await;
                                        return;
                                    }
                                }
                                Ok(_) => {}
                                Err(_) => {
                                    let err = D2bProviderError::Disconnected {
                                        operation: "decoding d2b shell output",
                                        correlation: Some(correlation_id.clone()),
                                    };
                                    send_terminal_event(
                                        &event_tx,
                                        D2bPaneEvent::ReattachRequired(err),
                                    )
                                    .await;
                                    let _ = shell.close_attach().await;
                                    return;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        send_terminal_event(&event_tx, D2bPaneEvent::ReattachRequired(err)).await;
                        let _ = shell.close_attach().await;
                        return;
                    }
                }
            }
        }
    }

    enum ActorCommandResult {
        Continue,
        Break,
    }

    impl ActorCommandResult {
        fn is_break(&self) -> bool {
            matches!(self, Self::Break)
        }
    }

    async fn handle_actor_command(
        shell: &mut D2bShell,
        command: D2bPaneCommand,
        event_tx: &Sender<D2bPaneEvent>,
        config: &D2bRuntimeConfig,
        correlation_id: &D2bCorrelationId,
    ) -> ActorCommandResult {
        match command {
            D2bPaneCommand::Write { bytes } => {
                let result = client_op(
                    "writing d2b shell input",
                    Some(correlation_id.clone()),
                    config.write_timeout,
                    shell.write_bytes(Redacted::new(bytes), false),
                )
                .await;
                match result {
                    Ok(write) if write.backpressured || write.stdin_closed => {
                        let err = D2bProviderError::Backpressure {
                            operation: "writing d2b shell input",
                            correlation: correlation_id.clone(),
                        };
                        send_terminal_event(event_tx, D2bPaneEvent::ReattachRequired(err)).await;
                        ActorCommandResult::Break
                    }
                    Ok(_) => ActorCommandResult::Continue,
                    Err(err) => {
                        send_terminal_event(event_tx, D2bPaneEvent::ReattachRequired(err)).await;
                        ActorCommandResult::Break
                    }
                }
            }
            D2bPaneCommand::Resize { size } => {
                let result = client_op(
                    "resizing d2b shell",
                    Some(correlation_id.clone()),
                    config.write_timeout,
                    shell.resize(
                        size.rows.try_into().unwrap_or(u32::MAX),
                        size.cols.try_into().unwrap_or(u32::MAX),
                    ),
                )
                .await;
                match result {
                    Ok(_) => ActorCommandResult::Continue,
                    Err(err) => {
                        send_terminal_event(event_tx, D2bPaneEvent::ReattachRequired(err)).await;
                        ActorCommandResult::Break
                    }
                }
            }
            D2bPaneCommand::CloseAttach { reason: _ } => ActorCommandResult::Break,
        }
    }

    async fn send_terminal_event(event_tx: &Sender<D2bPaneEvent>, event: D2bPaneEvent) {
        let _ = event_tx.send(event).await;
    }

    fn to_d2b_size(size: TerminalSize) -> D2bTerminalSize {
        D2bTerminalSize {
            rows: size.rows.try_into().unwrap_or(u32::MAX),
            cols: size.cols.try_into().unwrap_or(u32::MAX),
        }
    }

    fn try_send_command(
        tx: &Sender<D2bPaneCommand>,
        command: D2bPaneCommand,
        operation: &'static str,
        correlation: D2bCorrelationId,
    ) -> Result<(), D2bProviderError> {
        match tx.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                tx.close();
                Err(D2bProviderError::Backpressure {
                    operation,
                    correlation,
                })
            }
            Err(TrySendError::Closed(_)) => Err(D2bProviderError::Disconnected {
                operation,
                correlation: Some(correlation),
            }),
        }
    }

    pub struct D2bDomain {
        id: DomainId,
        name: String,
        target: String,
        transport: Arc<dyn D2bTransport>,
        state: Mutex<DomainState>,
        panes: Mutex<Vec<Weak<D2bPane>>>,
    }

    impl D2bDomain {
        pub fn new(
            name: impl Into<String>,
            target: impl Into<String>,
            transport: Arc<dyn D2bTransport>,
        ) -> Self {
            Self {
                id: alloc_domain_id(),
                name: name.into(),
                target: target.into(),
                transport,
                state: Mutex::new(DomainState::Detached),
                panes: Mutex::new(Vec::new()),
            }
        }

        pub fn target(&self) -> &str {
            &self.target
        }

        pub fn vm_name(&self) -> &str {
            self.target()
        }

        pub async fn discover(&self) -> anyhow::Result<D2bDiscovery> {
            self.transport.discover().await
        }

        pub async fn discover_sessions(&self) -> anyhow::Result<Vec<D2bSession>> {
            Ok(self.discover().await?.sessions)
        }
    }

    #[async_trait(?Send)]
    impl Domain for D2bDomain {
        async fn spawn_pane(
            &self,
            size: TerminalSize,
            command: Option<portable_pty::CommandBuilder>,
            _command_dir: Option<String>,
        ) -> anyhow::Result<Arc<dyn Pane>> {
            let session_id = d2b_session_from_command(command)?;

            let attached = self
                .transport
                .attach(D2bAttachRequest { session_id, size })
                .await?;

            let concrete_pane = D2bPane::new(self.id, size, attached);
            self.panes.lock().push(Arc::downgrade(&concrete_pane));
            let pane: Arc<dyn Pane> = concrete_pane;
            Mux::get().add_pane(&pane)?;
            Ok(pane)
        }

        fn spawnable(&self) -> bool {
            true
        }

        fn detachable(&self) -> bool {
            true
        }

        fn domain_id(&self) -> DomainId {
            self.id
        }

        fn domain_name(&self) -> &str {
            &self.name
        }

        async fn domain_label(&self) -> String {
            match self.discover().await {
                Ok(discovery) => {
                    let availability = if discovery.status.is_shell_ready() {
                        if discovery.sessions.is_empty() {
                            "no sessions".to_string()
                        } else {
                            format!("{} session(s)", discovery.sessions.len())
                        }
                    } else {
                        "unavailable".to_string()
                    };
                    match discovery.status.warning_text() {
                        Some(warning) => {
                            format!("d2b `{}` — {availability} — {warning}", self.name)
                        }
                        None => format!("d2b `{}` — {availability}", self.name),
                    }
                }
                Err(err) => {
                    log::debug!("d2b session discovery failed: {err:#}");
                    format!(
                        "d2b `{}` — unavailable — {}",
                        self.name,
                        super::sanitize_display_label(&err.to_string())
                    )
                }
            }
        }

        async fn attach(&self, _window_id: Option<WindowId>) -> anyhow::Result<()> {
            *self.state.lock() = DomainState::Attached;
            Ok(())
        }

        fn detach(&self) -> anyhow::Result<()> {
            *self.state.lock() = DomainState::Detached;
            let mut panes = self.panes.lock();
            panes.retain(|pane| {
                if let Some(pane) = pane.upgrade() {
                    pane.detach_non_destructive(D2bDetachReason::DomainDetach);
                    true
                } else {
                    false
                }
            });
            Ok(())
        }

        fn state(&self) -> DomainState {
            *self.state.lock()
        }
    }

    pub struct D2bPane {
        pane_id: PaneId,
        domain_id: DomainId,
        terminal: Mutex<wezterm_term::Terminal>,
        writer: Mutex<Box<dyn Write + Send>>,
        handle: D2bPaneHandle,
        command_tx: Sender<D2bPaneCommand>,
        detached: AtomicBool,
    }

    struct D2bPaneNotifHandler {
        pane_id: PaneId,
    }

    impl AlertHandler for D2bPaneNotifHandler {
        fn alert(&mut self, alert: Alert) {
            let pane_id = self.pane_id;
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::get();
                if let Alert::TabTitleChanged(title) = &alert {
                    if let Some((_domain, _window_id, tab_id)) = mux.resolve_pane_id(pane_id) {
                        if let Some(tab) = mux.get_tab(tab_id) {
                            tab.set_title(title.as_deref().unwrap_or(""));
                        }
                    }
                }
                mux.notify(MuxNotification::Alert { pane_id, alert });
            })
            .detach();
        }
    }

    impl D2bPane {
        pub fn new(
            domain_id: DomainId,
            size: TerminalSize,
            attached: D2bAttachedPane,
        ) -> Arc<Self> {
            let pane_id = alloc_pane_id();
            let mut terminal = wezterm_term::Terminal::new(
                size,
                Arc::new(config::TermConfig::new()),
                config::branding::APP_NAME_DISPLAY,
                config::wezterm_version(),
                Box::new(D2bInputWriter {
                    command_tx: attached.command_tx.clone(),
                    correlation_id: attached.handle.correlation_id.clone(),
                }),
            );
            terminal.set_notification_handler(Box::new(D2bPaneNotifHandler { pane_id }));

            let pane = Arc::new(Self {
                pane_id,
                domain_id,
                terminal: Mutex::new(terminal),
                writer: Mutex::new(Box::new(D2bInputWriter {
                    command_tx: attached.command_tx.clone(),
                    correlation_id: attached.handle.correlation_id.clone(),
                })),
                handle: attached.handle,
                command_tx: attached.command_tx,
                detached: AtomicBool::new(false),
            });
            spawn_event_forwarder(Arc::downgrade(&pane), attached.event_rx);
            pane
        }

        fn detach_non_destructive(&self, reason: D2bDetachReason) {
            if self.detached.swap(true, Ordering::SeqCst) {
                return;
            }
            if let Err(err) = try_send_command(
                &self.command_tx,
                D2bPaneCommand::CloseAttach { reason },
                "closing d2b shell attach",
                self.handle.correlation_id.clone(),
            ) {
                log::warn!("failed to enqueue d2b close attach: {err}");
            }
        }

        pub fn close_non_destructive(&self) {
            self.detach_non_destructive(D2bDetachReason::PaneClose)
        }
    }

    impl Pane for D2bPane {
        fn pane_id(&self) -> PaneId {
            self.pane_id
        }

        fn get_cursor_position(&self) -> StableCursorPosition {
            terminal_get_cursor_position(&mut self.terminal.lock())
        }

        fn get_current_seqno(&self) -> SequenceNo {
            self.terminal.lock().current_seqno()
        }

        fn get_changed_since(
            &self,
            lines: Range<StableRowIndex>,
            seqno: SequenceNo,
        ) -> RangeSet<StableRowIndex> {
            terminal_get_dirty_lines(&mut self.terminal.lock(), lines, seqno)
        }

        fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
            terminal_get_lines(&mut self.terminal.lock(), lines)
        }

        fn with_lines_mut(
            &self,
            lines: Range<StableRowIndex>,
            with_lines: &mut dyn crate::pane::WithPaneLines,
        ) {
            terminal_with_lines_mut(&mut self.terminal.lock(), lines, with_lines)
        }

        fn for_each_logical_line_in_stable_range_mut(
            &self,
            lines: Range<StableRowIndex>,
            for_line: &mut dyn crate::pane::ForEachPaneLogicalLine,
        ) {
            terminal_for_each_logical_line_in_stable_range_mut(
                &mut self.terminal.lock(),
                lines,
                for_line,
            );
        }

        fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
            crate::pane::impl_get_logical_lines_via_get_lines(self, lines)
        }

        fn get_dimensions(&self) -> RenderableDimensions {
            terminal_get_dimensions(&mut self.terminal.lock())
        }

        fn get_title(&self) -> String {
            let guest_title = self.terminal.lock().get_title().to_string();
            super::d2b_tab_title(&self.handle.target, &self.handle.session_id, &guest_title)
        }

        fn send_paste(&self, text: &str) -> anyhow::Result<()> {
            for chunk in text.as_bytes().chunks(MAX_INPUT_CHUNK) {
                try_send_command(
                    &self.command_tx,
                    D2bPaneCommand::Write {
                        bytes: chunk.to_vec(),
                    },
                    "queueing d2b paste",
                    self.handle.correlation_id.clone(),
                )
                .map_err(anyhow::Error::from)?;
            }
            Ok(())
        }

        fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
            Ok(None)
        }

        fn writer(&self) -> MappedMutexGuard<'_, dyn Write> {
            MutexGuard::map(self.writer.lock(), |writer| {
                let w: &mut dyn Write = writer.as_mut();
                w
            })
        }

        fn resize(&self, size: TerminalSize) -> anyhow::Result<()> {
            self.terminal.lock().resize(size);
            try_send_command(
                &self.command_tx,
                D2bPaneCommand::Resize { size },
                "queueing d2b resize",
                self.handle.correlation_id.clone(),
            )
            .map_err(anyhow::Error::from)
        }

        fn key_down(&self, key: KeyCode, mods: KeyModifiers) -> anyhow::Result<()> {
            self.terminal.lock().key_down(key, mods)
        }

        fn key_up(&self, key: KeyCode, mods: KeyModifiers) -> anyhow::Result<()> {
            self.terminal.lock().key_up(key, mods)
        }

        fn mouse_event(&self, event: MouseEvent) -> anyhow::Result<()> {
            self.terminal.lock().mouse_event(event)
        }

        fn perform_actions(&self, actions: Vec<termwiz::escape::Action>) {
            self.terminal.lock().perform_actions(actions)
        }

        fn is_dead(&self) -> bool {
            self.detached.load(Ordering::SeqCst)
        }

        fn kill(&self) {
            self.detach_non_destructive(D2bDetachReason::PaneKill);
        }

        fn palette(&self) -> ColorPalette {
            self.terminal.lock().palette()
        }

        fn domain_id(&self) -> DomainId {
            self.domain_id
        }

        fn trusted_d2b_target(&self) -> Option<&str> {
            Some(&self.handle.target)
        }

        fn trusted_d2b_session(&self) -> Option<&str> {
            Some(&self.handle.session_id)
        }

        fn d2b_guest_title(&self) -> Option<String> {
            Some(self.terminal.lock().get_title().to_string())
        }

        fn can_close_without_prompting(&self, _reason: CloseReason) -> bool {
            true
        }

        fn is_mouse_grabbed(&self) -> bool {
            self.terminal.lock().is_mouse_grabbed()
        }

        fn is_alt_screen_active(&self) -> bool {
            self.terminal.lock().is_alt_screen_active()
        }

        fn get_current_working_dir(&self, _policy: CachePolicy) -> Option<Url> {
            self.terminal.lock().get_current_dir().cloned()
        }

        fn set_config(&self, config: Arc<dyn TerminalConfiguration>) {
            self.terminal.lock().set_config(config);
        }

        fn get_config(&self) -> Option<Arc<dyn TerminalConfiguration>> {
            Some(self.terminal.lock().get_config())
        }

        fn copy_user_vars(&self) -> HashMap<String, String> {
            HashMap::from([
                (
                    "weezterm.d2b.target".to_string(),
                    self.handle.target.clone(),
                ),
                ("weezterm.d2b.vm".to_string(), self.handle.target.clone()),
                (
                    "weezterm.d2b.session".to_string(),
                    self.handle.session_id.clone(),
                ),
            ])
        }
    }

    impl Drop for D2bPane {
        fn drop(&mut self) {
            self.detach_non_destructive(D2bDetachReason::Drop);
        }
    }

    struct D2bInputWriter {
        command_tx: Sender<D2bPaneCommand>,
        correlation_id: D2bCorrelationId,
    }

    impl Write for D2bInputWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            for chunk in buf.chunks(MAX_INPUT_CHUNK) {
                try_send_command(
                    &self.command_tx,
                    D2bPaneCommand::Write {
                        bytes: chunk.to_vec(),
                    },
                    "queueing d2b terminal input",
                    self.correlation_id.clone(),
                )
                .map_err(|err| IoError::new(ErrorKind::WouldBlock, err))?;
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn spawn_event_forwarder(pane: Weak<D2bPane>, event_rx: Receiver<D2bPaneEvent>) {
        smol::spawn(async move {
            let mut parser = Parser::new();
            while let Ok(event) = event_rx.recv().await {
                let Some(pane) = pane.upgrade() else { break };
                match event {
                    D2bPaneEvent::Output(bytes) => {
                        let mut actions = Vec::new();
                        parser.parse(&bytes, |action| actions.push(action));
                        if !actions.is_empty() {
                            pane.perform_actions(actions);
                            Mux::notify_from_any_thread(MuxNotification::PaneOutput(
                                pane.pane_id(),
                            ));
                        }
                    }
                    D2bPaneEvent::Closed => {
                        pane.detached.store(true, Ordering::SeqCst);
                        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane.pane_id()));
                        break;
                    }
                    D2bPaneEvent::ReattachRequired(err) => {
                        pane.detached.store(true, Ordering::SeqCst);
                        log::warn!("d2b pane requires reattach: {err}");
                        Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane.pane_id()));
                        break;
                    }
                }
            }
            if let Some(pane) = pane.upgrade() {
                if !pane.detached.swap(true, Ordering::SeqCst) {
                    Mux::notify_from_any_thread(MuxNotification::PaneOutput(pane.pane_id()));
                }
            }
        })
        .detach();
    }

    pub struct UnsupportedD2bTransport;

    impl D2bTransport for UnsupportedD2bTransport {
        fn discover(&self) -> TransportFuture<'_, D2bDiscovery> {
            Box::pin(async { Err(anyhow!("native d2b transport is not wired yet")) })
        }

        fn attach(&self, _request: D2bAttachRequest) -> TransportFuture<'_, D2bAttachedPane> {
            Box::pin(async { Err(anyhow!("native d2b transport is not wired yet")) })
        }
    }

    pub fn unsupported_domain(name: impl Into<String>) -> D2bDomain {
        D2bDomain::new(name, "unsupported", Arc::new(UnsupportedD2bTransport))
    }

    #[cfg(test)]
    mod test {
        use super::*;
        use d2b_toolkit_core::{NegotiatedCapabilities, PublicResponse, WorkloadOpResponse};
        use futures::io::{AsyncReadExt, Cursor};
        use parking_lot::Mutex;

        #[derive(Default)]
        struct FakeTransport {
            sessions: Vec<D2bSession>,
            status: Option<D2bTargetStatus>,
            attach_error: Option<D2bProviderError>,
            attach_requests: Arc<Mutex<Vec<D2bAttachRequest>>>,
            queue_depth: usize,
            event_depth: usize,
            detach_calls: Arc<Mutex<Vec<D2bDetachReason>>>,
            inputs: Arc<Mutex<Vec<Vec<u8>>>>,
            resizes: Arc<Mutex<Vec<TerminalSize>>>,
            pause_commands: bool,
            emit_disconnect: bool,
            emit_stale: bool,
            emit_drop: bool,
            emit_write_timeout: bool,
            latency: Duration,
        }

        impl FakeTransport {
            fn attached(&self, request: D2bAttachRequest) -> D2bAttachedPane {
                self.attach_requests.lock().push(request.clone());
                let queue_depth = if self.queue_depth == 0 {
                    DEFAULT_QUEUE_DEPTH
                } else {
                    self.queue_depth
                };
                let event_depth = if self.event_depth == 0 {
                    DEFAULT_EVENT_DEPTH
                } else {
                    self.event_depth
                };
                let (command_tx, command_rx) = async_channel::bounded(queue_depth);
                let (event_tx, event_rx) = async_channel::bounded(event_depth);
                let target = self
                    .status
                    .as_ref()
                    .map(|status| status.target().to_string())
                    .unwrap_or_else(|| "work".to_string());
                let session_id = request
                    .session_id
                    .clone()
                    .unwrap_or_else(|| "session-1".to_string());
                let correlation_id =
                    D2bCorrelationId::from_target_session("fake-pane", &target, &session_id);
                let handle = D2bPaneHandle {
                    target,
                    session_id,
                    pane_id: "pane-1".to_string(),
                    correlation_id: correlation_id.clone(),
                };
                if self.pause_commands {
                    smol::spawn(async move {
                        smol::Timer::after(Duration::from_secs(60)).await;
                        drop(command_rx);
                    })
                    .detach();
                } else {
                    let detach_calls = Arc::clone(&self.detach_calls);
                    let inputs = Arc::clone(&self.inputs);
                    let resizes = Arc::clone(&self.resizes);
                    let latency = self.latency;
                    let emit_disconnect = self.emit_disconnect;
                    let emit_stale = self.emit_stale;
                    let emit_drop = self.emit_drop;
                    let emit_write_timeout = self.emit_write_timeout;
                    smol::spawn(async move {
                        if latency > Duration::ZERO {
                            smol::Timer::after(latency).await;
                        }
                        if emit_disconnect {
                            let _ = event_tx
                                .send(D2bPaneEvent::ReattachRequired(
                                    D2bProviderError::Disconnected {
                                        operation: "fake disconnect",
                                        correlation: Some(correlation_id.clone()),
                                    },
                                ))
                                .await;
                            return;
                        }
                        if emit_stale {
                            let _ = event_tx
                                .send(D2bPaneEvent::ReattachRequired(
                                    D2bProviderError::StaleSession {
                                        operation: "fake stale session",
                                        correlation: correlation_id.clone(),
                                    },
                                ))
                                .await;
                            return;
                        }
                        if emit_drop {
                            let _ = event_tx
                                .send(D2bPaneEvent::ReattachRequired(
                                    D2bProviderError::DroppedOutput {
                                        correlation: correlation_id.clone(),
                                    },
                                ))
                                .await;
                            return;
                        }
                        while let Ok(command) = command_rx.recv().await {
                            match command {
                                D2bPaneCommand::Write { bytes } => {
                                    if emit_write_timeout {
                                        let _ = event_tx
                                            .send(D2bPaneEvent::ReattachRequired(
                                                D2bProviderError::Timeout {
                                                    operation: "fake write timeout",
                                                    correlation: Some(correlation_id.clone()),
                                                },
                                            ))
                                            .await;
                                        break;
                                    }
                                    inputs.lock().push(bytes)
                                }
                                D2bPaneCommand::Resize { size } => resizes.lock().push(size),
                                D2bPaneCommand::CloseAttach { reason } => {
                                    detach_calls.lock().push(reason);
                                    let _ = event_tx.try_send(D2bPaneEvent::Closed);
                                    break;
                                }
                            }
                        }
                    })
                    .detach();
                }
                D2bAttachedPane::new(handle, command_tx, event_rx)
            }
        }

        impl D2bTransport for FakeTransport {
            fn discover(&self) -> TransportFuture<'_, D2bDiscovery> {
                let sessions = self.sessions.clone();
                let status = self
                    .status
                    .clone()
                    .unwrap_or_else(|| D2bTargetStatus::legacy("work".to_string()));
                Box::pin(async move { Ok(D2bDiscovery { status, sessions }) })
            }

            fn attach(&self, request: D2bAttachRequest) -> TransportFuture<'_, D2bAttachedPane> {
                if let Some(err) = self.attach_error.clone() {
                    return Box::pin(async move { Err(anyhow::Error::new(err)) });
                }
                let pane = self.attached(request);
                Box::pin(async move { Ok(pane) })
            }
        }

        fn size() -> TerminalSize {
            TerminalSize {
                rows: 24,
                cols: 80,
                pixel_width: 640,
                pixel_height: 480,
                dpi: 96,
            }
        }

        fn unsafe_workload_fixture() -> WorkloadPublicSummary {
            // Mirrored from d2b-toolkit v0.2.0's public-workload-v3-v1 fixtures.
            let response: PublicResponse = serde_json::from_str(include_str!(
                "../test-data/public-workload-v3-v1/unsafe-local-list-response.json"
            ))
            .unwrap();
            match response {
                PublicResponse::Workload {
                    response: WorkloadOpResponse::List(mut result),
                    ..
                } => result.workloads.remove(0),
                _ => panic!("toolkit unsafe-local fixture had an unexpected shape"),
            }
        }

        fn handle() -> D2bPaneHandle {
            D2bPaneHandle {
                target: "work".to_string(),
                session_id: "session-1".to_string(),
                pane_id: "pane-1".to_string(),
                correlation_id: D2bCorrelationId::from_sensitive("test", "session-1"),
            }
        }

        fn attached_with_transport(transport: &FakeTransport) -> D2bAttachedPane {
            transport.attached(D2bAttachRequest {
                session_id: Some("session-1".to_string()),
                size: size(),
            })
        }

        #[test]
        fn shell_management_timeout_covers_daemon_guest_control_budget() {
            let config = D2bRuntimeConfig::default();
            assert_eq!(config.shell_management_timeout, Duration::from_secs(15));
            assert!(config.shell_management_timeout > config.read_timeout);
            assert!(config.shell_management_timeout > config.write_timeout);
        }

        #[test]
        fn native_transport_refuses_privileged_socket_before_connect() {
            let transport = NativeD2bTransport::new(
                "work",
                D2bRuntimeConfig {
                    socket_path: PathBuf::from("/run/d2b/priv.sock"),
                    ..D2bRuntimeConfig::default()
                },
            )
            .unwrap();
            let err = match smol::block_on(transport.connect_client()) {
                Ok(_) => panic!("priv socket should be refused"),
                Err(err) => err,
            };
            let rendered = err.to_string();
            assert!(rendered.contains("refused privileged broker socket"));
            assert!(!rendered.contains("/run/d2b/priv.sock"));
        }

        #[test]
        fn toolkit_fixture_exposes_unsafe_local_and_helper_posture() {
            let workload = unsafe_workload_fixture();
            let status = D2bTargetStatus::from_workload(&workload);

            assert_eq!(status.target(), "tools.host.d2b");
            assert_eq!(
                status.provider_kind,
                Some(WorkloadProviderKind::UnsafeLocal)
            );
            assert_eq!(
                status.isolation,
                Some(d2b_toolkit_core::IsolationPosture::UnsafeLocal)
            );
            assert_eq!(
                status.availability,
                Some(WorkloadAvailability::HelperUnavailable)
            );
            let warning = status.warning_text().unwrap();
            assert!(warning.contains("UNSAFE LOCAL — NO ISOLATION"));
            assert!(warning.contains("helper unavailable"));
            assert!(!status.is_shell_ready());
        }

        #[test]
        fn unsafe_local_feature_skew_fails_closed_before_shell_operations() {
            let mut status = D2bTargetStatus::new(
                "tools.host.d2b",
                WorkloadProviderKind::UnsafeLocal,
                d2b_toolkit_core::IsolationPosture::UnsafeLocal,
                WorkloadAvailability::Ready,
                true,
            )
            .unwrap();
            let capabilities = NegotiatedCapabilities::from_features([
                KnownFeatureFlag::ConfiguredLaunchV1.wire_value(),
                KnownFeatureFlag::UnsafeLocalProviderV1.wire_value(),
            ]);
            let mut client = PublicSocketClient::with_negotiated_capabilities(
                Cursor::new(Vec::new()),
                capabilities,
            );

            NativeD2bTransport::apply_unsafe_shell_requirement(&mut client, &mut status).unwrap();

            assert_eq!(
                status.required_feature(),
                Some(KnownFeatureFlag::UnsafeLocalShellV1)
            );
            let err = ensure_target_shell_ready(&status).unwrap_err();
            assert!(matches!(
                err,
                D2bProviderError::FeatureSkew {
                    feature: KnownFeatureFlag::UnsafeLocalShellV1
                }
            ));
        }

        #[test]
        fn unsafe_target_keeps_one_canonical_public_socket_shell_route() {
            let mut status = D2bTargetStatus::new(
                "tools.host.d2b",
                WorkloadProviderKind::UnsafeLocal,
                d2b_toolkit_core::IsolationPosture::UnsafeLocal,
                WorkloadAvailability::Ready,
                true,
            )
            .unwrap();
            let capabilities = NegotiatedCapabilities::from_features([
                KnownFeatureFlag::ConfiguredLaunchV1.wire_value(),
                KnownFeatureFlag::UnsafeLocalProviderV1.wire_value(),
                KnownFeatureFlag::UnsafeLocalShellV1.wire_value(),
            ]);
            let mut client = PublicSocketClient::with_negotiated_capabilities(
                Cursor::new(Vec::new()),
                capabilities,
            );

            NativeD2bTransport::apply_unsafe_shell_requirement(&mut client, &mut status).unwrap();
            ensure_target_shell_ready(&status).unwrap();
            let transport =
                NativeD2bTransport::new(status.target(), D2bRuntimeConfig::default()).unwrap();

            assert_eq!(transport.target, "tools.host.d2b");
            assert_eq!(
                transport.config.socket_path,
                PathBuf::from("/run/d2b/public.sock")
            );
        }

        #[test]
        fn debug_redacts_session_handle_shell_and_terminal_data_but_keeps_digest() {
            let session = D2bSession {
                id: "session-secret".to_string(),
                label: "quiet-otter".to_string(),
                target: "work".to_string(),
                workspace: Some("private".to_string()),
                state: ShellSessionState::Detached,
                attached: false,
                is_default: false,
                correlation_id: D2bCorrelationId::from_sensitive("session", "session-secret"),
            };
            let handle = handle();
            let attach = D2bAttachRequest {
                session_id: Some("session-secret".to_string()),
                size: size(),
            };
            let write = D2bPaneCommand::Write {
                bytes: b"terminal bytes and /home/alice".to_vec(),
            };

            for rendered in [format!("{session:?}"), format!("{handle:?}")] {
                assert!(!rendered.contains("session-secret"));
                assert!(!rendered.contains("quiet-otter"));
                assert!(!rendered.contains("session-1"));
                assert!(!rendered.contains("pane-1"));
                assert!(rendered.contains("redacted"));
                assert!(rendered.contains("d2b:"));
            }
            let rendered = format!("{attach:?}");
            assert!(!rendered.contains("session-secret"));
            assert!(rendered.contains("redacted"));
            let rendered = format!("{write:?}");
            assert!(rendered.contains("Write"));
            assert!(!rendered.contains("terminal bytes"));
            assert!(!rendered.contains("alice"));
        }

        fn wait_for<F>(mut predicate: F)
        where
            F: FnMut() -> bool,
        {
            for _ in 0..50 {
                if predicate() {
                    return;
                }
                smol::block_on(smol::Timer::after(Duration::from_millis(10)));
            }
            assert!(predicate());
        }

        #[test]
        fn discovery_uses_transport_without_d2bd() {
            let transport = Arc::new(FakeTransport {
                sessions: vec![D2bSession {
                    id: "session-1".to_string(),
                    label: "work".to_string(),
                    target: "work".to_string(),
                    workspace: None,
                    state: ShellSessionState::Detached,
                    attached: false,
                    is_default: false,
                    correlation_id: D2bCorrelationId::from_sensitive("session", "session-1"),
                }],
                ..Default::default()
            });
            let domain = D2bDomain::new("d2b", "work", transport);

            let sessions = smol::block_on(domain.discover_sessions()).unwrap();
            assert_eq!(sessions[0].id, "session-1");
        }

        #[test]
        fn domain_label_shows_unsafe_local_and_helper_unavailable() {
            let status = D2bTargetStatus::new(
                "tools.host.d2b",
                WorkloadProviderKind::UnsafeLocal,
                d2b_toolkit_core::IsolationPosture::UnsafeLocal,
                WorkloadAvailability::HelperUnavailable,
                true,
            )
            .unwrap();
            let domain = D2bDomain::new(
                "host-tools",
                "tools.host.d2b",
                Arc::new(FakeTransport {
                    status: Some(status),
                    ..Default::default()
                }),
            );

            let label = smol::block_on(domain.domain_label());
            assert!(label.contains("UNSAFE LOCAL — NO ISOLATION"));
            assert!(label.contains("helper unavailable"));
            assert!(label.contains("unavailable"));
        }

        #[test]
        fn attach_failure_is_typed() {
            let err = D2bProviderError::AttachFailed {
                kind: "guest-control-shell-timeout".to_string(),
            };
            let transport = FakeTransport {
                attach_error: Some(err),
                ..Default::default()
            };
            let result = smol::block_on(transport.attach(D2bAttachRequest {
                session_id: None,
                size: size(),
            }));
            let err = match result {
                Ok(_) => panic!("attach unexpectedly succeeded"),
                Err(err) => err,
            };
            assert!(err.is::<D2bProviderError>());
        }

        #[test]
        fn kill_detaches_without_destroying_session() {
            let transport = FakeTransport::default();
            let pane = D2bPane::new(1, size(), attached_with_transport(&transport));

            pane.kill();
            pane.kill();
            drop(pane);

            wait_for(|| !transport.detach_calls.lock().is_empty());
            assert_eq!(
                *transport.detach_calls.lock(),
                vec![D2bDetachReason::PaneKill]
            );
        }

        #[test]
        fn drop_detaches_if_pane_was_not_closed() {
            let transport = FakeTransport::default();
            let pane = D2bPane::new(1, size(), attached_with_transport(&transport));

            drop(pane);

            wait_for(|| !transport.detach_calls.lock().is_empty());
            assert_eq!(*transport.detach_calls.lock(), vec![D2bDetachReason::Drop]);
        }

        #[test]
        fn explicit_close_uses_detach_not_kill() {
            let transport = FakeTransport::default();
            let pane = D2bPane::new(1, size(), attached_with_transport(&transport));

            pane.close_non_destructive();
            drop(pane);

            wait_for(|| !transport.detach_calls.lock().is_empty());
            assert_eq!(
                *transport.detach_calls.lock(),
                vec![D2bDetachReason::PaneClose]
            );
        }

        #[test]
        fn domain_detach_detaches_tracked_panes() {
            let transport = Arc::new(FakeTransport::default());
            let domain = D2bDomain::new("d2b", "work", transport.clone());
            let pane = D2bPane::new(1, size(), attached_with_transport(&transport));
            domain.panes.lock().push(Arc::downgrade(&pane));

            domain.detach().unwrap();
            drop(pane);

            wait_for(|| !transport.detach_calls.lock().is_empty());
            assert_eq!(
                *transport.detach_calls.lock(),
                vec![D2bDetachReason::DomainDetach]
            );
        }

        #[test]
        fn paste_and_resize_use_nonblocking_actor_queue() {
            let transport = FakeTransport::default();
            let pane = D2bPane::new(1, size(), attached_with_transport(&transport));

            pane.send_paste("hello").unwrap();
            pane.resize(TerminalSize {
                rows: 40,
                cols: 100,
                pixel_width: 800,
                pixel_height: 600,
                dpi: 96,
            })
            .unwrap();

            wait_for(|| !transport.inputs.lock().is_empty());
            wait_for(|| !transport.resizes.lock().is_empty());
            assert_eq!(*transport.inputs.lock(), vec![b"hello".to_vec()]);
            assert_eq!(transport.resizes.lock()[0].rows, 40);
        }

        #[test]
        fn full_pane_queue_returns_typed_backpressure_without_blocking() {
            let transport = FakeTransport {
                queue_depth: 1,
                pause_commands: true,
                ..Default::default()
            };
            let pane = D2bPane::new(1, size(), attached_with_transport(&transport));

            pane.send_paste("first").unwrap();
            let err = pane.send_paste("second").unwrap_err();
            assert!(err.is::<D2bProviderError>());
            assert!(format!("{err}").contains("queue full"));
        }

        #[test]
        fn fake_transport_simulates_disconnect_stale_drop_write_timeout_and_latency() {
            for (transport, expected) in [
                (
                    FakeTransport {
                        emit_disconnect: true,
                        latency: Duration::from_millis(1),
                        ..Default::default()
                    },
                    "disconnected",
                ),
                (
                    FakeTransport {
                        emit_stale: true,
                        ..Default::default()
                    },
                    "stale session",
                ),
                (
                    FakeTransport {
                        emit_drop: true,
                        ..Default::default()
                    },
                    "dropped terminal output",
                ),
                (
                    FakeTransport {
                        emit_write_timeout: true,
                        ..Default::default()
                    },
                    "write timeout",
                ),
            ] {
                let pane = D2bPane::new(1, size(), attached_with_transport(&transport));
                if expected == "write timeout" {
                    pane.send_paste("trigger").unwrap();
                }
                wait_for(|| pane.is_dead());
                assert!(pane.is_dead(), "{}", expected);
            }
        }

        #[test]
        fn d2b_title_format_suffixes_target_and_session_once() {
            assert_eq!(
                super::super::d2b_tab_title("work", "build", ""),
                "[work:build]"
            );
            assert_eq!(
                super::super::d2b_tab_title("work", "build", "nvim"),
                "nvim [work:build]"
            );
            assert_eq!(
                super::super::d2b_tab_title("work", "build", "wezterm"),
                "[work:build]"
            );
            assert_eq!(
                super::super::d2b_tab_title("work", "build", "nvim [work:build]"),
                "nvim [work:build]"
            );
            assert_eq!(
                super::super::d2b_tab_title("work\x1b[31m", "\x07", "nvim\x1b]0;secret\x07"),
                "nvim [work:unnamed]"
            );
        }

        #[test]
        fn d2b_title_width_truncates_only_the_untrusted_guest_title() {
            let title = super::super::d2b_tab_title_for_width(
                "work",
                "build",
                "[fake:admin] malicious title padded to hide identity",
                28,
            );
            assert!(title.ends_with(" [work:build]"), "{title}");
            assert!(termwiz::cell::unicode_column_width(&title, None) <= 28);

            let repeated = super::super::d2b_tab_title_for_width(
                "work",
                "build",
                "[fake:admin] padded [work:build]",
                20,
            );
            assert!(repeated.ends_with(" [work:build]"), "{repeated}");
            assert_eq!(repeated.matches("[work:build]").count(), 1);
        }

        #[test]
        fn display_label_sanitizes_controls_and_has_placeholder() {
            assert_eq!(
                super::super::sanitize_display_label("build\x1b[31m"),
                "build"
            );
            assert_eq!(
                super::super::sanitize_display_label("\x07\x1b[31m"),
                "unnamed"
            );
            let long = "a".repeat(128);
            assert_eq!(super::super::sanitize_display_label(&long).len(), 96);
        }

        #[test]
        fn shell_name_validation_matches_d2b_grammar() {
            for name in ["default", "build_1", "a.b-c", "9"] {
                assert!(super::super::validate_shell_name(name).is_ok(), "{}", name);
            }
            for name in ["", "-bad", "has space", "slash/name", "{tmpl}"] {
                assert!(super::super::validate_shell_name(name).is_err(), "{}", name);
            }
        }

        #[test]
        fn spawn_command_env_selects_d2b_session_without_subprocess_bridge() {
            let transport = FakeTransport::default();
            let mut command = portable_pty::CommandBuilder::new_default_prog();
            command.env(super::super::D2B_SHELL_NAME_ENV, "build");
            let session_id = d2b_session_from_command(Some(command)).unwrap();

            let attached = smol::block_on(transport.attach(D2bAttachRequest {
                session_id,
                size: size(),
            }))
            .unwrap();
            let pane = D2bPane::new(1, size(), attached);

            assert_eq!(
                transport.attach_requests.lock()[0].session_id.as_deref(),
                Some("build")
            );
            assert_eq!(pane.get_title(), "[work:build]");
        }

        #[test]
        fn canonical_dotted_target_normalizes_and_generates_bounded_shell_name() {
            assert_eq!(
                super::super::normalize_d2b_target("d2b://tools.host.d2b").unwrap(),
                "tools.host.d2b"
            );
            let name = super::super::friendly_session_name("tools.host.d2b", &[]);
            assert!(super::super::validate_shell_name(&name).is_ok());
            assert!(name.len() <= 64);
            assert!(!name.contains("tools.host.d2b"));
            assert_eq!(
                super::super::friendly_session_name("work", &[]),
                "work-shell"
            );
        }

        #[test]
        fn bound_vm_alias_normalizes_to_target_and_conflicts_fail_closed() {
            assert_eq!(
                super::super::resolve_bound_target_aliases(None, Some("work")).unwrap(),
                Some("work".to_string())
            );
            assert_eq!(
                super::super::resolve_bound_target_aliases(
                    Some("tools.host.d2b"),
                    Some("d2b://tools.host.d2b")
                )
                .unwrap(),
                Some("tools.host.d2b".to_string())
            );
            let err =
                super::super::resolve_bound_target_aliases(Some("tools.host.d2b"), Some("work"))
                    .unwrap_err();
            assert!(err.contains(super::super::D2B_BOUND_TARGET_ENV));
            assert!(err.contains(super::super::D2B_BOUND_VM_ENV));
            assert!(!err.contains("tools.host.d2b"));
        }

        #[test]
        fn target_socket_and_domain_keys_are_bounded_stable_and_non_reversible() {
            let base = PathBuf::from("runtime");
            let target = "tools.host.d2b";
            let path = super::super::target_mux_socket_path(&base, target);
            let repeated = super::super::target_mux_socket_path(&base, target);
            let with_scheme = super::super::target_mux_socket_path(&base, "d2b://tools.host.d2b");
            let other = super::super::target_mux_socket_path(&base, "corp.work.d2b");
            let domain_key = super::super::target_domain_key(target);

            assert_eq!(path, repeated);
            assert_eq!(path, with_scheme);
            assert_ne!(path, other);
            assert!(!path.to_string_lossy().contains(target));
            assert!(!domain_key.contains(target));
            assert_eq!(domain_key.len(), 36);
            assert!(path.to_string_lossy().len() < 80);
            assert_eq!(
                super::super::vm_mux_socket_path(&base, "work"),
                super::super::target_mux_socket_path(&base, "work")
            );
        }

        #[test]
        fn reconnect_identity_uses_canonical_target_and_keeps_vm_user_var_alias() {
            let status = D2bTargetStatus::new(
                "tools.host.d2b",
                WorkloadProviderKind::UnsafeLocal,
                d2b_toolkit_core::IsolationPosture::UnsafeLocal,
                WorkloadAvailability::Ready,
                true,
            )
            .unwrap();
            let transport = FakeTransport {
                status: Some(status),
                ..Default::default()
            };
            let pane = D2bPane::new(1, size(), attached_with_transport(&transport));
            let vars = pane.copy_user_vars();

            assert_eq!(
                vars.get("weezterm.d2b.target").map(String::as_str),
                Some("tools.host.d2b")
            );
            assert_eq!(
                vars.get("weezterm.d2b.vm").map(String::as_str),
                Some("tools.host.d2b")
            );
            assert_eq!(pane.get_title(), "[tools.host.d2b:session-1]");
            assert_eq!(pane.trusted_d2b_target(), Some("tools.host.d2b"));
            assert_eq!(pane.trusted_d2b_session(), Some("session-1"));

            let first = D2bCorrelationId::from_target_session(
                "attached-shell",
                "tools.host.d2b",
                "session-1",
            );
            let second = D2bCorrelationId::from_target_session(
                "attached-shell",
                "tools.personal.d2b",
                "session-1",
            );
            assert_ne!(first, second);
        }

        #[test]
        fn idle_d2b_socket_yields_to_the_timeout_reactor() {
            let (client, _server) = nix::sys::socket::socketpair(
                nix::sys::socket::AddressFamily::Unix,
                nix::sys::socket::SockType::SeqPacket,
                None,
                nix::sys::socket::SockFlag::SOCK_CLOEXEC
                    | nix::sys::socket::SockFlag::SOCK_NONBLOCK,
            )
            .unwrap();
            let mut socket = D2bSocket::new(Socket::from(client)).unwrap();
            let mut byte = [0_u8; 1];
            let result = smol::block_on(timeout_result(
                "testing idle socket",
                None,
                Duration::from_millis(20),
                socket.read(&mut byte),
            ));
            assert!(matches!(result, Err(D2bProviderError::Timeout { .. })));
        }

        #[test]
        fn oversized_seqpacket_is_rejected_via_msg_trunc_length() {
            let (client, server) = nix::sys::socket::socketpair(
                nix::sys::socket::AddressFamily::Unix,
                nix::sys::socket::SockType::SeqPacket,
                None,
                nix::sys::socket::SockFlag::SOCK_CLOEXEC
                    | nix::sys::socket::SockFlag::SOCK_NONBLOCK,
            )
            .unwrap();
            let mut socket = D2bSocket::with_packet_limit(Socket::from(client), 8).unwrap();
            let packet = [0_u8; 9];
            assert_eq!(
                nix::sys::socket::send(
                    server.as_raw_fd(),
                    &packet,
                    nix::sys::socket::MsgFlags::empty(),
                )
                .unwrap(),
                packet.len()
            );
            let mut byte = [0_u8; 1];
            let error = smol::block_on(socket.read(&mut byte)).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use crate::domain::{alloc_domain_id, Domain, DomainId, DomainState, SplitSource};
    use crate::pane::{Pane, PaneId};
    use crate::tab::{SplitRequest, Tab, TabId};
    use crate::window::WindowId;
    use async_trait::async_trait;
    use std::sync::Arc;
    use wezterm_term::TerminalSize;

    pub struct D2bDomain;

    impl D2bDomain {
        pub fn unsupported() -> anyhow::Result<Self> {
            anyhow::bail!("native d2b domains are only supported on Linux")
        }
    }

    #[async_trait(?Send)]
    impl Domain for D2bDomain {
        async fn spawn_pane(
            &self,
            _size: TerminalSize,
            _command: Option<portable_pty::CommandBuilder>,
            _command_dir: Option<String>,
        ) -> anyhow::Result<Arc<dyn Pane>> {
            anyhow::bail!("native d2b domains are only supported on Linux")
        }

        async fn spawn(
            &self,
            _size: TerminalSize,
            _command: Option<portable_pty::CommandBuilder>,
            _command_dir: Option<String>,
            _window: WindowId,
        ) -> anyhow::Result<Arc<Tab>> {
            anyhow::bail!("native d2b domains are only supported on Linux")
        }

        async fn split_pane(
            &self,
            _source: SplitSource,
            _tab: TabId,
            _pane_id: PaneId,
            _split_request: SplitRequest,
        ) -> anyhow::Result<Arc<dyn Pane>> {
            anyhow::bail!("native d2b domains are only supported on Linux")
        }

        fn detachable(&self) -> bool {
            false
        }

        fn domain_id(&self) -> DomainId {
            alloc_domain_id()
        }

        fn domain_name(&self) -> &str {
            "d2b"
        }

        async fn attach(&self, _window_id: Option<WindowId>) -> anyhow::Result<()> {
            anyhow::bail!("native d2b domains are only supported on Linux")
        }

        fn detach(&self) -> anyhow::Result<()> {
            anyhow::bail!("native d2b domains are only supported on Linux")
        }

        fn state(&self) -> DomainState {
            DomainState::Detached
        }
    }
}

pub use imp::*;
