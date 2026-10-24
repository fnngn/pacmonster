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
use ratatui::text::Line;
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

fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

fn forward_output<R: io::Read + Send + 'static>(stream: R, sender: Sender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(format!("Could not read update output: {error}"));
                    break;
                }
            }
        }
    });
}

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

fn authorize_updates() -> io::Result<bool> {
    if is_root()? {
        return Ok(true);
    }

    let status = Command::new("sudo").arg("-v").status().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("updates require root privileges; could not run `sudo -v`: {error}"),
        )
    })?;
    if status.success() {
        Ok(false)
    } else {
        Err(io::Error::other(
            "updates require root privileges; sudo authorization was not granted",
        ))
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

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

fn start_update(sender: &Sender<String>) -> io::Result<Child> {
    let running_as_root = authorize_updates()?;
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
        .args(["update", "--yes"])
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;

    if let Some(stdout) = child.stdout.take() {
        forward_output(stdout, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        forward_output(stderr, sender.clone());
    }
    Ok(child)
}

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

fn main() -> io::Result<()> {
    std::panic::set_hook(Box::new(|panic| {
        restore_terminal();
        eprintln!("{panic}");
    }));

    let run_updates = std::env::args().any(|argument| argument == "--run-updates");
    let (output_sender, output_receiver): (Sender<String>, Receiver<String>) = mpsc::channel();
    let mut output = VecDeque::new();
    let mut child = None;
    let mut completion_prompt = false;
    let mut completion_status = String::new();
    let mut interrupt_requested = false;
    let mut exit_code = 0;

    if run_updates {
        match start_update(&output_sender) {
            Ok(update_child) => {
                output.push_back("Starting all planned updates (--yes)...".to_string());
                child = Some(update_child);
            }
            Err(error) => {
                output.push_back(format!(
                    "Could not start `pacmonster update --yes`: {error}"
                ));
                completion_status = "Update process could not be started.".to_string();
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
            while let Ok(line) = output_receiver.try_recv() {
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
                completion_status =
                    format!("Updates finished (exit code {exit_code}). Exit TUI? [y/N]");
                completion_prompt = true;
            }

            let state = read_state();
            terminal.draw(|frame| {
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
                let visible_lines = chunks[1].height as usize;
                let scroll = log_lines.len().saturating_sub(visible_lines) as u16;
                frame.render_widget(
                    Paragraph::new(log_lines)
                        .wrap(Wrap { trim: true })
                        .scroll((scroll, 0)),
                    chunks[1],
                );

                let footer = if completion_prompt {
                    completion_status.as_str()
                } else if child.is_some() {
                    "Updates running — press q or Ctrl+C to request a safe interrupt"
                } else {
                    "Press q to quit"
                };
                frame.render_widget(Paragraph::new(footer), chunks[2]);
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    let wants_quit = key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL));
                    if completion_prompt {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => break,
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter => {
                                completion_prompt = false;
                                completion_status =
                                    "Updates complete. Press q whenever you are ready to exit."
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
