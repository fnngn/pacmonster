use std::collections::VecDeque;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

#[derive(Clone, Debug, serde::Deserialize)]
struct StateGhost {
    id: usize,
    busy: bool,
    target: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct StateFile {
    active: usize,
    idle: usize,
    capacity: usize,
    ghosts: Vec<StateGhost>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct DoctorCheck {
    name: String,
    status: String,
    detail: String,
}

/// Read the newest scheduler state file available to the current process.
fn read_state() -> Option<StateFile> {
    let effective_uid = fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|line| line.starts_with("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse::<u32>()
        .ok()?;
    let paths = [
        PathBuf::from(format!("/var/tmp/pacmonster-state-{effective_uid}.json")),
        PathBuf::from("/var/tmp/pacmonster-state-0.json"),
    ];

    paths
        .iter()
        .filter_map(|path| {
            let modified = path.metadata().ok()?.modified().ok()?;
            let state = serde_json::from_reader(File::open(path).ok()?).ok()?;
            Some((modified, state))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, state)| state)
}

/// Restore the terminal after normal exit or a panic.
fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

/// Forward one child-process stream to the UI without blocking terminal input.
fn forward_output<R: io::Read + Send + 'static>(
    stream: R,
    is_stdout: bool,
    sender: Sender<(bool, String)>,
) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            match line {
                Ok(line) => {
                    if sender.send((is_stdout, line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send((false, format!("Could not read command output: {error}")));
                    break;
                }
            }
        }
        let _ = sender.send((is_stdout, "\0".to_string()));
    });
}

/// Report whether the process has an effective user ID of zero.
fn is_root() -> io::Result<bool> {
    let status = fs::read_to_string("/proc/self/status")?;
    let uid_line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| io::Error::other("could not determine the effective user ID"))?;
    let effective_uid = uid_line
        .split_whitespace()
        .nth(2)
        .ok_or_else(|| io::Error::other("could not parse the effective user ID"))?;
    Ok(effective_uid == "0")
}

/// Validate sudo credentials before starting a privileged maintenance command.
fn authorize_root(action: &str) -> io::Result<bool> {
    if is_root()? {
        return Ok(true);
    }

    let status = Command::new("sudo").arg("-v").status().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{action} requires root privileges; could not run `sudo -v`: {error}"),
        )
    })?;
    if status.success() {
        Ok(false)
    } else {
        Err(io::Error::other(format!(
            "{action} requires root privileges; sudo authorization was not granted"
        )))
    }
}

/// Find an executable by searching the current process PATH.
fn find_executable(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

/// Locate the Python interpreter and install root for the current user's package.
fn find_python_package() -> Option<(PathBuf, PathBuf)> {
    let output = Command::new("python3")
        .args([
            "-c",
            "import pathlib, sys, pacmonster; print(sys.executable); print(pathlib.Path(pacmonster.__file__).resolve().parent.parent)",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let lines: Vec<_> = String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(str::to_owned)
        .collect();
    if lines.len() != 2 {
        return None;
    }
    Some((PathBuf::from(&lines[0]), PathBuf::from(&lines[1])))
}

/// Start the selected Pacmonster command and stream its output to the UI.
fn start_command(sender: &Sender<(bool, String)>, mode: &str) -> io::Result<Child> {
    let arguments: &[&str] = match mode {
        "updates" => &["update", "--yes"],
        "doctor" => &["doctor", "--json"],
        _ => return Err(io::Error::other(format!("unsupported TUI mode: {mode}"))),
    };
    let running_as_root = authorize_root(mode)?;
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("TUI source root could not be resolved"))?;

    let mut command = if source_root.join("pacmonster/main.py").is_file() {
        let mut python = if running_as_root {
            Command::new("python3")
        } else {
            let mut sudo = Command::new("sudo");
            sudo.arg("-n").arg("python3");
            sudo
        };
        python
            .args(["-m", "pacmonster.main"])
            .current_dir(source_root);
        python
    } else if PathBuf::from("/usr/local/bin/pacmonster").is_file() {
        let pacmonster = if running_as_root {
            Command::new("/usr/local/bin/pacmonster")
        } else {
            let mut sudo = Command::new("sudo");
            sudo.args(["-n", "/usr/local/bin/pacmonster"]);
            sudo
        };
        pacmonster
    } else if let Some(pacmonster) = find_executable("pacmonster").filter(|path| {
        !path.starts_with(env::var_os("HOME").map(PathBuf::from).unwrap_or_default())
    }) {
        if running_as_root {
            Command::new(pacmonster)
        } else {
            let mut sudo = Command::new("sudo");
            sudo.arg("-n").arg(pacmonster);
            sudo
        }
    } else if let Some((python, package_root)) = find_python_package() {
        if running_as_root {
            let mut command = Command::new(python);
            command
                .args(["-m", "pacmonster.main"])
                .env("PYTHONPATH", package_root);
            command
        } else {
            let mut sudo = Command::new("sudo");
            sudo.args(["-n", "env"])
                .arg(format!("PYTHONPATH={}", package_root.display()))
                .arg(python)
                .args(["-m", "pacmonster.main"]);
            sudo
        }
    } else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Pacmonster is not installed for root and no source checkout is available; "
                .to_string()
                + "run `./install.sh` to install the system-wide command",
        ));
    };
    command
        .args(arguments)
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;

    if let Some(stdout) = child.stdout.take() {
        forward_output(stdout, true, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        forward_output(stderr, false, sender.clone());
    }
    Ok(child)
}

/// Send an interrupt request to the active maintenance process.
fn interrupt_update(child: &Child) -> io::Result<()> {
    let process_id = child.id().to_string();
    let status = Command::new("kill")
        .args(["-INT", process_id.as_str()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            "could not send SIGINT to the update process",
        ))
    }
}

/// Move the viewport by a signed number of rows without leaving its bounds.
fn move_scroll(current: u16, maximum: u16, delta: i32) -> u16 {
    (i32::from(current) + delta).clamp(0, i32::from(maximum)) as u16
}

/// Estimate rendered rows for wrapped text at the current viewport width.
fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

/// Run the dashboard, optional maintenance command, and terminal event loop.
fn main() -> io::Result<()> {
    std::panic::set_hook(Box::new(|panic| {
        restore_terminal();
        eprintln!("{panic}");
    }));

    let mode = if std::env::args().any(|argument| argument == "--run-doctor") {
        Some("doctor")
    } else if std::env::args().any(|argument| argument == "--run-updates") {
        Some("updates")
    } else {
        None
    };
    let (output_sender, output_receiver): (Sender<(bool, String)>, Receiver<(bool, String)>) =
        mpsc::channel();
    let mut output = VecDeque::new();
    let mut command_stdout = Vec::new();
    let mut doctor_checks: Vec<DoctorCheck> = Vec::new();
    let mut child = None;
    let mut child_finished = false;
    let mut stdout_finished = false;
    let mut completion_prompt = false;
    let mut completion_status = String::new();
    let mut interrupt_requested = false;
    let mut scroll_offset = 0u16;
    let mut max_scroll = 0u16;
    let mut page_size = 1u16;
    let mut follow_output = true;
    let mut exit_code = 0;

    if let Some(mode) = mode {
        match start_command(&output_sender, mode) {
            Ok(command_child) => {
                let description = match mode {
                    "updates" => "Starting all planned updates (--yes)...",
                    "doctor" => "Running Pacmonster Doctor diagnostics...",
                    _ => unreachable!(),
                };
                output.push_back(description.to_string());
                child = Some(command_child);
            }
            Err(error) => {
                output.push_back(format!("Could not start Pacmonster {mode}: {error}"));
                completion_status = format!("Pacmonster {mode} could not be started.");
                completion_prompt = true;
                exit_code = 4;
            }
        }
    }

    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    let result = (|| -> io::Result<()> {
        loop {
            while let Ok((is_stdout, line)) = output_receiver.try_recv() {
                if line == "\0" {
                    if is_stdout {
                        stdout_finished = true;
                    }
                    continue;
                }
                if is_stdout {
                    command_stdout.push(line.clone());
                }
                output.push_back(line);
                if output.len() > 300 {
                    output.pop_front();
                }
            }

            let child_status = match child.as_mut() {
                Some(update_child) => update_child.try_wait()?,
                None => None,
            };
            if let Some(status) = child_status {
                child.take();
                exit_code = status.code().unwrap_or(1);
                child_finished = true;
            }
            if child_finished && (mode != Some("doctor") || stdout_finished) && !completion_prompt {
                if mode == Some("doctor") {
                    match serde_json::from_str::<Vec<DoctorCheck>>(&command_stdout.join("\n")) {
                        Ok(checks) => doctor_checks = checks,
                        Err(error) => output
                            .push_back(format!("Could not parse Doctor results as JSON: {error}")),
                    }
                }
                completion_status = match mode {
                    Some("doctor") => {
                        let failures = doctor_checks
                            .iter()
                            .filter(|check| check.status == "fail")
                            .count();
                        format!(
                            "Doctor finished: {} checks, {failures} failures (exit code {exit_code}). Exit TUI? [y/N]",
                            doctor_checks.len()
                        )
                    }
                    _ => format!("Updates finished (exit code {exit_code}). Exit TUI? [y/N]"),
                };
                completion_prompt = true;
            }

            let state = read_state();
            terminal.draw(|frame| {
                if mode == Some("doctor") {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(2),
                            Constraint::Min(3),
                            Constraint::Length(2),
                        ])
                        .split(frame.area());
                    let count_status = |status: &str| {
                        doctor_checks
                            .iter()
                            .filter(|check| check.status == status)
                            .count()
                    };
                    let summary = Line::from(vec![
                        Span::styled(
                            format!("WITCHDOCTOR  {} checks  ", doctor_checks.len()),
                            Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("PASS {}  ", count_status("ok")),
                            Style::default().fg(Color::Green),
                        ),
                        Span::styled(
                            format!("WARN {}  ", count_status("warning")),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled(
                            format!("SKIP {}  ", count_status("skip")),
                            Style::default().fg(Color::Gray),
                        ),
                        Span::styled(
                            format!("FAIL {}", count_status("fail")),
                            Style::default().fg(Color::Red),
                        ),
                    ]);
                    frame.render_widget(Paragraph::new(summary), chunks[0]);
                    let mut lines: Vec<Line> = doctor_checks
                        .iter()
                        .flat_map(|check| {
                            let (label, color) = match check.status.as_str() {
                                "ok" => ("PASS", Color::Green),
                                "warning" => ("WARN", Color::Yellow),
                                "skip" => ("SKIP", Color::Gray),
                                "fail" => ("FAIL", Color::Red),
                                _ => ("????", Color::White),
                            };
                            vec![
                                Line::from(vec![
                                    Span::styled(format!("[{label}] "), Style::default().fg(color)),
                                    Span::styled(
                                        check.name.clone(),
                                        Style::default().fg(Color::White),
                                    ),
                                ]),
                                Line::from(format!("       {}", check.detail)),
                                Line::from(""),
                            ]
                        })
                        .collect();
                    if doctor_checks.is_empty() {
                        lines.extend(output.iter().cloned().map(Line::from));
                    }
                    max_scroll = wrapped_line_count(&lines, chunks[1].width)
                        .saturating_sub(chunks[1].height as usize)
                        .min(u16::MAX as usize) as u16;
                    page_size = chunks[1].height.saturating_sub(1).max(1);
                    if follow_output {
                        scroll_offset = max_scroll;
                    }
                    scroll_offset = scroll_offset.min(max_scroll);
                    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
                    frame.render_widget(paragraph.scroll((scroll_offset, 0)), chunks[1]);
                    let footer = if completion_prompt {
                        completion_status.as_str()
                    } else {
                        "↑/↓ PgUp/PgDn scroll • Home/End jump • q or Ctrl+C cancel/exit"
                    };
                    frame.render_widget(Paragraph::new(footer), chunks[2]);
                    return;
                }

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(8),
                        Constraint::Min(3),
                        Constraint::Length(2),
                    ])
                    .split(frame.area());

                let active = state.as_ref().map(|s| s.active).unwrap_or(0);
                let idle = state.as_ref().map(|s| s.idle).unwrap_or(0);
                let available = state
                    .as_ref()
                    .map(|s| s.capacity.saturating_sub(s.active))
                    .unwrap_or(0);
                let capacity_lines = vec![
                    Line::from("Ghost Pool"),
                    Line::from(""),
                    Line::from(format!("ᗣ Active      {active}")),
                    Line::from(format!("◌ Idle        {idle}")),
                    Line::from(format!("◌ Available   {available}")),
                ];
                frame.render_widget(Paragraph::new(capacity_lines), chunks[0]);

                let mut log_lines: Vec<Line> = state
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .ghosts
                            .iter()
                            .filter(|ghost| ghost.busy)
                            .map(|ghost| {
                                Line::from(format!(
                                    "ᗣ G{:02} {}",
                                    ghost.id,
                                    ghost.target.as_deref().unwrap_or_default()
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                log_lines.extend(output.iter().cloned().map(Line::from));
                max_scroll = wrapped_line_count(&log_lines, chunks[1].width)
                    .saturating_sub(chunks[1].height as usize)
                    .min(u16::MAX as usize) as u16;
                page_size = chunks[1].height.saturating_sub(1).max(1);
                if follow_output {
                    scroll_offset = max_scroll;
                }
                scroll_offset = scroll_offset.min(max_scroll);
                let paragraph = Paragraph::new(log_lines).wrap(Wrap { trim: true });
                frame.render_widget(paragraph.scroll((scroll_offset, 0)), chunks[1]);

                let footer = if completion_prompt {
                    completion_status.as_str()
                } else if child.is_some() {
                    "↑/↓ PgUp/PgDn scroll • Home/End jump • q or Ctrl+C requests safe interrupt"
                } else {
                    "↑/↓ PgUp/PgDn scroll • Home/End jump • q to quit"
                };
                frame.render_widget(Paragraph::new(footer), chunks[2]);
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    let navigation = match key.code {
                        KeyCode::Up | KeyCode::Char('k') => Some(-1),
                        KeyCode::Down | KeyCode::Char('j') => Some(1),
                        KeyCode::PageUp => Some(-i32::from(page_size)),
                        KeyCode::PageDown => Some(i32::from(page_size)),
                        KeyCode::Home => {
                            scroll_offset = 0;
                            follow_output = false;
                            Some(0)
                        }
                        KeyCode::End => {
                            scroll_offset = max_scroll;
                            follow_output = true;
                            Some(0)
                        }
                        _ => None,
                    };
                    if let Some(delta) = navigation {
                        scroll_offset = move_scroll(scroll_offset, max_scroll, delta);
                        if scroll_offset < max_scroll {
                            follow_output = false;
                        } else if delta > 0 {
                            follow_output = true;
                        }
                        continue;
                    }

                    let wants_quit = key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL));
                    if completion_prompt {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => break,
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter => {
                                completion_prompt = false;
                                completion_status = match mode {
                                    Some("doctor") => {
                                        "Diagnostics complete. Press q whenever you are ready to exit."
                                    }
                                    _ => {
                                        "Updates complete. Press q whenever you are ready to exit."
                                    }
                                }
                                .to_string();
                            }

                            KeyCode::Char('q') => break,
                            _ => {}
                        }
                    } else if wants_quit {
                        if let Some(update_child) = child.as_ref() {
                            if interrupt_requested {
                                output
                                    .push_back("An interrupt is already in progress.".to_string());
                            } else if let Err(error) = interrupt_update(update_child) {
                                output.push_back(format!("Could not interrupt updates: {error}"));
                            } else {
                                interrupt_requested = true;
                                output.push_back(
                                    "Interrupt requested; waiting for package managers to stop safely."
                                        .to_string(),
                                );
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    })();

    restore_terminal();
    result?;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{move_scroll, wrapped_line_count};
    use ratatui::text::Line;

    #[test]
    fn scroll_offset_stays_within_content() {
        assert_eq!(move_scroll(3, 10, -8), 0);
        assert_eq!(move_scroll(8, 10, 5), 10);
        assert_eq!(move_scroll(5, 5, 1), 5);
    }

    #[test]
    fn wrapped_line_count_accounts_for_long_lines() {
        let lines = vec![Line::from("123456"), Line::from("ab")];
        assert_eq!(wrapped_line_count(&lines, 3), 3);
    }
}
