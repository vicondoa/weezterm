// --- weezterm remote features ---
//! DevContainer management overlay.
//!
//! Displays discovered devcontainers with interactive controls.
//! Follows the TermWiz overlay pattern used by port_forward.rs.

use mux::devcontainer_discover::{ContainerStatus, DevContainerInfo};
use termwiz::cell::AttributeChange;
use termwiz::color::AnsiColor;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers};
use termwiz::surface::{Change, CursorVisibility, Position};
use termwiz::terminal::Terminal;

/// Actions that can result from the overlay interaction
#[derive(Debug, Clone)]
pub enum DevContainerAction {
    Close,
    /// Connect to a container (open new tab)
    Connect { container_id: String },
    /// Set a container as the primary for new tabs
    SetPrimary { container_id: String },
    /// Start a stopped container
    StartContainer { container_id: String },
    /// Stop a running container
    StopContainer { container_id: String },
    /// Delete a container
    DeleteContainer { container_id: String },
    /// Create a new container from a workspace folder
    CreateContainer { workspace_folder: String },
}

/// Run the devcontainer manager overlay.
pub fn run_devcontainer_overlay(
    mut term: impl Terminal,
    entries: Vec<DevContainerInfo>,
    domain_name: String,
    host_label: String,
    primary_container_id: Option<String>,
    default_workspace_folder: Option<String>,
) -> anyhow::Result<DevContainerAction> {
    let mut state = OverlayState {
        entries,
        selected: 0,
        filter: String::new(),
        primary_container_id,
        domain_name,
        host_label,
        expanded: None,
        create_input: None,
        default_workspace_folder,
    };

    term.set_raw_mode()?;
    term.render(&[Change::CursorVisibility(CursorVisibility::Hidden)])?;

    loop {
        render_overlay(&mut term, &state)?;

        match term.poll_input(None) {
            Ok(Some(input)) => {
                // If in create-input mode
                if let Some(ref mut input_state) = state.create_input {
                    match input {
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Escape,
                            ..
                        }) => {
                            state.create_input = None;
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Enter,
                            ..
                        }) => {
                            let folder = input_state.clone();
                            if !folder.is_empty() {
                                return Ok(DevContainerAction::CreateContainer {
                                    workspace_folder: folder,
                                });
                            }
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Backspace,
                            ..
                        }) => {
                            input_state.pop();
                        }
                        InputEvent::Key(KeyEvent {
                            key: KeyCode::Char(c),
                            ..
                        }) => {
                            input_state.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                // Normal mode
                match input {
                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Escape,
                        ..
                    })
                    | InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('q'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        return Ok(DevContainerAction::Close);
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::UpArrow,
                        ..
                    })
                    | InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('k'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        let max = state.total_rows();
                        if state.selected > 0 {
                            state.selected -= 1;
                        } else if max > 0 {
                            state.selected = max - 1;
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::DownArrow,
                        ..
                    })
                    | InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('j'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        let max = state.total_rows().saturating_sub(1);
                        if state.selected < max {
                            state.selected += 1;
                        } else {
                            state.selected = 0;
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Enter,
                        ..
                    }) => {
                        if state.is_create_row() {
                            // Enter create mode
                            state.create_input = Some(
                                state
                                    .default_workspace_folder
                                    .clone()
                                    .unwrap_or_default(),
                            );
                        } else if let Some(entry) = state.selected_entry() {
                            if state.expanded.as_deref() == Some(&entry.container_id) {
                                state.expanded = None;
                            } else {
                                state.expanded = Some(entry.container_id.clone());
                            }
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('c'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        if let Some(entry) = state.selected_entry() {
                            if entry.status.is_running() {
                                return Ok(DevContainerAction::Connect {
                                    container_id: entry.container_id.clone(),
                                });
                            }
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('p'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        if let Some(entry) = state.selected_entry() {
                            if entry.status.is_running() {
                                return Ok(DevContainerAction::SetPrimary {
                                    container_id: entry.container_id.clone(),
                                });
                            }
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('s'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        if let Some(entry) = state.selected_entry() {
                            if !entry.status.is_running() {
                                return Ok(DevContainerAction::StartContainer {
                                    container_id: entry.container_id.clone(),
                                });
                            }
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('S'),
                        modifiers: Modifiers::SHIFT,
                        ..
                    })
                    | InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('S'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        if let Some(entry) = state.selected_entry() {
                            if entry.status.is_running() {
                                return Ok(DevContainerAction::StopContainer {
                                    container_id: entry.container_id.clone(),
                                });
                            }
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('d'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        if let Some(entry) = state.selected_entry() {
                            if !entry.status.is_running() {
                                return Ok(DevContainerAction::DeleteContainer {
                                    container_id: entry.container_id.clone(),
                                });
                            }
                        }
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('n'),
                        modifiers: Modifiers::NONE,
                        ..
                    }) => {
                        state.create_input = Some(
                            state
                                .default_workspace_folder
                                .clone()
                                .unwrap_or_default(),
                        );
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Char('/'),
                        ..
                    }) => {
                        state.filter.clear();
                    }

                    InputEvent::Key(KeyEvent {
                        key: KeyCode::Backspace,
                        ..
                    }) => {
                        state.filter.pop();
                        state.selected = 0;
                    }

                    _ => {}
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }

    Ok(DevContainerAction::Close)
}

struct OverlayState {
    entries: Vec<DevContainerInfo>,
    selected: usize,
    filter: String,
    primary_container_id: Option<String>,
    domain_name: String,
    host_label: String,
    expanded: Option<String>,
    create_input: Option<String>,
    default_workspace_folder: Option<String>,
}

impl OverlayState {
    fn filtered_entries(&self) -> Vec<&DevContainerInfo> {
        if self.filter.is_empty() {
            self.entries.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.entries
                .iter()
                .filter(|e| {
                    e.container_name.to_lowercase().contains(&filter_lower)
                        || e.image.to_lowercase().contains(&filter_lower)
                        || e.local_folder.to_lowercase().contains(&filter_lower)
                })
                .collect()
        }
    }

    fn total_rows(&self) -> usize {
        // container rows + 1 for "Create new" row
        self.filtered_entries().len() + 1
    }

    fn is_create_row(&self) -> bool {
        self.selected == self.filtered_entries().len()
    }

    fn selected_entry(&self) -> Option<&DevContainerInfo> {
        let filtered = self.filtered_entries();
        filtered.get(self.selected).copied()
    }

    fn is_primary(&self, container_id: &str) -> bool {
        self.primary_container_id.as_deref() == Some(container_id)
    }
}

fn render_overlay(term: &mut impl Terminal, state: &OverlayState) -> anyhow::Result<()> {
    let mut changes = vec![
        Change::ClearScreen(Default::default()),
        Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(0),
        },
    ];

    // Header
    push_bold_white(&mut changes);
    changes.push(Change::Text(format!(
        " DevContainer Manager \u{2500} {}",
        state.host_label
    )));
    push_grey(&mut changes);
    changes.push(Change::Text("                    Esc close\r\n".into()));
    changes.push(Change::Text(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\r\n".into(),
    ));

    let filtered = state.filtered_entries();

    if filtered.is_empty() && state.entries.is_empty() {
        push_grey(&mut changes);
        changes.push(Change::Text(
            "\r\n \u{26A0} No devcontainers found.\r\n\r\n".into(),
        ));
        changes.push(Change::Text(
            "   Press n to create a new devcontainer.\r\n\r\n".into(),
        ));
    } else {
        // Group by status: running first, then stopped
        let running: Vec<_> = filtered.iter().filter(|e| e.status.is_running()).collect();
        let stopped: Vec<_> = filtered.iter().filter(|e| !e.status.is_running()).collect();

        changes.push(Change::Text("\r\n".into()));

        let mut row_idx = 0usize;

        if !running.is_empty() {
            push_bold_white(&mut changes);
            changes.push(Change::Text(
                format!(" RUNNING ({})\r\n\r\n", running.len()),
            ));
            for entry in &running {
                render_container_row(&mut changes, state, entry, row_idx);
                if state.expanded.as_deref() == Some(&entry.container_id) {
                    render_expanded_details(&mut changes, entry);
                }
                row_idx += 1;
            }
            changes.push(Change::Text("\r\n".into()));
        }

        if !stopped.is_empty() {
            push_bold_white(&mut changes);
            changes.push(Change::Text(
                format!(" STOPPED ({})\r\n\r\n", stopped.len()),
            ));
            for entry in &stopped {
                render_container_row(&mut changes, state, entry, row_idx);
                if state.expanded.as_deref() == Some(&entry.container_id) {
                    render_expanded_details(&mut changes, entry);
                }
                row_idx += 1;
            }
            changes.push(Change::Text("\r\n".into()));
        }

        // "Create new container" row
        let is_create_selected = state.selected == row_idx;
        if is_create_selected {
            changes.push(Change::Attribute(AttributeChange::Foreground(
                AnsiColor::Aqua.into(),
            )));
            changes.push(Change::Text(" \u{25B8} ".into()));
        } else {
            changes.push(Change::Text("   ".into()));
        }
        changes.push(Change::Attribute(AttributeChange::Foreground(
            AnsiColor::Green.into(),
        )));
        changes.push(Change::Text("+ Create new container...\r\n".into()));

        // Inline create input
        if let Some(ref input_text) = state.create_input {
            push_grey(&mut changes);
            changes.push(Change::Text(
                "   \u{2502}\r\n".into(),
            ));
            changes.push(Change::Text(
                "   \u{2502}  Workspace folder: ".into(),
            ));
            push_white(&mut changes);
            changes.push(Change::Text(format!("{}\u{258F}\r\n", input_text)));
            push_grey(&mut changes);
            changes.push(Change::Text(
                "   \u{2502}\r\n".into(),
            ));
        }
    }

    // Footer
    push_grey(&mut changes);
    changes.push(Change::Text(
        " \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\r\n".into(),
    ));
    changes.push(Change::Text(
        " \u{2605} = primary (new tabs open here)\r\n".into(),
    ));
    push_white(&mut changes);
    changes.push(Change::Text(
        " Enter expand  c connect  p primary  n new  s start  S stop  d delete\r\n".into(),
    ));

    term.render(&changes)?;
    term.flush()?;
    Ok(())
}

fn render_container_row(
    changes: &mut Vec<Change>,
    state: &OverlayState,
    entry: &DevContainerInfo,
    row_idx: usize,
) {
    let is_selected = state.selected == row_idx;
    let is_expanded = state.expanded.as_deref() == Some(&entry.container_id);
    let is_primary = state.is_primary(&entry.container_id);

    // Selection indicator
    if is_selected {
        changes.push(Change::Attribute(AttributeChange::Foreground(
            AnsiColor::Aqua.into(),
        )));
        if is_expanded {
            changes.push(Change::Text(" \u{25BE} ".into())); // ▾
        } else {
            changes.push(Change::Text(" \u{25B8} ".into())); // ▸
        }
    } else {
        changes.push(Change::Text("   ".into()));
    }

    // Status symbol
    if entry.status.is_running() {
        changes.push(Change::Attribute(AttributeChange::Foreground(
            AnsiColor::Green.into(),
        )));
        changes.push(Change::Text("\u{25CF} ".into())); // ●
    } else {
        changes.push(Change::Attribute(AttributeChange::Foreground(
            AnsiColor::Yellow.into(),
        )));
        changes.push(Change::Text("\u{25CB} ".into())); // ○
    }

    // Primary marker
    if is_primary {
        changes.push(Change::Attribute(AttributeChange::Foreground(
            AnsiColor::Yellow.into(),
        )));
        changes.push(Change::Text("\u{2605} ".into())); // ★
    } else {
        changes.push(Change::Text("  ".into()));
    }

    // Container name
    push_bold_white(changes);
    let name = &entry.container_name;
    let name_display = if name.len() > 20 {
        format!("{}..", &name[..18])
    } else {
        format!("{:20}", name)
    };
    changes.push(Change::Text(name_display));

    // Image (truncated)
    push_grey(changes);
    let image = &entry.image;
    let image_display = if image.len() > 16 {
        format!(" {:16}", &image[..16])
    } else {
        format!(" {:16}", image)
    };
    changes.push(Change::Text(image_display));

    // Local folder
    changes.push(Change::Text(format!("  {}", entry.local_folder)));

    changes.push(Change::Text("\r\n".into()));
}

fn render_expanded_details(changes: &mut Vec<Change>, entry: &DevContainerInfo) {
    push_grey(changes);

    let short_id = if entry.container_id.len() > 12 {
        &entry.container_id[..12]
    } else {
        &entry.container_id
    };

    let lines = [
        format!("   \u{2502}  ID \u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7} {}", short_id),
        format!("   \u{2502}  Image \u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7} {}", entry.image),
        format!("   \u{2502}  Folder \u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7} {}", entry.local_folder),
        format!("   \u{2502}  Workspace \u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7}\u{00B7} {}", entry.workspace_folder.as_deref().unwrap_or("(unknown)")),
    ];

    for line in &lines {
        changes.push(Change::Text(format!("{}\r\n", line)));
    }

    // Actions
    changes.push(Change::Text("   \u{2502}\r\n".into()));
    push_white(changes);
    if entry.status.is_running() {
        changes.push(Change::Text(
            "   \u{2502}  [c] Connect  [p] Set primary  [S] Stop\r\n".into(),
        ));
    } else {
        changes.push(Change::Text(
            "   \u{2502}  [s] Start  [d] Delete\r\n".into(),
        ));
    }
    push_grey(changes);
    changes.push(Change::Text("   \u{2502}\r\n".into()));
}

fn push_bold_white(changes: &mut Vec<Change>) {
    changes.push(Change::Attribute(AttributeChange::Foreground(
        AnsiColor::White.into(),
    )));
    changes.push(Change::Attribute(AttributeChange::Intensity(
        termwiz::cell::Intensity::Bold,
    )));
}

fn push_white(changes: &mut Vec<Change>) {
    changes.push(Change::Attribute(AttributeChange::Foreground(
        AnsiColor::White.into(),
    )));
    changes.push(Change::Attribute(AttributeChange::Intensity(
        termwiz::cell::Intensity::Normal,
    )));
}

fn push_grey(changes: &mut Vec<Change>) {
    changes.push(Change::Attribute(AttributeChange::Foreground(
        AnsiColor::Grey.into(),
    )));
    changes.push(Change::Attribute(AttributeChange::Intensity(
        termwiz::cell::Intensity::Normal,
    )));
}
// --- end weezterm remote features ---
