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
    use crate::Mux;
    use anyhow::{anyhow, bail};
    use async_trait::async_trait;
    use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
    use rangeset::RangeSet;
    use std::future::Future;
    use std::io::{Error as IoError, ErrorKind, Write};
    use std::ops::Range;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Weak};
    use termwiz::surface::{Line, SequenceNo};
    use url::Url;
    use wezterm_term::color::ColorPalette;
    use wezterm_term::{
        KeyCode, KeyModifiers, MouseEvent, StableRowIndex, TerminalConfiguration, TerminalSize,
    };

    pub type TransportFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

    #[derive(Clone, Eq, PartialEq)]
    pub struct D2bSession {
        pub id: String,
        pub label: String,
        pub vm_name: String,
        pub workspace: Option<String>,
    }

    impl std::fmt::Debug for D2bSession {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("D2bSession")
                .field("id", &"<redacted>")
                .field("label", &"<redacted>")
                .field("vm_name", &"<redacted>")
                .field("workspace", &self.workspace.as_ref().map(|_| "<redacted>"))
                .finish()
        }
    }

    #[derive(Clone, Eq, PartialEq)]
    pub struct D2bPaneHandle {
        pub session_id: String,
        pub pane_id: String,
    }

    impl std::fmt::Debug for D2bPaneHandle {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("D2bPaneHandle")
                .field("session_id", &"<redacted>")
                .field("pane_id", &"<redacted>")
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

    pub trait D2bTransport: Send + Sync {
        fn discover_sessions(&self) -> TransportFuture<'_, Vec<D2bSession>>;

        fn attach(&self, request: D2bAttachRequest) -> TransportFuture<'_, D2bPaneHandle>;

        fn detach(
            &self,
            handle: &D2bPaneHandle,
            reason: D2bDetachReason,
        ) -> TransportFuture<'_, ()>;

        fn send_input(&self, handle: &D2bPaneHandle, input: Vec<u8>) -> TransportFuture<'_, ()>;

        fn resize(&self, handle: &D2bPaneHandle, size: TerminalSize) -> TransportFuture<'_, ()>;
    }

    pub struct D2bDomain {
        id: DomainId,
        name: String,
        transport: Arc<dyn D2bTransport>,
        state: Mutex<DomainState>,
        panes: Mutex<Vec<Weak<D2bPane>>>,
    }

    impl D2bDomain {
        pub fn new(name: impl Into<String>, transport: Arc<dyn D2bTransport>) -> Self {
            Self {
                id: alloc_domain_id(),
                name: name.into(),
                transport,
                state: Mutex::new(DomainState::Detached),
                panes: Mutex::new(Vec::new()),
            }
        }

        pub async fn discover_sessions(&self) -> anyhow::Result<Vec<D2bSession>> {
            self.transport.discover_sessions().await
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
            if command.is_some() {
                bail!("d2b domains attach existing sessions; command spawning is unsupported");
            }

            let handle = self
                .transport
                .attach(D2bAttachRequest {
                    session_id: None,
                    size,
                })
                .await?;

            let concrete_pane = Arc::new(D2bPane::new(
                self.id,
                size,
                handle,
                Arc::clone(&self.transport),
            ));
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
            match self.discover_sessions().await {
                Ok(sessions) if sessions.is_empty() => format!("d2b `{}` — no sessions", self.name),
                Ok(sessions) => format!("d2b `{}` — {} session(s)", self.name, sessions.len()),
                Err(err) => {
                    log::debug!("d2b session discovery failed for {}: {err:#}", self.name);
                    format!("d2b `{}` — unavailable", self.name)
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
        transport: Arc<dyn D2bTransport>,
        input_tx: mpsc::Sender<Vec<u8>>,
        detached: AtomicBool,
    }

    impl D2bPane {
        pub fn new(
            domain_id: DomainId,
            size: TerminalSize,
            handle: D2bPaneHandle,
            transport: Arc<dyn D2bTransport>,
        ) -> Self {
            let pane_id = alloc_pane_id();
            let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
            spawn_input_forwarder(Arc::clone(&transport), handle.clone(), input_rx);

            let terminal = wezterm_term::Terminal::new(
                size,
                Arc::new(config::TermConfig::new()),
                config::branding::APP_NAME_DISPLAY,
                config::wezterm_version(),
                Box::new(D2bInputWriter {
                    input_tx: input_tx.clone(),
                }),
            );

            Self {
                pane_id,
                domain_id,
                terminal: Mutex::new(terminal),
                writer: Mutex::new(Box::new(D2bInputWriter {
                    input_tx: input_tx.clone(),
                })),
                handle,
                transport,
                input_tx,
                detached: AtomicBool::new(false),
            }
        }

        fn detach_non_destructive(&self, reason: D2bDetachReason) {
            if self.detached.swap(true, Ordering::SeqCst) {
                return;
            }
            let transport = Arc::clone(&self.transport);
            let handle = self.handle.clone();
            smol::spawn(async move {
                if let Err(err) = transport.detach(&handle, reason).await {
                    log::warn!("failed to detach d2b pane: {err:#}");
                }
            })
            .detach();
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
            self.terminal.lock().get_title().to_string()
        }

        fn send_paste(&self, text: &str) -> anyhow::Result<()> {
            self.input_tx
                .send(text.as_bytes().to_vec())
                .map_err(|err| anyhow!("d2b input queue closed: {err}"))
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
            let transport = Arc::clone(&self.transport);
            let handle = self.handle.clone();
            smol::spawn(async move {
                if let Err(err) = transport.resize(&handle, size).await {
                    log::warn!("failed to resize d2b pane: {err:#}");
                }
            })
            .detach();
            Ok(())
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
    }

    impl Drop for D2bPane {
        fn drop(&mut self) {
            self.detach_non_destructive(D2bDetachReason::Drop);
        }
    }

    struct D2bInputWriter {
        input_tx: mpsc::Sender<Vec<u8>>,
    }

    impl Write for D2bInputWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.input_tx.send(buf.to_vec()).map_err(|err| {
                IoError::new(
                    ErrorKind::BrokenPipe,
                    format!("d2b input queue closed: {err}"),
                )
            })?;
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn spawn_input_forwarder(
        transport: Arc<dyn D2bTransport>,
        handle: D2bPaneHandle,
        input_rx: mpsc::Receiver<Vec<u8>>,
    ) {
        std::thread::Builder::new()
            .name("weezterm-d2b-input".to_string())
            .spawn(move || {
                while let Ok(input) = input_rx.recv() {
                    if let Err(err) = smol::block_on(transport.send_input(&handle, input)) {
                        log::warn!("d2b input forward failed: {err:#}");
                        break;
                    }
                }
            })
            .expect("spawn d2b input forwarder");
    }

    pub struct UnsupportedD2bTransport;

    impl D2bTransport for UnsupportedD2bTransport {
        fn discover_sessions(&self) -> TransportFuture<'_, Vec<D2bSession>> {
            Box::pin(async { Err(anyhow!("native d2b transport is not wired yet")) })
        }

        fn attach(&self, _request: D2bAttachRequest) -> TransportFuture<'_, D2bPaneHandle> {
            Box::pin(async { Err(anyhow!("native d2b transport is not wired yet")) })
        }

        fn detach(
            &self,
            _handle: &D2bPaneHandle,
            _reason: D2bDetachReason,
        ) -> TransportFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }

        fn send_input(&self, _handle: &D2bPaneHandle, _input: Vec<u8>) -> TransportFuture<'_, ()> {
            Box::pin(async { Err(anyhow!("native d2b transport is not wired yet")) })
        }

        fn resize(&self, _handle: &D2bPaneHandle, _size: TerminalSize) -> TransportFuture<'_, ()> {
            Box::pin(async { Err(anyhow!("native d2b transport is not wired yet")) })
        }
    }

    pub fn unsupported_domain(name: impl Into<String>) -> D2bDomain {
        D2bDomain::new(name, Arc::new(UnsupportedD2bTransport))
    }

    #[cfg(test)]
    mod test {
        use super::*;
        use parking_lot::Mutex;

        #[derive(Default)]
        struct FakeTransport {
            sessions: Vec<D2bSession>,
            detach_calls: Mutex<Vec<D2bDetachReason>>,
            inputs: Mutex<Vec<Vec<u8>>>,
            resizes: Mutex<Vec<TerminalSize>>,
        }

        impl D2bTransport for FakeTransport {
            fn discover_sessions(&self) -> TransportFuture<'_, Vec<D2bSession>> {
                let sessions = self.sessions.clone();
                Box::pin(async move { Ok(sessions) })
            }

            fn attach(&self, request: D2bAttachRequest) -> TransportFuture<'_, D2bPaneHandle> {
                Box::pin(async move {
                    Ok(D2bPaneHandle {
                        session_id: request
                            .session_id
                            .unwrap_or_else(|| "session-1".to_string()),
                        pane_id: "pane-1".to_string(),
                    })
                })
            }

            fn detach(
                &self,
                _handle: &D2bPaneHandle,
                reason: D2bDetachReason,
            ) -> TransportFuture<'_, ()> {
                Box::pin(async move {
                    self.detach_calls.lock().push(reason);
                    Ok(())
                })
            }

            fn send_input(
                &self,
                _handle: &D2bPaneHandle,
                input: Vec<u8>,
            ) -> TransportFuture<'_, ()> {
                Box::pin(async move {
                    self.inputs.lock().push(input);
                    Ok(())
                })
            }

            fn resize(
                &self,
                _handle: &D2bPaneHandle,
                size: TerminalSize,
            ) -> TransportFuture<'_, ()> {
                Box::pin(async move {
                    self.resizes.lock().push(size);
                    Ok(())
                })
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

        fn handle() -> D2bPaneHandle {
            D2bPaneHandle {
                session_id: "session-1".to_string(),
                pane_id: "pane-1".to_string(),
            }
        }

        #[test]
        fn debug_redacts_session_and_handle_data() {
            let session = D2bSession {
                id: "session-secret".to_string(),
                label: "quiet-otter".to_string(),
                vm_name: "work".to_string(),
                workspace: Some("private".to_string()),
            };
            let handle = handle();
            let attach = D2bAttachRequest {
                session_id: Some("session-secret".to_string()),
                size: size(),
            };

            for rendered in [
                format!("{session:?}"),
                format!("{handle:?}"),
                format!("{attach:?}"),
            ] {
                assert!(!rendered.contains("session-secret"));
                assert!(!rendered.contains("quiet-otter"));
                assert!(rendered.contains("redacted"));
            }
        }

        fn wait_for<F>(mut predicate: F)
        where
            F: FnMut() -> bool,
        {
            for _ in 0..50 {
                if predicate() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(predicate());
        }

        #[test]
        fn discovery_uses_transport_without_d2bd() {
            let transport = Arc::new(FakeTransport {
                sessions: vec![D2bSession {
                    id: "session-1".to_string(),
                    label: "work".to_string(),
                    vm_name: "work".to_string(),
                    workspace: None,
                }],
                ..Default::default()
            });
            let domain = D2bDomain::new("d2b", transport);

            let sessions = smol::block_on(domain.discover_sessions()).unwrap();
            assert_eq!(sessions[0].id, "session-1");
        }

        #[test]
        fn kill_detaches_without_destroying_session() {
            let transport = Arc::new(FakeTransport::default());
            let pane = D2bPane::new(1, size(), handle(), transport.clone());

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
            let transport = Arc::new(FakeTransport::default());
            let pane = D2bPane::new(1, size(), handle(), transport.clone());

            drop(pane);

            wait_for(|| !transport.detach_calls.lock().is_empty());
            assert_eq!(*transport.detach_calls.lock(), vec![D2bDetachReason::Drop]);
        }

        #[test]
        fn explicit_close_uses_detach_not_kill() {
            let transport = Arc::new(FakeTransport::default());
            let pane = D2bPane::new(1, size(), handle(), transport.clone());

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
            let domain = D2bDomain::new("d2b", transport.clone());
            let pane = Arc::new(D2bPane::new(1, size(), handle(), transport.clone()));
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
        fn paste_and_resize_use_transport() {
            let transport = Arc::new(FakeTransport::default());
            let pane = D2bPane::new(1, size(), handle(), transport.clone());

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
