//! Port forwarding management overlay.
//!
//! Displays detected and forwarded ports with interactive controls.
//! Follows the TermWiz overlay pattern used by the launcher and selector.
//!
//! --- weezterm remote features ---

use termwiz::cell::AttributeChange;
use termwiz::color::AnsiColor;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers};
use termwiz::surface::{Change, CursorVisibility, Position};
use termwiz::terminal::Terminal;

/// Entry for display in the port forwarding overlay
#[derive(Debug, Clone)]
pub struct PortDisplayEntry {
    pub remote_port: u16,
    pub local_port: u16,
    pub remote_host: String,
    pub label: Option<String>,
    pub is_forwarded: bool,
    pub is_error: bool,
    pub error_msg: Option<String>,
}

/// Run the port forwarding overlay.
///
/// This is the main entry point, called from the overlay spawn mechanism.
/// It renders the port list and handles keyboard input.
pub fn run_port_forward_overlay(
    mut term: impl Terminal,
    entries: Vec<PortDisplayEntry>,
) -> anyhow::Result<PortForwardAction> {
    let mut state = OverlayState {
        entries,
        selected: 0,
        filter: String::new(),
        action: None,
    };

    term.set_raw_mode()?;
    term.render(&[Change::CursorVisibility(CursorVisibility::Hidden)])?;

    loop {
        render_overlay(&mut term, &state)?;

        match term.poll_input(None) {
            Ok(Some(input)) => match input {
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('q'),
                    modifiers: Modifiers::NONE,
                    ..
                }) => {
                    return Ok(PortForwardAction::Close);
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
                    if state.selected > 0 {
                        state.selected -= 1;
                    } else if !state.filtered_entries().is_empty() {
                        state.selected = state.filtered_entries().len() - 1;
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
                    let max = state.filtered_entries().len().saturating_sub(1);
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
                    if let Some(entry) = state.selected_entry() {
                        if entry.is_forwarded {
                            return Ok(PortForwardAction::StopForward {
                                remote_port: entry.remote_port,
                            });
                        } else {
                            return Ok(PortForwardAction::StartForward {
                                remote_port: entry.remote_port,
                                remote_host: entry.remote_host.clone(),
                                preferred_local_port: entry.remote_port,
                            });
                        }
                    }
                }

                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('f'),
                    modifiers: Modifiers::NONE,
                    ..
                }) => {
                    return Ok(PortForwardAction::ManualForward);
                }

                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('s'),
                    modifiers: Modifiers::NONE,
                    ..
                }) => {
                    if let Some(entry) = state.selected_entry() {
                        if entry.is_forwarded {
                            return Ok(PortForwardAction::StopForward {
                                remote_port: entry.remote_port,
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
                        return Ok(PortForwardAction::Exclude {
                            remote_port: entry.remote_port,
                        });
                    }
                }

                InputEvent::Key(KeyEvent {
                    key: KeyCode::Backspace,
                    ..
                }) => {
                    state.filter.pop();
                    state.selected = 0;
                }

                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char(c),
                    modifiers: Modifiers::NONE,
                    ..
                }) if c.is_ascii_digit() => {
                    state.filter.push(c);
                    state.selected = 0;
                }

                _ => {}
            },
            Ok(None) => {}
            Err(_) => break,
        }
    }

    Ok(PortForwardAction::Close)
}

/// Actions that can result from the overlay interaction
#[derive(Debug, Clone)]
pub enum PortForwardAction {
    Close,
    StartForward {
        remote_port: u16,
        remote_host: String,
        preferred_local_port: u16,
    },
    StopForward {
        remote_port: u16,
    },
    Exclude {
        remote_port: u16,
    },
    ManualForward,
}

struct OverlayState {
    entries: Vec<PortDisplayEntry>,
    selected: usize,
    filter: String,
    action: Option<PortForwardAction>,
}

impl OverlayState {
    fn filtered_entries(&self) -> Vec<&PortDisplayEntry> {
        if self.filter.is_empty() {
            self.entries.iter().collect()
        } else {
            self.entries
                .iter()
                .filter(|e| e.remote_port.to_string().contains(&self.filter))
                .collect()
        }
    }

    fn selected_entry(&self) -> Option<PortDisplayEntry> {
        let filtered = self.filtered_entries();
        filtered.get(self.selected).map(|e| (*e).clone())
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
    changes.push(Change::Attribute(AttributeChange::Foreground(
        AnsiColor::White.into(),
    )));
    changes.push(Change::Attribute(AttributeChange::Intensity(
        termwiz::cell::Intensity::Bold,
    )));
    changes.push(Change::Text(
        " Port Forwarding                      [Esc] close\r\n".into(),
    ));
    changes.push(Change::Attribute(AttributeChange::Intensity(
        termwiz::cell::Intensity::Normal,
    )));
    changes.push(Change::Attribute(AttributeChange::Foreground(
        AnsiColor::Grey.into(),
    )));
    changes.push(Change::Text(
        " ──────────────────────────────────────────────────\r\n".into(),
    ));

    let filtered = state.filtered_entries();

    if filtered.is_empty() {
        changes.push(Change::Attribute(AttributeChange::Foreground(
            AnsiColor::Grey.into(),
        )));
        changes.push(Change::Text(
            "  No ports detected. Waiting for activity...\r\n".into(),
        ));
    } else {
        for (idx, entry) in filtered.iter().enumerate() {
            let is_selected = idx == state.selected;

            // Selection indicator
            if is_selected {
                changes.push(Change::Attribute(AttributeChange::Foreground(
                    AnsiColor::Cyan.into(),
                )));
                changes.push(Change::Text(" ▸ ".into()));
            } else {
                changes.push(Change::Text("   ".into()));
            }

            // Status indicator
            if entry.is_error {
                changes.push(Change::Attribute(AttributeChange::Foreground(
                    AnsiColor::Red.into(),
                )));
                changes.push(Change::Text("✗ ".into()));
            } else if entry.is_forwarded {
                changes.push(Change::Attribute(AttributeChange::Foreground(
                    AnsiColor::Green.into(),
                )));
                changes.push(Change::Text("● ".into()));
            } else {
                changes.push(Change::Attribute(AttributeChange::Foreground(
                    AnsiColor::Yellow.into(),
                )));
                changes.push(Change::Text("○ ".into()));
            }

            // Port info
            changes.push(Change::Attribute(AttributeChange::Foreground(
                AnsiColor::White.into(),
            )));

            let port_text = if entry.is_forwarded {
                format!(":{} → localhost:{}", entry.remote_port, entry.local_port)
            } else {
                format!(":{}", entry.remote_port)
            };
            changes.push(Change::Text(port_text));

            // Label
            if let Some(ref label) = entry.label {
                changes.push(Change::Attribute(AttributeChange::Foreground(
                    AnsiColor::Grey.into(),
                )));
                changes.push(Change::Text(format!("  ({})", label)));
            }

            // Error message
            if let Some(ref err) = entry.error_msg {
                changes.push(Change::Attribute(AttributeChange::Foreground(
                    AnsiColor::Red.into(),
                )));
                changes.push(Change::Text(format!("  [{}]", err)));
            }

            // State hint
            if !entry.is_forwarded && !entry.is_error {
                changes.push(Change::Attribute(AttributeChange::Foreground(
                    AnsiColor::Grey.into(),
                )));
                changes.push(Change::Text("  (detected)".into()));
            }

            changes.push(Change::Text("\r\n".into()));
        }
    }

    // Footer
    changes.push(Change::Attribute(AttributeChange::Foreground(
        AnsiColor::Grey.into(),
    )));
    changes.push(Change::Text(
        " ──────────────────────────────────────────────────\r\n".into(),
    ));
    changes.push(Change::Text(
        " [Enter] toggle  [F]orward new  [S]top  [D]elete  ".into(),
    ));

    // Filter display
    if !state.filter.is_empty() {
        changes.push(Change::Text(format!("\r\n Filter: {}", state.filter)));
    }

    term.render(&changes)?;
    term.flush()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_overlay_state_filtering() {
        let state = OverlayState {
            entries: vec![
                PortDisplayEntry {
                    remote_port: 3000,
                    local_port: 3000,
                    remote_host: "127.0.0.1".into(),
                    label: None,
                    is_forwarded: true,
                    is_error: false,
                    error_msg: None,
                },
                PortDisplayEntry {
                    remote_port: 8080,
                    local_port: 8080,
                    remote_host: "0.0.0.0".into(),
                    label: Some("webpack".into()),
                    is_forwarded: false,
                    is_error: false,
                    error_msg: None,
                },
            ],
            selected: 0,
            filter: "80".into(),
            action: None,
        };

        let filtered = state.filtered_entries();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].remote_port, 8080);
    }

    #[test]
    fn test_overlay_state_selection_wrapping() {
        let state = OverlayState {
            entries: vec![
                PortDisplayEntry {
                    remote_port: 3000,
                    local_port: 3000,
                    remote_host: "127.0.0.1".into(),
                    label: None,
                    is_forwarded: true,
                    is_error: false,
                    error_msg: None,
                },
                PortDisplayEntry {
                    remote_port: 8080,
                    local_port: 8080,
                    remote_host: "0.0.0.0".into(),
                    label: None,
                    is_forwarded: false,
                    is_error: false,
                    error_msg: None,
                },
            ],
            selected: 1,
            filter: String::new(),
            action: None,
        };

        assert_eq!(state.selected_entry().unwrap().remote_port, 8080);
    }
}
