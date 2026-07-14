use crate::overlay::quickselect;
use crate::termwindow::TermWindowNotif;
use config::keyassignment::KeyAssignment;
use mux::d2b::{
    friendly_session_name, sanitize_display_label, validate_shell_name, D2bDomain, D2bSession,
    D2bTargetStatus, ShellSessionState,
};
use mux::domain::Domain;
use mux::termwiztermtab::TermWizTerminal;
use mux::Mux;
use termwiz::cell::{AttributeChange, CellAttributes, Intensity};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;
use termwiz_funcs::truncate_right;
use window::WindowOps;

const ROW_OVERHEAD: usize = 3;
const ALPHABET: &str = "1234567890abcdefghilmnopqrstuvwxyz";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bTargetScope {
    pub domain_name: String,
    pub target: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerKind {
    AllTargets,
    CurrentTarget,
}

#[derive(Clone, PartialEq, Eq)]
pub struct D2bTargetPickerEntry {
    pub domain_name: String,
    pub target: String,
    pub status: Option<D2bTargetStatus>,
    pub unavailable_reason: Option<String>,
    pub sessions: Vec<D2bSession>,
    pub generated_name: String,
}

impl D2bTargetPickerEntry {
    fn available(&self) -> bool {
        self.unavailable_reason.is_none()
            && self
                .status
                .as_ref()
                .map(D2bTargetStatus::is_shell_ready)
                .unwrap_or(false)
    }

    fn warning_text(&self) -> Option<String> {
        self.unavailable_reason
            .clone()
            .or_else(|| self.status.as_ref().and_then(D2bTargetStatus::warning_text))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EntryAction {
    Open {
        domain: String,
        name: String,
    },
    Manual {
        domain: String,
        default_name: String,
    },
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    label: String,
    disabled: bool,
    action: EntryAction,
}

#[derive(Clone)]
pub struct D2bLauncherArgs {
    pane_id: mux::pane::PaneId,
    targets: Vec<D2bTargetPickerEntry>,
    kind: PickerKind,
}

impl D2bLauncherArgs {
    pub async fn new(pane_id: mux::pane::PaneId) -> Self {
        Self::collect(pane_id, None).await
    }

    pub async fn new_scoped(pane_id: mux::pane::PaneId, scope: D2bTargetScope) -> Self {
        Self::collect(pane_id, Some(scope)).await
    }

    async fn collect(pane_id: mux::pane::PaneId, scope: Option<D2bTargetScope>) -> Self {
        let mux = Mux::get();
        let mut targets = vec![];
        for domain in mux.iter_domains() {
            let Some(d2b) = domain.downcast_ref::<D2bDomain>() else {
                continue;
            };
            let domain_name = d2b.domain_name().to_string();
            if scope
                .as_ref()
                .is_some_and(|scope| scope.domain_name != domain_name)
            {
                continue;
            }
            let configured_target = d2b.target().to_string();
            match d2b.discover().await {
                Ok(discovery) => {
                    let names = discovery
                        .sessions
                        .iter()
                        .map(|session| session.id.clone())
                        .collect::<Vec<_>>();
                    let target = discovery.status.target().to_string();
                    if scope.as_ref().is_some_and(|scope| scope.target != target) {
                        let trusted_target = scope
                            .as_ref()
                            .map(|scope| scope.target.clone())
                            .unwrap_or(configured_target);
                        targets.push(D2bTargetPickerEntry {
                            domain_name,
                            generated_name: friendly_session_name(&trusted_target, &names),
                            target: trusted_target,
                            status: None,
                            unavailable_reason: Some(
                                "trusted target identity mismatch".to_string(),
                            ),
                            sessions: vec![],
                        });
                        continue;
                    }
                    let generated_name = friendly_session_name(&target, &names);
                    targets.push(D2bTargetPickerEntry {
                        domain_name,
                        target,
                        status: Some(discovery.status),
                        unavailable_reason: None,
                        sessions: discovery.sessions,
                        generated_name,
                    });
                }
                Err(err) => {
                    log::debug!("d2b discovery failed: {err:#}");
                    let target = scope
                        .as_ref()
                        .map(|scope| scope.target.clone())
                        .unwrap_or(configured_target);
                    targets.push(D2bTargetPickerEntry {
                        domain_name,
                        generated_name: friendly_session_name(&target, &[]),
                        target,
                        status: None,
                        unavailable_reason: Some(sanitize_display_label(&err.to_string())),
                        sessions: vec![],
                    });
                }
            }
        }
        if let Some(scope) = &scope {
            if targets.is_empty() {
                targets.push(D2bTargetPickerEntry {
                    domain_name: scope.domain_name.clone(),
                    target: scope.target.clone(),
                    status: None,
                    unavailable_reason: Some("trusted d2b domain is unavailable".to_string()),
                    sessions: vec![],
                    generated_name: friendly_session_name(&scope.target, &[]),
                });
            }
        }
        targets.sort_by(|a, b| {
            a.target
                .cmp(&b.target)
                .then(a.domain_name.cmp(&b.domain_name))
        });
        Self {
            pane_id,
            targets,
            kind: if scope.is_some() {
                PickerKind::CurrentTarget
            } else {
                PickerKind::AllTargets
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Pick,
    Prompt {
        domain: String,
        input: String,
        error: Option<String>,
    },
}

struct State {
    args: D2bLauncherArgs,
    window: ::window::Window,
    entries: Vec<Entry>,
    active_idx: usize,
    top_row: usize,
    max_items: usize,
    labels: Vec<String>,
    selection: String,
    mode: Mode,
}

impl State {
    fn new(args: D2bLauncherArgs, window: ::window::Window) -> Self {
        Self {
            entries: build_entries(&args.targets, args.kind),
            args,
            window,
            active_idx: 0,
            top_row: 0,
            max_items: 0,
            labels: vec![],
            selection: String::new(),
            mode: Mode::Pick,
        }
    }

    fn render(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        match self.mode.clone() {
            Mode::Pick => self.render_picker(term),
            Mode::Prompt { input, error, .. } => self.render_prompt(term, &input, error.as_deref()),
        }
    }

    fn render_picker(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        let size = term.get_screen_size()?;
        let max_width = size.cols.saturating_sub(6);
        let max_items = size.rows.saturating_sub(ROW_OVERHEAD);
        if max_items != self.max_items {
            self.labels = quickselect::compute_labels_for_alphabet_with_preserved_case(
                ALPHABET,
                self.entries.len().min(max_items + 1),
            );
            self.max_items = max_items;
        }

        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(0),
            },
            Change::Text(match self.args.kind {
                PickerKind::AllTargets => {
                    "d2b sessions: Enter=open  n=name new  Esc=cancel\r\n".to_string()
                }
                PickerKind::CurrentTarget => "d2b shells: Enter=open  Esc=cancel\r\n".to_string(),
            }),
            Change::AllAttributes(CellAttributes::default()),
        ];
        let max_label_len = self.labels.iter().map(|s| s.len()).max().unwrap_or(0);
        let mut labels = self.labels.iter();
        for (row_num, (entry_idx, entry)) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.top_row)
            .enumerate()
        {
            if row_num > max_items {
                break;
            }
            if entry_idx == self.active_idx {
                changes.push(AttributeChange::Reverse(true).into());
            }
            if let Some(label) = labels.next() {
                changes.push(Change::Text(format!(" {label:>max_label_len$}. ")));
            } else {
                changes.push(Change::Text(" ".repeat(max_label_len + 3)));
            }
            if entry.disabled {
                changes.push(AttributeChange::Intensity(Intensity::Half).into());
            }
            changes.push(Change::Text(truncate_right(&entry.label, max_width)));
            if entry.disabled {
                changes.push(AttributeChange::Intensity(Intensity::Normal).into());
            }
            if entry_idx == self.active_idx {
                changes.push(AttributeChange::Reverse(false).into());
            }
            changes.push(Change::AllAttributes(CellAttributes::default()));
            changes.push(Change::Text("\r\n".to_string()));
        }
        term.render(&changes)
    }

    fn render_prompt(
        &self,
        term: &mut TermWizTerminal,
        input: &str,
        error: Option<&str>,
    ) -> termwiz::Result<()> {
        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(0),
            },
            Change::Text(
                "Name new d2b shell (1-64 bytes, [A-Za-z0-9._-]); Enter=open Esc=cancel\r\n"
                    .to_string(),
            ),
            Change::Text(format!("> {input}")),
        ];
        if let Some(error) = error {
            changes.push(Change::Text(format!("\r\nInvalid name: {error}")));
        }
        term.render(&changes)
    }

    fn move_up(&mut self) {
        self.active_idx = self.active_idx.saturating_sub(1);
        if self.active_idx < self.top_row {
            self.top_row = self.active_idx;
        }
    }

    fn move_down(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.active_idx = (self.active_idx + 1).min(self.entries.len() - 1);
        if self.active_idx > self.top_row + self.max_items {
            self.top_row = self.active_idx.saturating_sub(self.max_items);
        }
    }

    fn open(&self, domain: String, name: String) {
        self.window.notify(TermWindowNotif::PerformAssignment {
            pane_id: self.args.pane_id,
            assignment: KeyAssignment::D2bOpenSession {
                domain,
                name: Some(name),
            },
            tx: None,
        });
    }

    fn launch(&mut self, active_idx: usize) -> bool {
        let Some(entry) = self.entries.get(active_idx).cloned() else {
            return false;
        };
        match entry.action {
            EntryAction::Open { domain, name } => {
                self.open(domain, name);
                true
            }
            EntryAction::Manual {
                domain,
                default_name,
            } => {
                self.mode = Mode::Prompt {
                    domain,
                    input: default_name,
                    error: None,
                };
                false
            }
            EntryAction::Disabled => false,
        }
    }

    fn run_loop(&mut self, term: &mut TermWizTerminal) -> anyhow::Result<()> {
        while let Ok(Some(event)) = term.poll_input(None) {
            match self.mode.clone() {
                Mode::Pick => self.handle_pick_event(event),
                Mode::Prompt {
                    domain,
                    input,
                    error: _,
                } => self.handle_prompt_event(event, domain, input),
            }
            if matches!(self.mode, Mode::Pick) && self.selection == "__done__" {
                break;
            }
            self.render(term)?;
        }
        Ok(())
    }

    fn handle_pick_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char(c),
                modifiers: Modifiers::NONE,
            }) if ALPHABET.contains(c) => {
                self.selection.push(c);
                if let Some(pos) = self.labels.iter().position(|x| *x == self.selection) {
                    self.active_idx = self.top_row + pos;
                    if self.launch(self.active_idx) {
                        self.selection = "__done__".to_string();
                    }
                }
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('n'),
                ..
            }) => {
                if let Some(entry) = self.entries.get(self.active_idx).cloned() {
                    if let EntryAction::Manual {
                        domain,
                        default_name,
                    } = entry.action
                    {
                        self.mode = Mode::Prompt {
                            domain,
                            input: default_name,
                            error: None,
                        };
                    }
                }
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('j'),
                ..
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::DownArrow,
                ..
            }) => self.move_down(),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('k'),
                ..
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::UpArrow,
                ..
            }) => self.move_up(),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Enter,
                ..
            }) => {
                if self.launch(self.active_idx) {
                    self.selection = "__done__".to_string();
                }
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::Char('G') | KeyCode::Char('['),
                modifiers: Modifiers::CTRL,
            }) => {
                self.selection = "__done__".to_string();
            }
            InputEvent::Mouse(MouseEvent {
                y, mouse_buttons, ..
            }) if mouse_buttons.contains(MouseButtons::VERT_WHEEL) => {
                if mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
                    self.top_row = self.top_row.saturating_sub(1);
                } else {
                    self.top_row = (self.top_row + 1).min(
                        self.entries
                            .len()
                            .saturating_sub(self.max_items)
                            .saturating_sub(1),
                    );
                }
                if y > 0 && y as usize <= self.entries.len() {
                    self.active_idx = self.top_row + y as usize - 1;
                }
            }
            _ => {}
        }
    }

    fn handle_prompt_event(&mut self, event: InputEvent, domain: String, mut input: String) {
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            }) => self.mode = Mode::Pick,
            InputEvent::Key(KeyEvent {
                key: KeyCode::Backspace,
                ..
            }) => {
                input.pop();
                self.mode = Mode::Prompt {
                    domain,
                    input,
                    error: None,
                };
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Enter,
                ..
            }) => match validate_shell_name(&input) {
                Ok(()) => {
                    self.open(domain, input);
                    self.mode = Mode::Pick;
                    self.selection = "__done__".to_string();
                }
                Err(err) => {
                    self.mode = Mode::Prompt {
                        domain,
                        input,
                        error: Some(err),
                    }
                }
            },
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char(c),
                ..
            }) => {
                input.push(c);
                self.mode = Mode::Prompt {
                    domain,
                    input,
                    error: None,
                };
            }
            _ => {
                self.mode = Mode::Prompt {
                    domain,
                    input,
                    error: None,
                }
            }
        }
    }
}

fn build_entries(targets: &[D2bTargetPickerEntry], kind: PickerKind) -> Vec<Entry> {
    let mut entries = vec![];
    for target in targets {
        let target_label = sanitize_display_label(&target.target);
        let warning = target.warning_text();
        if !target.available() {
            let reason = warning.unwrap_or_else(|| "unavailable".to_string());
            entries.push(Entry {
                label: format!("Unavailable d2b target `{target_label}` ({reason})"),
                disabled: true,
                action: EntryAction::Disabled,
            });
            continue;
        }
        let posture = warning
            .as_deref()
            .map(|warning| format!(" — {warning}"))
            .unwrap_or_default();

        if kind == PickerKind::CurrentTarget {
            entries.push(Entry {
                label: format!(
                    "New d2b shell `[{target_label}:{}]`{posture}",
                    sanitize_display_label(&target.generated_name)
                ),
                disabled: false,
                action: EntryAction::Open {
                    domain: target.domain_name.clone(),
                    name: target.generated_name.clone(),
                },
            });

            let mut sessions = target
                .sessions
                .iter()
                .filter(|session| !session.attached && session.state == ShellSessionState::Detached)
                .cloned()
                .collect::<Vec<_>>();
            sessions.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.id.cmp(&b.id)));
            for session in sessions {
                let session_label = sanitize_display_label(&session.id);
                entries.push(Entry {
                    label: format!(
                        "Reattach d2b shell `[{target_label}:{session_label}]`{posture}"
                    ),
                    disabled: false,
                    action: EntryAction::Open {
                        domain: target.domain_name.clone(),
                        name: session.id,
                    },
                });
            }
            continue;
        }

        entries.push(Entry {
            label: format!(
                "New d2b shell on `{target_label}` ({}){posture}",
                sanitize_display_label(&target.generated_name)
            ),
            disabled: false,
            action: EntryAction::Open {
                domain: target.domain_name.clone(),
                name: target.generated_name.clone(),
            },
        });
        entries.push(Entry {
            label: format!("Name new d2b shell on `{target_label}`…{posture}"),
            disabled: false,
            action: EntryAction::Manual {
                domain: target.domain_name.clone(),
                default_name: target.generated_name.clone(),
            },
        });

        let mut sessions = target.sessions.clone();
        sessions.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.id.cmp(&b.id)));
        for session in sessions {
            let session_label = sanitize_display_label(&session.id);
            let state = if session.is_default {
                "default".to_string()
            } else if session.attached {
                "attached".to_string()
            } else {
                format!("{:?}", session.state).to_ascii_lowercase()
            };
            entries.push(Entry {
                label: format!(
                    "Open d2b shell `{target_label}:{session_label}` ({state}){posture}"
                ),
                disabled: false,
                action: EntryAction::Open {
                    domain: target.domain_name.clone(),
                    name: session.id,
                },
            });
        }
    }
    entries
}

pub fn d2b_launcher(
    args: D2bLauncherArgs,
    mut term: TermWizTerminal,
    window: ::window::Window,
) -> anyhow::Result<()> {
    let mut state = State::new(args, window);
    term.set_raw_mode()?;
    term.render(&[Change::Title("d2b sessions".to_string())])?;
    state.render(&mut term)?;
    state.run_loop(&mut term)
}

#[cfg(test)]
mod test {
    use super::*;
    use mux::d2b::{
        D2bCorrelationId, IsolationPosture, ShellSessionState, WorkloadAvailability,
        WorkloadProviderKind,
    };

    fn session(name: &str, is_default: bool) -> D2bSession {
        D2bSession {
            id: name.to_string(),
            label: name.to_string(),
            target: "work".to_string(),
            workspace: None,
            state: ShellSessionState::Detached,
            attached: false,
            is_default,
            correlation_id: D2bCorrelationId::from_sensitive("session", name),
        }
    }

    fn ready_status(target: &str) -> D2bTargetStatus {
        D2bTargetStatus::new(
            target,
            WorkloadProviderKind::LocalVm,
            IsolationPosture::VirtualMachine,
            WorkloadAvailability::Ready,
            true,
        )
        .unwrap()
    }

    #[test]
    fn entries_disable_offline_targets_without_open_actions() {
        let entries = build_entries(
            &[D2bTargetPickerEntry {
                domain_name: "d2b-work".to_string(),
                target: "work".to_string(),
                status: None,
                unavailable_reason: Some("offline".to_string()),
                sessions: vec![],
                generated_name: "work-shell".to_string(),
            }],
            PickerKind::AllTargets,
        );
        assert_eq!(entries.len(), 1);
        assert!(entries[0].disabled);
        assert_eq!(entries[0].action, EntryAction::Disabled);
    }

    #[test]
    fn entries_include_generated_manual_and_existing_sessions() {
        let entries = build_entries(
            &[D2bTargetPickerEntry {
                domain_name: "d2b-work".to_string(),
                target: "work".to_string(),
                status: Some(ready_status("work")),
                unavailable_reason: None,
                sessions: vec![session("default", true), session("build", false)],
                generated_name: "work-shell".to_string(),
            }],
            PickerKind::AllTargets,
        );
        assert!(entries
            .iter()
            .any(|entry| entry.label.contains("work-shell")));
        assert!(entries
            .iter()
            .any(|entry| matches!(entry.action, EntryAction::Manual { .. })));
        assert!(entries
            .iter()
            .any(|entry| entry.label.contains("work:default")));
        assert!(entries.iter().all(|entry| !entry.disabled));
    }

    #[test]
    fn entries_sanitize_target_and_session_labels() {
        let entries = build_entries(
            &[D2bTargetPickerEntry {
                domain_name: "d2b-work".to_string(),
                target: "work\x1b[31m".to_string(),
                status: Some(ready_status("work")),
                unavailable_reason: None,
                sessions: vec![session("bad\x07\x1b[31m", false)],
                generated_name: "work-shell".to_string(),
            }],
            PickerKind::AllTargets,
        );
        let combined = entries
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!combined.contains('\x1b'));
        assert!(!combined.contains('\x07'));
        assert!(combined.contains("work:bad"));
    }

    #[test]
    fn entries_warn_that_unsafe_local_has_no_isolation() {
        let status = D2bTargetStatus::new(
            "tools.host.d2b",
            WorkloadProviderKind::UnsafeLocal,
            IsolationPosture::UnsafeLocal,
            WorkloadAvailability::Ready,
            true,
        )
        .unwrap();
        let entries = build_entries(
            &[D2bTargetPickerEntry {
                domain_name: "host-tools".to_string(),
                target: "tools.host.d2b".to_string(),
                status: Some(status),
                unavailable_reason: None,
                sessions: vec![],
                generated_name: "d2b-tools-shell".to_string(),
            }],
            PickerKind::AllTargets,
        );

        assert!(entries.iter().all(|entry| !entry.disabled));
        assert!(entries
            .iter()
            .all(|entry| entry.label.contains("UNSAFE LOCAL — NO ISOLATION")));
    }

    #[test]
    fn helper_unavailable_is_visible_and_disables_target() {
        let status = D2bTargetStatus::new(
            "tools.host.d2b",
            WorkloadProviderKind::UnsafeLocal,
            IsolationPosture::UnsafeLocal,
            WorkloadAvailability::HelperUnavailable,
            true,
        )
        .unwrap();
        let entries = build_entries(
            &[D2bTargetPickerEntry {
                domain_name: "host-tools".to_string(),
                target: "tools.host.d2b".to_string(),
                status: Some(status),
                unavailable_reason: None,
                sessions: vec![],
                generated_name: "d2b-tools-shell".to_string(),
            }],
            PickerKind::AllTargets,
        );

        assert_eq!(entries.len(), 1);
        assert!(entries[0].disabled);
        assert!(entries[0].label.contains("UNSAFE LOCAL — NO ISOLATION"));
        assert!(entries[0].label.contains("helper unavailable"));
    }

    #[test]
    fn scoped_entries_create_immediately_and_only_reattach_detached_shells() {
        let mut attached = session("attached", false);
        attached.state = ShellSessionState::Attached;
        attached.attached = true;
        let mut killed = session("killed", false);
        killed.state = ShellSessionState::Killed;
        let entries = build_entries(
            &[D2bTargetPickerEntry {
                domain_name: "host-tools".to_string(),
                target: "tools.host.d2b".to_string(),
                status: Some(ready_status("tools.host.d2b")),
                unavailable_reason: None,
                sessions: vec![
                    session("detached", false),
                    attached,
                    killed,
                    session("default", true),
                ],
                generated_name: "new-shell".to_string(),
            }],
            PickerKind::CurrentTarget,
        );

        assert_eq!(entries.len(), 3);
        assert!(entries[0].label.contains("[tools.host.d2b:new-shell]"));
        assert!(matches!(
            &entries[0].action,
            EntryAction::Open { name, .. } if name == "new-shell"
        ));
        assert!(entries
            .iter()
            .all(|entry| !matches!(entry.action, EntryAction::Manual { .. })));
        let labels = entries
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(labels.contains("detached"));
        assert!(labels.contains("default"));
        assert!(!labels.contains("attached"));
        assert!(!labels.contains("killed"));
    }
}
