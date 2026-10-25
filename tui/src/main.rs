use std::collections::VecDeque;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

#[derive(Clone, Debug, serde::Deserialize)]
struct StateGhost {
    id: usize,
    busy: bool,
    target: Option<String>,
    #[serde(default)]
    progress: Option<u8>,
    #[serde(default)]
    elapsed_seconds: f64,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct StateFile {
    active: usize,
    idle: usize,
    capacity: usize,
    #[serde(default)]
    cpu_cores: Option<f64>,
    ghosts: Vec<StateGhost>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct DoctorCheck {
    name: String,
    status: String,
    detail: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct ArcheologistRecord {
    timestamp: String,
    level: String,
    source: String,
    message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArcheologistView {
    Menu,
    SearchInput,
    Results,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdvisorView {
    Menu,
    Help,
    Readme,
    License,
    Developer,
}

#[derive(Clone, Debug)]
struct ReadmeLine {
    text: String,
    heading: bool,
    code: bool,
    bullet: bool,
}

const START_MENU_ENTRIES: [(&str, &str); 6] = [
    (
        "GHOSTHUNTER",
        "Run all planned updates in the GhostPool dashboard",
    ),
    (
        "WITCHDOCTOR",
        "Examine system health or check Pacmonster installation",
    ),
    ("LIBRARIAN", "Browse and create package recovery snapshots"),
    (
        "ARCHEOLOGIST",
        "Search package-manager logs and investigate warnings",
    ),
    (
        "ADVISOR",
        "Read command help, project guide, and developer notes",
    ),
    ("RESTORATOR", "Open the recovery snapshot list in the CLI"),
];
const START_TUI_MODES: [&str; 5] = [
    "--run-updates",
    "--run-doctor",
    "--run-librarian",
    "--run-archeologist",
    "--run-advisor",
];
const EMBEDDED_README: &str = include_str!("../../README.md");
const EMBEDDED_LICENSE: &str = include_str!("../../LICENSE");

#[derive(Clone, Debug, serde::Deserialize)]
struct SnapshotTarget {
    name: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct LibrarianSnapshot {
    id: String,
    created: String,
    target: SnapshotTarget,
    package_count: usize,
    #[serde(default)]
    archive_bytes: u64,
    restorable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LibrarianView {
    Menu,
    SnapshotList,
    SnapshotDetail,
}

const PARCHMENT: Color = Color::Rgb(222, 205, 166);
const GOLD: Color = Color::Rgb(190, 151, 81);
const PETROL: Color = Color::Rgb(45, 112, 119);
const INK: Color = Color::Rgb(70, 49, 35);
const ERROR_RED: Color = Color::Rgb(133, 54, 48);

fn animated_ghost(frame: usize, id: usize) -> &'static str {
    const FRAMES: [&str; 4] = ["[o.o]", "[o_O]", "[O_o]", "[o.o]"];
    FRAMES[(frame + id) % FRAMES.len()]
}

fn progress_display(progress: Option<u8>, frame: usize, id: usize) -> String {
    const WIDTH: usize = 10;
    match progress {
        Some(progress) => {
            let filled = usize::from(progress.min(100)) * WIDTH / 100;
            format!(
                "[{}{}] [~{}%]",
                "#".repeat(filled),
                "-".repeat(WIDTH - filled),
                progress.min(100)
            )
        }
        None => {
            let start = (frame + id) % (WIDTH - 2);
            let bar: String = (0..WIDTH)
                .map(|index| {
                    if (start..start + 3).contains(&index) {
                        '#'
                    } else {
                        '-'
                    }
                })
                .collect();
            format!("[{bar}] [estimating]")
        }
    }
}

fn render_doctor(
    frame: &mut ratatui::Frame<'_>,
    checks: &[DoctorCheck],
    show_results: bool,
    selection: usize,
    notice: &str,
    scroll_offset: &mut u16,
    max_scroll: &mut u16,
    page_size: &mut u16,
    follow_output: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let title = if show_results {
        " WITCHDOCTOR  •  DIAGNOSTIC REPORT "
    } else {
        " WITCHDOCTOR  •  THE INFIRMARY "
    };
    let title_block = Block::default()
        .title(title)
        .title_style(
            Style::default()
                .fg(PETROL)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PETROL));
    let title_area = title_block.inner(chunks[0]);
    frame.render_widget(title_block, chunks[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            if show_results {
                notice
            } else {
                "A quiet examination of Pacmonster and its host."
            },
            Style::default().fg(GOLD),
        )),
        title_area,
    );

    let lines = if show_results {
        let mut report = vec![
            Line::from(Span::styled(
                format!(
                    "PASS {}   WARN {}   SKIP {}   FAIL {}",
                    checks.iter().filter(|check| check.status == "ok").count(),
                    checks
                        .iter()
                        .filter(|check| check.status == "warning")
                        .count(),
                    checks.iter().filter(|check| check.status == "skip").count(),
                    checks.iter().filter(|check| check.status == "fail").count(),
                ),
                Style::default()
                    .fg(PETROL)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Line::from(""),
        ];
        for check in checks {
            let (label, color) = match check.status.as_str() {
                "ok" => ("PASS", PETROL),
                "warning" => ("WARN", GOLD),
                "skip" => ("SKIP", GOLD),
                "fail" => ("FAIL", ERROR_RED),
                _ => ("????", PARCHMENT),
            };
            report.push(Line::from(vec![
                Span::styled(format!("[{label}] "), Style::default().fg(color)),
                Span::styled(check.name.clone(), Style::default().fg(PARCHMENT)),
            ]));
            report.push(Line::from(Span::styled(
                format!("       {}", check.detail),
                Style::default().fg(GOLD),
            )));
            report.push(Line::from(""));
        }
        report
    } else {
        [
            ("EXAMINE SYSTEM", "Run host and package-manager diagnostics"),
            (
                "CHECK INSTALLATION",
                "Verify Pacmonster CLI, Python package, and TUI",
            ),
            ("LEAVE THE INFIRMARY", "Return to the command line"),
        ]
        .iter()
        .enumerate()
        .flat_map(|(index, (label, description))| {
            let selected = index == selection;
            vec![
                Line::from(Span::styled(
                    format!(
                        " {} [{}] {}",
                        if selected { ">" } else { " " },
                        index + 1,
                        label
                    ),
                    Style::default()
                        .fg(if selected { PETROL } else { PARCHMENT })
                        .add_modifier(if selected {
                            ratatui::style::Modifier::BOLD
                        } else {
                            ratatui::style::Modifier::empty()
                        }),
                )),
                Line::from(Span::styled(
                    format!("       {description}"),
                    Style::default().fg(GOLD),
                )),
                Line::from(""),
            ]
        })
        .collect()
    };
    let report_block = Block::default()
        .title(if show_results {
            " EXAMINATION NOTES "
        } else {
            " SELECT AN ENTRY "
        })
        .title_style(Style::default().fg(GOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PETROL));
    let report_area = report_block.inner(chunks[1]);
    frame.render_widget(report_block, chunks[1]);
    *max_scroll = wrapped_line_count(&lines, report_area.width)
        .saturating_sub(report_area.height as usize)
        .min(u16::MAX as usize) as u16;
    *page_size = report_area.height.saturating_sub(1).max(1);
    if !show_results || follow_output {
        *scroll_offset = *max_scroll;
    }
    *scroll_offset = (*scroll_offset).min(*max_scroll);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        report_area,
    );
    let footer = if show_results {
        "↑/↓ READ  •  PGUP/PGDN PAGE  •  ESC RETURN TO MENU  •  Q EXIT"
    } else {
        "↑/↓ CHOOSE  •  ENTER OPEN  •  Q LEAVE"
    };
    frame.render_widget(
        Paragraph::new(Span::styled(footer, Style::default().fg(PETROL))),
        chunks[2],
    );
}

fn render_archeologist(
    frame: &mut ratatui::Frame<'_>,
    view: ArcheologistView,
    selection: usize,
    records: &[ArcheologistRecord],
    query: &str,
    notice: &str,
    scroll_offset: &mut u16,
    max_scroll: &mut u16,
    page_size: &mut u16,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let heading = match view {
        ArcheologistView::Results => " ARCHEOLOGIST  •  EXCAVATION NOTES ",
        _ => " ARCHEOLOGIST  •  THE RECORDS ROOM ",
    };
    let header = Block::default()
        .title(heading)
        .title_style(
            Style::default()
                .fg(PETROL)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PETROL));
    frame.render_widget(header, chunks[0]);
    let lines: Vec<Line> = match view {
        ArcheologistView::Menu => [
            (
                "READ ALL LOGS",
                "Browse Pacmonster and package-manager records",
            ),
            (
                "FIND WARNINGS & ERRORS",
                "Reduce the record to likely problems",
            ),
            (
                "SEARCH THE ARCHIVE",
                "Find a word or phrase in available logs",
            ),
            ("LEAVE THE RECORDS ROOM", "Return to the command line"),
        ]
        .iter()
        .enumerate()
        .flat_map(|(index, (label, description))| {
            let selected = index == selection;
            vec![
                Line::from(Span::styled(
                    format!(
                        " {} [{}] {}",
                        if selected { ">" } else { " " },
                        index + 1,
                        label
                    ),
                    Style::default()
                        .fg(if selected { PETROL } else { PARCHMENT })
                        .add_modifier(if selected {
                            ratatui::style::Modifier::BOLD
                        } else {
                            ratatui::style::Modifier::empty()
                        }),
                )),
                Line::from(Span::styled(
                    format!("       {description}"),
                    Style::default().fg(GOLD),
                )),
                Line::from(""),
            ]
        })
        .collect(),
        ArcheologistView::SearchInput => vec![
            Line::from(Span::styled(
                "Enter a term to search, then press ENTER:",
                Style::default().fg(GOLD),
            )),
            Line::from(Span::styled(
                format!("> {query}_"),
                Style::default().fg(PARCHMENT),
            )),
        ],
        ArcheologistView::Results if records.is_empty() => vec![Line::from(Span::styled(
            "NO RECORDS MATCHED THIS EXCAVATION.",
            Style::default().fg(GOLD),
        ))],
        ArcheologistView::Results => records
            .iter()
            .map(|record| {
                let color = match record.level.as_str() {
                    "error" => ERROR_RED,
                    "warning" => GOLD,
                    _ => PARCHMENT,
                };
                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", record.level.to_uppercase()),
                        Style::default().fg(color),
                    ),
                    Span::styled(
                        format!("{} {}: ", record.timestamp, record.source),
                        Style::default().fg(PETROL),
                    ),
                    Span::styled(record.message.clone(), Style::default().fg(PARCHMENT)),
                ])
            })
            .collect(),
    };
    let panel = Block::default()
        .title(if view == ArcheologistView::Results {
            " SEARCH RESULTS "
        } else {
            " SELECT AN ENTRY "
        })
        .title_style(Style::default().fg(GOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PETROL));
    let inner = panel.inner(chunks[1]);
    frame.render_widget(panel, chunks[1]);
    *max_scroll = wrapped_line_count(&lines, inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    *page_size = inner.height.saturating_sub(1).max(1);
    *scroll_offset = (*scroll_offset).min(*max_scroll);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        inner,
    );
    let footer = match view {
        ArcheologistView::Results => {
            "↑/↓ READ  •  PGUP/PGDN PAGE  •  ESC RETURN TO MENU  •  Q EXIT"
        }

        ArcheologistView::SearchInput => "TYPE QUERY  •  ENTER SEARCH  •  ESC CANCEL",
        ArcheologistView::Menu => "↑/↓ CHOOSE  •  ENTER OPEN  •  Q LEAVE",
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(footer, Style::default().fg(PETROL))),
        ]),
        chunks[2],
    );
}

fn render_start_menu(frame: &mut ratatui::Frame<'_>, selection: usize, notice: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let header = Block::default()
        .title(" PACMONSTER  •  THE READING ROOM ")
        .title_style(
            Style::default()
                .fg(PETROL)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PETROL));
    let header_area = header.inner(chunks[0]);
    frame.render_widget(header, chunks[0]);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Choose a department to begin.",
                Style::default()
                    .fg(PARCHMENT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "System care, records, and recovery under one roof.",
                Style::default().fg(GOLD),
            )),
        ]),
        header_area,
    );

    let mut lines = Vec::new();
    for (index, (name, description)) in START_MENU_ENTRIES.iter().enumerate() {
        lines.push(Line::from(Span::styled(
            format!(
                " {} [{}] {name}",
                if index == selection { ">" } else { " " },
                index + 1,
            ),
            Style::default()
                .fg(if index == selection {
                    PETROL
                } else {
                    PARCHMENT
                })
                .add_modifier(if index == selection {
                    ratatui::style::Modifier::BOLD
                } else {
                    ratatui::style::Modifier::empty()
                }),
        )));
        lines.push(Line::from(Span::styled(
            format!("       {description}"),
            Style::default().fg(GOLD),
        )));
        lines.push(Line::from(""));
    }
    let menu = Block::default()
        .title(" DEPARTMENTS ")
        .title_style(Style::default().fg(GOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PETROL));
    frame.render_widget(Paragraph::new(lines).block(menu), chunks[1]);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(
                "↑/↓ CHOOSE  •  ENTER OPEN  •  Q LEAVE",
                Style::default().fg(PETROL),
            )),
        ]),
        chunks[2],
    );
}

fn project_readme() -> io::Result<String> {
    if let Some(path) = env::var_os("PACMONSTER_README") {
        return fs::read_to_string(PathBuf::from(path));
    }
    let candidates = [
        env::current_dir()?.join("README.md"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("README.md"),
    ];
    for path in candidates {
        if path.is_file() {
            return fs::read_to_string(path);
        }
    }
    Ok(EMBEDDED_README.to_string())
}

fn project_license() -> io::Result<String> {
    if let Some(path) = env::var_os("PACMONSTER_LICENSE") {
        return fs::read_to_string(PathBuf::from(path));
    }
    let candidates = [
        env::current_dir()?.join("LICENSE"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("LICENSE"),
    ];
    for path in candidates {
        if path.is_file() {
            return fs::read_to_string(path);
        }
    }
    Ok(EMBEDDED_LICENSE.to_string())
}

fn strip_inline_markdown(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let remaining = &text[index..];
        let link = if remaining.starts_with('[') {
            remaining.find("](").and_then(|label_end| {
                remaining[label_end + 2..]
                    .find(')')
                    .map(|url_end| (label_end, url_end))
            })
        } else {
            None
        };
        if let Some((label_end, url_end)) = link {
            let label = &remaining[1..label_end];
            let url = &remaining[label_end + 2..label_end + 2 + url_end];
            output.push_str(label);
            if !url.is_empty() && url != label {
                output.push_str(" (");
                output.push_str(url);
                output.push(')');
            }
            index += label_end + 3 + url_end;
            continue;
        }
        if remaining.starts_with("**") || remaining.starts_with("__") || remaining.starts_with("~~")
        {
            index += 2;
            continue;
        }
        let character = remaining
            .chars()
            .next()
            .expect("remaining text is non-empty");
        if character == '*' || character == '`' {
            index += character.len_utf8();
        } else {
            output.push(character);
            index += character.len_utf8();
        }
    }
    output
}

fn markdown_list_item(text: &str) -> Option<(String, bool)> {
    if let Some(item) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")) {
        return Some((strip_inline_markdown(item), true));
    }
    let (number, item) = text.split_once(". ")?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    Some((format!("{number}. {}", strip_inline_markdown(item)), false))
}

fn format_readme(markdown: &str) -> Vec<ReadmeLine> {
    let mut in_code = false;
    let mut result: Vec<ReadmeLine> = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if trimmed.is_empty() {
            if !result.is_empty() && !result.last().is_some_and(|line| line.text.is_empty()) {
                result.push(ReadmeLine {
                    text: String::new(),
                    heading: false,
                    code: false,
                    bullet: false,
                });
            }
            continue;
        }
        if trimmed.chars().all(|character| character == '-') {
            continue;
        }

        let heading_depth = if in_code {
            0
        } else {
            trimmed
                .chars()
                .take_while(|character| *character == '#')
                .count()
        };
        let (text, bullet) = if in_code {
            (line.trim_end().to_string(), false)
        } else if heading_depth > 0 {
            (
                strip_inline_markdown(trimmed[heading_depth..].trim()).to_uppercase(),
                false,
            )
        } else if trimmed.starts_with('|') {
            let cells: Vec<_> = trimmed
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            if cells.iter().all(|cell| {
                cell.chars()
                    .all(|character| character == '-' || character == ':')
            }) {
                continue;
            }
            (
                cells
                    .iter()
                    .map(|cell| strip_inline_markdown(cell))
                    .collect::<Vec<_>>()
                    .join("  •  "),
                false,
            )
        } else if let Some((item, bullet)) = markdown_list_item(trimmed) {
            (item, bullet)
        } else {
            (
                strip_inline_markdown(trimmed.trim_start_matches('>').trim()),
                false,
            )
        };
        result.push(ReadmeLine {
            text,
            heading: heading_depth > 0,
            code: in_code,
            bullet,
        });
    }
    while result.last().is_some_and(|line| line.text.is_empty()) {
        result.pop();
    }
    result
}

fn format_license(text: &str) -> Vec<ReadmeLine> {
    let mut result: Vec<ReadmeLine> = text
        .lines()
        .map(|line| ReadmeLine {
            text: line.trim_end().to_string(),
            heading: false,
            code: false,
            bullet: false,
        })
        .collect();
    while result.first().is_some_and(|line| line.text.is_empty()) {
        result.remove(0);
    }
    while result.last().is_some_and(|line| line.text.is_empty()) {
        result.pop();
    }
    result
}

fn render_advisor(
    frame: &mut ratatui::Frame<'_>,
    view: AdvisorView,
    selection: usize,
    notice: &str,
    help_lines: &[String],
    readme_lines: &[ReadmeLine],
    scroll_offset: &mut u16,
    max_scroll: &mut u16,
    page_size: &mut u16,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let (title, panel_title) = match view {
        AdvisorView::Menu => (" ADVISOR  •  THE REFERENCE ROOM ", " AVAILABLE GUIDES "),
        AdvisorView::Help => (" ADVISOR  •  COMMAND REFERENCE ", " PACMONSTER HELP "),
        AdvisorView::Readme => (" ADVISOR  •  PROJECT HANDBOOK ", " README "),
        AdvisorView::License => (" ADVISOR  •  LICENSE ", " GNU GENERAL PUBLIC LICENSE "),
        AdvisorView::Developer => (" ADVISOR  •  DEVELOPER NOTES ", " PROJECT INFORMATION "),
    };
    let header = Block::default()
        .title(title)
        .title_style(
            Style::default()
                .fg(PETROL)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PETROL));
    frame.render_widget(header, chunks[0]);

    let lines: Vec<Line> = match view {
        AdvisorView::Menu => [
            ("PACMONSTER HELP", "Browse available CLI commands"),
            (
                "READ THE README",
                "Open the project handbook, generated from README.md",
            ),
            (
                "READ THE LICENSE",
                "Browse the complete GNU General Public License v3.0",
            ),
            (
                "DEVELOPER INFORMATION",
                "Inspect project, build, and contribution details",
            ),
            ("LEAVE THE REFERENCE ROOM", "Return to the previous menu"),
        ]
        .iter()
        .enumerate()
        .flat_map(|(index, (label, description))| {
            let selected = index == selection;
            vec![
                Line::from(Span::styled(
                    format!(
                        " {} [{}] {}",
                        if selected { ">" } else { " " },
                        index + 1,
                        label
                    ),
                    Style::default()
                        .fg(if selected { PETROL } else { PARCHMENT })
                        .add_modifier(if selected {
                            ratatui::style::Modifier::BOLD
                        } else {
                            ratatui::style::Modifier::empty()
                        }),
                )),
                Line::from(Span::styled(
                    format!("       {description}"),
                    Style::default().fg(GOLD),
                )),
                Line::from(""),
            ]
        })
        .collect(),
        AdvisorView::Help => help_lines
            .iter()
            .map(|line| Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT))))
            .collect(),
        AdvisorView::Readme => readme_lines
            .iter()
            .map(|line| {
                let text = if line.code {
                    format!("    {}", line.text)
                } else if line.bullet {
                    format!("  •  {}", line.text)
                } else {
                    line.text.clone()
                };
                let style = if line.heading {
                    Style::default()
                        .fg(PETROL)
                        .add_modifier(ratatui::style::Modifier::BOLD)
                } else if line.code {
                    Style::default().fg(GOLD)
                } else {
                    Style::default().fg(PARCHMENT)
                };
                Line::from(Span::styled(text, style))
            })
            .collect(),
        AdvisorView::License => readme_lines
            .iter()
            .map(|line| {
                let heading = line.text.trim_start();
                let style = if heading.starts_with("GNU GENERAL PUBLIC LICENSE")
                    || heading.starts_with("TERMS AND CONDITIONS")
                    || heading.starts_with("Preamble")
                {
                    Style::default()
                        .fg(PETROL)
                        .add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    Style::default().fg(PARCHMENT)
                };
                Line::from(Span::styled(line.text.clone(), style))
            })
            .collect(),
        AdvisorView::Developer => [
            format!("Pacmonster CLI version: {}", env!("CARGO_PKG_VERSION")),
            "Rust TUI: Ratatui / Crossterm".to_string(),
            "Minimum Rust toolchain: 1.85".to_string(),
            "Source repository: https://github.com/fnngn/pacmonster".to_string(),
            "License: GNU General Public License v3.0".to_string(),
            "Python package and CLI: pacmonster/".to_string(),
            "Terminal interfaces and navigation: tui/src/main.rs".to_string(),
            "Tests: python -m unittest discover -s tests".to_string(),
            "TUI checks: cd tui && cargo test && cargo build --release".to_string(),
        ]
        .iter()
        .map(|line| Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT))))
        .collect(),
    };
    let panel = Block::default()
        .title(panel_title)
        .title_style(Style::default().fg(GOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PETROL));
    let inner = panel.inner(chunks[1]);
    frame.render_widget(panel, chunks[1]);
    *max_scroll = wrapped_line_count(&lines, inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    *page_size = inner.height.saturating_sub(1).max(1);
    if view == AdvisorView::Menu {
        *scroll_offset = 0;
    } else {
        *scroll_offset = (*scroll_offset).min(*max_scroll);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        inner,
    );
    let footer = if view == AdvisorView::Menu {
        "↑/↓ CHOOSE  •  ENTER OPEN  •  Q LEAVE"
    } else {
        "↑/↓ READ  •  PGUP/PGDN PAGE  •  HOME/END JUMP  •  ESC RETURN"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(footer, Style::default().fg(PETROL))),
        ]),
        chunks[2],
    );
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

/// Start a Pacmonster argument list and stream its output to the UI.
fn start_command(
    sender: &Sender<(bool, String)>,
    arguments: &[&str],
    action: &str,
) -> io::Result<Child> {
    let requires_root = action != "advisor";
    let running_as_root = if requires_root {
        authorize_root(action)?
    } else {
        is_root()?
    };
    let use_sudo = requires_root && !running_as_root;
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("TUI source root could not be resolved"))?;

    let mut command = if source_root.join("pacmonster/main.py").is_file() {
        let mut python = if use_sudo {
            let mut sudo = Command::new("sudo");
            sudo.arg("-n").arg("python3");
            sudo
        } else {
            Command::new("python3")
        };
        python
            .args(["-m", "pacmonster.main"])
            .current_dir(source_root);
        python
    } else if PathBuf::from("/usr/local/bin/pacmonster").is_file() {
        let pacmonster = if use_sudo {
            let mut sudo = Command::new("sudo");
            sudo.args(["-n", "/usr/local/bin/pacmonster"]);
            sudo
        } else {
            Command::new("/usr/local/bin/pacmonster")
        };
        pacmonster
    } else if let Some(pacmonster) = find_executable("pacmonster").filter(|path| {
        !path.starts_with(env::var_os("HOME").map(PathBuf::from).unwrap_or_default())
    }) {
        if use_sudo {
            let mut sudo = Command::new("sudo");
            sudo.arg("-n").arg(pacmonster);
            sudo
        } else {
            Command::new(pacmonster)
        }
    } else if let Some((python, package_root)) = find_python_package() {
        if use_sudo {
            let mut sudo = Command::new("sudo");
            sudo.args(["-n", "env"])
                .arg(format!("PYTHONPATH={}", package_root.display()))
                .arg(python)
                .args(["-m", "pacmonster.main"]);
            sudo
        } else {
            let mut command = Command::new(python);
            command
                .args(["-m", "pacmonster.main"])
                .env("PYTHONPATH", package_root);
            command
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

fn render_librarian(
    frame: &mut ratatui::Frame<'_>,
    view: LibrarianView,
    menu_selection: usize,
    snapshot_selection: usize,
    snapshots: &[LibrarianSnapshot],
    detail_lines: &[String],
    output: &VecDeque<String>,
    notice: &str,
    operation_running: bool,
    scroll_offset: &mut u16,
    max_scroll: &mut u16,
    page_size: &mut u16,
    follow_output: bool,
) {
    let area = frame.area();
    let outer = Block::default()
        .title(" THE OLD LIBRARY  •  PACMONSTER ARCHIVES ")
        .title_style(
            Style::default()
                .fg(PETROL)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PETROL));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(inner);
    let banner = Paragraph::new(vec![
        Line::from(Span::styled(
            "  CATALOGUE OF SYSTEM STATES",
            Style::default()
                .fg(PARCHMENT)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  A RECORD OF EVERY PACKAGE, SHELVED FOR A DAY OF NEED",
            Style::default().fg(GOLD),
        )),
    ]);
    frame.render_widget(banner, chunks[0]);

    let (lines, scrollable) = if operation_running {
        (
            output
                .iter()
                .map(|line| {
                    Line::from(Span::styled(
                        format!("> {line}"),
                        Style::default().fg(PARCHMENT),
                    ))
                })
                .collect(),
            true,
        )
    } else {
        match view {
            LibrarianView::Menu => {
                let entries = [
                    (
                        "BROWSE THE CATALOGUE",
                        "Consult saved package records by system and date",
                    ),
                    (
                        "ENTER A NEW VOLUME",
                        "Preserve package versions and available archives",
                    ),
                    ("LEAVE THE READING ROOM", "Return to the command line"),
                ];
                let lines = entries
                    .iter()
                    .enumerate()
                    .flat_map(|(index, (label, description))| {
                        let selected = index == menu_selection;
                        vec![
                            Line::from(Span::styled(
                                format!(
                                    " {} [{}] {}",
                                    if selected { ">" } else { " " },
                                    index + 1,
                                    label
                                ),
                                Style::default()
                                    .fg(if selected { PETROL } else { PARCHMENT })
                                    .add_modifier(if selected {
                                        ratatui::style::Modifier::BOLD
                                    } else {
                                        ratatui::style::Modifier::empty()
                                    }),
                            )),
                            Line::from(Span::styled(
                                format!("       {description}"),
                                Style::default().fg(GOLD),
                            )),
                            Line::from(""),
                        ]
                    })
                    .collect();
                (lines, false)
            }
            LibrarianView::SnapshotList => {
                let mut lines = vec![
                    Line::from(Span::styled(
                        "  CATALOGUE  //  SELECT A RECORD TO EXAMINE",
                        Style::default()
                            .fg(PETROL)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    )),
                    Line::from(""),
                ];
                if snapshots.is_empty() {
                    lines.push(Line::from(
                        "  THIS SHELF IS EMPTY. ENTER A NEW VOLUME TO BEGIN.",
                    ));
                }
                for (index, snapshot) in snapshots.iter().enumerate() {
                    let selected = index == snapshot_selection;
                    let archive_status = if snapshot.restorable {
                        format!("RESTORABLE / {} bytes", snapshot.archive_bytes)
                    } else {
                        "MANIFEST ONLY".to_string()
                    };
                    lines.push(Line::from(Span::styled(
                        format!(
                            "{} {}  {}  {} packages  {}",
                            if selected { ">" } else { " " },
                            snapshot.target.name,
                            snapshot.created,
                            snapshot.package_count,
                            archive_status
                        ),
                        Style::default()
                            .fg(if selected { PETROL } else { PARCHMENT })
                            .add_modifier(if selected {
                                ratatui::style::Modifier::BOLD
                            } else {
                                ratatui::style::Modifier::empty()
                            }),
                    )));
                    lines.push(Line::from(Span::styled(
                        format!("    ID: {}", snapshot.id),
                        Style::default().fg(GOLD),
                    )));
                }
                (lines, true)
            }
            LibrarianView::SnapshotDetail => (
                detail_lines
                    .iter()
                    .map(|line| {
                        Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT)))
                    })
                    .collect(),
                true,
            ),
        }
    };

    if scrollable {
        *max_scroll = wrapped_line_count(&lines, chunks[1].width)
            .saturating_sub(chunks[1].height as usize)
            .min(u16::MAX as usize) as u16;
        *page_size = chunks[1].height.saturating_sub(1).max(1);
        if operation_running && follow_output {
            *scroll_offset = *max_scroll;
        } else if view == LibrarianView::SnapshotList && !snapshots.is_empty() {
            let selected_row = 2 + snapshot_selection * 2;
            if selected_row < usize::from(*scroll_offset) {
                *scroll_offset = selected_row as u16;
            } else if selected_row >= usize::from(*scroll_offset) + usize::from(*page_size) {
                *scroll_offset = selected_row.saturating_sub(usize::from(*page_size) - 1) as u16;
            }
        }
        *scroll_offset = (*scroll_offset).min(*max_scroll);
    } else {
        *scroll_offset = 0;
        *max_scroll = 0;
        *page_size = chunks[1].height.saturating_sub(1).max(1);
    }
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(INK));
    let panel_inner = panel.inner(chunks[1]);
    frame.render_widget(panel, chunks[1]);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        panel_inner,
    );
    let footer = if operation_running {
        "ARCHIVIST AT WORK  •  Q OR CTRL+C REQUESTS SAFE CANCELLATION"
    } else if notice.starts_with("LIBRARY ERROR:") {
        "ATTENTION REQUIRED  •  ESC/Q RETURN"
    } else {
        match view {
            LibrarianView::Menu => "↑/↓ CHOOSE  •  ENTER OPEN  •  Q LEAVE",
            LibrarianView::SnapshotList => {
                "↑/↓ SELECT  •  ENTER EXAMINE  •  S ADD VOLUME  •  ESC BACK"
            }
            LibrarianView::SnapshotDetail => {
                "↑/↓ READ  •  PGUP/PGDN PAGE  •  HOME/END JUMP  •  ESC RETURN TO CATALOGUE"
            }
        }
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(footer, Style::default().fg(PETROL))),
        ]),
        chunks[2],
    );
}

fn launch_librarian_command(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    sender: &Sender<(bool, String)>,
    arguments: &[&str],
    action: &str,
) -> io::Result<Child> {
    restore_terminal();
    let child_result = start_command(sender, arguments, action);
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?;
    child_result
}

fn launch_start_selection(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    selection: usize,
) -> io::Result<i32> {
    restore_terminal();
    let result = (|| {
        if selection < START_TUI_MODES.len() {
            let executable = env::current_exe()?;
            let status = Command::new(executable)
                .arg(START_TUI_MODES[selection])
                .status()?;
            return Ok(status.code().unwrap_or(1));
        }

        let running_as_root = authorize_root("restaurator")?;
        let executable = PathBuf::from("/usr/local/bin/pacmonster");
        let executable = if executable.is_file() {
            executable
        } else {
            find_executable("pacmonster")
                .filter(|path| {
                    !path.starts_with(env::var_os("HOME").map(PathBuf::from).unwrap_or_default())
                })
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "Pacmonster CLI not found; run `./install.sh` first",
                    )
                })?
        };
        let mut command = if running_as_root {
            Command::new(executable)
        } else {
            let mut sudo = Command::new("sudo");
            sudo.arg("-n").arg(executable);
            sudo
        };
        let status = command.args(["restaurator", "list"]).status()?;
        Ok(status.code().unwrap_or(1))
    })();

    let restore_result = terminal::enable_raw_mode().and_then(|()| {
        execute!(io::stdout(), EnterAlternateScreen)?;
        terminal.clear()?;
        Ok(())
    });
    restore_result?;
    result
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
    } else if std::env::args().any(|argument| argument == "--run-librarian") {
        Some("librarian")
    } else if std::env::args().any(|argument| argument == "--run-archeologist") {
        Some("archeologist")
    } else if std::env::args().any(|argument| argument == "--run-start") {
        Some("start")
    } else if std::env::args().any(|argument| argument == "--run-advisor") {
        Some("advisor")
    } else {
        None
    };
    let (output_sender, output_receiver): (Sender<(bool, String)>, Receiver<(bool, String)>) =
        mpsc::channel();
    let mut output = VecDeque::new();
    let mut command_stdout = Vec::new();
    let mut command_stderr = Vec::new();
    let mut doctor_checks: Vec<DoctorCheck> = Vec::new();
    let mut doctor_show_results = false;
    let mut doctor_menu_selection = 0usize;
    let mut doctor_notice = "THE INFIRMARY IS READY.".to_string();
    let mut archeologist_view = ArcheologistView::Menu;
    let mut archeologist_selection = 0usize;
    let mut archeologist_records: Vec<ArcheologistRecord> = Vec::new();
    let mut archeologist_query = String::new();
    let mut archeologist_notice = "THE RECORDS ROOM IS READY.".to_string();
    let mut start_menu_selection = 0usize;
    let mut start_menu_notice = "WELCOME TO PACMONSTER.".to_string();
    let mut advisor_view = AdvisorView::Menu;
    let mut advisor_selection = 0usize;
    let mut advisor_help_lines: Vec<String> = Vec::new();
    let mut advisor_readme_lines: Vec<ReadmeLine> = Vec::new();
    let mut advisor_notice = "THE REFERENCE ROOM IS READY.".to_string();
    let mut librarian_view = LibrarianView::Menu;
    let mut librarian_menu_selection = 0usize;
    let mut librarian_snapshot_selection = 0usize;
    let mut librarian_snapshots: Vec<LibrarianSnapshot> = Vec::new();
    let mut librarian_detail_lines: Vec<String> = Vec::new();
    let mut librarian_notice = "THE CATALOGUE IS READY.".to_string();
    let mut active_command: Option<String> = None;
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
    let animation_started = Instant::now();

    if let Some(mode) = mode.filter(|mode| *mode == "updates") {
        let (arguments, action): (&[&str], &str) = match mode {
            "updates" => (&["update", "--yes"], "updates"),
            _ => unreachable!(),
        };
        match start_command(&output_sender, arguments, action) {
            Ok(command_child) => {
                let description = match mode {
                    "updates" => "Starting all planned updates (--yes)...",
                    _ => unreachable!(),
                };
                output.push_back(description.to_string());
                active_command = Some(mode.to_string());
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
                } else if mode != Some("updates") {
                    command_stderr.push(line.clone());
                }
                if mode == Some("librarian") || mode == Some("archeologist") {
                    output.push_back(line);
                    if output.len() > 300 {
                        output.pop_front();
                    }
                } else {
                    output.push_back(line);
                    if output.len() > 300 {
                        output.pop_front();
                    }
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
            if child_finished && stdout_finished && !completion_prompt {
                if mode == Some("librarian") && exit_code != 0 {
                    let detail = command_stderr.join(" ").trim().to_string();
                    librarian_notice = if exit_code == 130 && !detail.is_empty() {
                        format!("OPERATION CANCELLED: {detail}")
                    } else if exit_code == 130 {
                        "OPERATION CANCELLED. TEMPORARY DATA WAS CLEANED UP.".to_string()
                    } else if detail.is_empty() {
                        format!(
                            "LIBRARY ERROR: {} exited with status {exit_code}.",
                            active_command.as_deref().unwrap_or("librarian")
                        )
                    } else {
                        format!("LIBRARY ERROR: {detail}")
                    };
                }
                match active_command.as_deref() {
                    Some("advisor-help") => {
                        match serde_json::from_str::<Vec<String>>(&command_stdout.join("\n")) {
                            Ok(lines) => {
                                advisor_help_lines = lines;
                                advisor_view = AdvisorView::Help;
                                advisor_notice = "CLI HELP LOADED.".to_string();
                                scroll_offset = 0;
                            }
                            Err(error) => {
                                advisor_view = AdvisorView::Help;
                                advisor_notice = format!("CLI HELP COULD NOT BE READ: {error}");
                            }
                        }
                    }
                    Some("doctor-diagnostics" | "doctor-installation") => {
                        match serde_json::from_str::<Vec<DoctorCheck>>(&command_stdout.join("\n")) {
                            Ok(checks) => {
                                doctor_checks = checks;
                                doctor_show_results = true;
                                scroll_offset = 0;
                                follow_output = false;
                                doctor_notice =
                                    if active_command.as_deref() == Some("doctor-installation") {
                                        "INSTALLATION CHECK COMPLETE.".to_string()
                                    } else {
                                        "SYSTEM DIAGNOSIS COMPLETE.".to_string()
                                    };
                            }
                            Err(error) => {
                                doctor_show_results = true;
                                doctor_notice =
                                    format!("DIAGNOSTIC RESULTS COULD NOT BE READ: {error}");
                                output.push_back(command_stderr.join(" "));
                            }
                        }
                    }
                    Some("archeologist-all" | "archeologist-issues" | "archeologist-search") => {
                        match serde_json::from_str::<Vec<ArcheologistRecord>>(
                            &command_stdout.join("\n"),
                        ) {
                            Ok(records) => {
                                archeologist_records = records;
                                archeologist_view = ArcheologistView::Results;
                                archeologist_notice =
                                    format!("{} RECORD(S) UNEARTHED.", archeologist_records.len());
                                scroll_offset = 0;
                                follow_output = false;
                            }
                            Err(error) => {
                                archeologist_notice =
                                    format!("LOG RECORDS COULD NOT BE READ: {error}");
                                archeologist_view = ArcheologistView::Results;
                            }
                        }
                    }
                    Some("librarian-list" | "librarian-snapshot") if exit_code == 0 => {
                        match serde_json::from_str::<Vec<LibrarianSnapshot>>(
                            &command_stdout.join("\n"),
                        ) {
                            Ok(snapshots) => {
                                librarian_snapshots = snapshots;
                                librarian_snapshot_selection = 0;
                                librarian_view = LibrarianView::SnapshotList;
                                librarian_notice =
                                    if active_command.as_deref() == Some("librarian-snapshot") {
                                        format!(
                                            "VOLUME ENTERED. {} RECORD(S) NOW SHELVED.",
                                            librarian_snapshots.len()
                                        )
                                    } else {
                                        format!(
                                            "CATALOGUE OPEN. {} RECORD(S) FOUND.",
                                            librarian_snapshots.len()
                                        )
                                    };
                            }
                            Err(error) => {
                                librarian_notice =
                                    format!("ARCHIVE DATA COULD NOT BE READ: {error}");
                                librarian_view = LibrarianView::SnapshotList;
                            }
                        }
                    }
                    Some("librarian-show") if exit_code == 0 => {
                        match serde_json::from_str::<serde_json::Value>(&command_stdout.join("\n"))
                        {
                            Ok(detail) => {
                                librarian_detail_lines = serde_json::to_string_pretty(&detail)
                                    .unwrap_or_else(|error| {
                                        format!("Could not format details: {error}")
                                    })
                                    .lines()
                                    .map(str::to_owned)
                                    .collect();
                                librarian_view = LibrarianView::SnapshotDetail;
                                librarian_notice =
                                    "CATALOGUE RECORD // PRESS ESC TO RETURN".to_string();
                                scroll_offset = 0;
                                follow_output = false;
                            }
                            Err(error) => {
                                librarian_notice =
                                    format!("CATALOGUE RECORD COULD NOT BE READ: {error}");
                                librarian_view = LibrarianView::SnapshotList;
                            }
                        }
                    }
                    _ => {}
                }
                if mode == Some("updates") {
                    completion_status =
                        format!("Updates finished (exit code {exit_code}). Exit TUI? [y/N]");
                    completion_prompt = true;
                }
                child_finished = false;
                stdout_finished = false;
                interrupt_requested = false;
                command_stdout.clear();
                command_stderr.clear();
                active_command = None;
            }

            let state = read_state();
            let animation_frame = (animation_started.elapsed().as_millis() / 250) as usize;
            terminal.draw(|frame| {
                if mode == Some("start") {
                    render_start_menu(frame, start_menu_selection, &start_menu_notice);
                    return;
                }
                if mode == Some("advisor") {
                    render_advisor(
                        frame,
                        advisor_view,
                        advisor_selection,
                        &advisor_notice,
                        &advisor_help_lines,
                        &advisor_readme_lines,
                        &mut scroll_offset,
                        &mut max_scroll,
                        &mut page_size,
                    );
                    return;
                }
                if mode == Some("librarian") {
                    render_librarian(
                        frame,
                        librarian_view,
                        librarian_menu_selection,
                        librarian_snapshot_selection,
                        &librarian_snapshots,
                        &librarian_detail_lines,
                        &output,
                        &librarian_notice,
                        child.is_some(),
                        &mut scroll_offset,
                        &mut max_scroll,
                        &mut page_size,
                        follow_output,
                    );
                    return;
                }
                if mode == Some("doctor") {
                    render_doctor(
                        frame,
                        &doctor_checks,
                        doctor_show_results,
                        doctor_menu_selection,
                        &doctor_notice,
                        &mut scroll_offset,
                        &mut max_scroll,
                        &mut page_size,
                        follow_output,
                    );
                    return;
                }
                if mode == Some("archeologist") {
                    render_archeologist(
                        frame,
                        archeologist_view,
                        archeologist_selection,
                        &archeologist_records,
                        &archeologist_query,
                        &archeologist_notice,
                        &mut scroll_offset,
                        &mut max_scroll,
                        &mut page_size,
                    );
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
                    Line::from(Span::styled(
                        "GHOST POOL",
                        Style::default()
                            .fg(PARCHMENT)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("ᗣ Active      ", Style::default().fg(PETROL)),
                        Span::styled(active.to_string(), Style::default().fg(GOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("◌ Idle        ", Style::default().fg(GOLD)),
                        Span::styled(idle.to_string(), Style::default().fg(PARCHMENT)),
                    ]),
                    Line::from(vec![
                        Span::styled("◌ Available   ", Style::default().fg(GOLD)),
                        Span::styled(available.to_string(), Style::default().fg(PARCHMENT)),
                    ]),
                    Line::from(vec![
                        Span::styled("CPU capacity  ", Style::default().fg(GOLD)),
                        Span::styled(
                            format!(
                                "{} core(s)",
                                state
                                    .as_ref()
                                    .and_then(|snapshot| snapshot.cpu_cores)
                                    .map(|cores| format!("{cores}"))
                                    .unwrap_or_else(|| "unknown".to_string())
                            ),
                            Style::default().fg(PARCHMENT),
                        ),
                    ]),
                ];
                let pool_block = Block::default()
                    .title(" THE GHOST POOL ")
                    .title_style(Style::default().fg(GOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(PETROL));
                frame.render_widget(Paragraph::new(capacity_lines).block(pool_block), chunks[0]);

                let mut log_lines: Vec<Line> = state
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .ghosts
                            .iter()
                            .filter(|ghost| ghost.busy)
                            .map(|ghost| {
                                let progress =
                                    progress_display(ghost.progress, animation_frame, ghost.id);
                                Line::from(vec![
                                    Span::styled(
                                        animated_ghost(animation_frame, ghost.id),
                                        Style::default()
                                            .fg(PETROL)
                                            .add_modifier(ratatui::style::Modifier::BOLD),
                                    ),
                                    Span::styled(
                                        format!(" G{:02} ", ghost.id),
                                        Style::default().fg(GOLD),
                                    ),
                                    Span::styled(progress, Style::default().fg(GOLD)),
                                    Span::raw(" "),
                                    Span::styled(
                                        ghost.target.as_deref().unwrap_or_default().to_string(),
                                        Style::default().fg(PARCHMENT),
                                    ),
                                    Span::styled(
                                        format!("  {:.0}s", ghost.elapsed_seconds),
                                        Style::default().fg(PARCHMENT),
                                    ),
                                ])
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                log_lines.extend(output.iter().map(|line| {
                    Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT)))
                }));
                let log_block = Block::default()
                    .title(" CURRENT READING ")
                    .title_style(Style::default().fg(GOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(PETROL));
                let log_area = log_block.inner(chunks[1]);
                frame.render_widget(log_block, chunks[1]);
                max_scroll = wrapped_line_count(&log_lines, log_area.width)
                    .saturating_sub(log_area.height as usize)
                    .min(u16::MAX as usize) as u16;
                page_size = chunks[1].height.saturating_sub(1).max(1);
                if follow_output {
                    scroll_offset = max_scroll;
                }
                scroll_offset = scroll_offset.min(max_scroll);
                let paragraph = Paragraph::new(log_lines).wrap(Wrap { trim: true });
                frame.render_widget(paragraph.scroll((scroll_offset, 0)), log_area);

                let footer = if completion_prompt {
                    completion_status.as_str()
                } else if child.is_some() {
                    "↑/↓ PgUp/PgDn scroll • Home/End jump • q or Ctrl+C requests safe interrupt"
                } else {
                    "↑/↓ PgUp/PgDn scroll • Home/End jump • q to quit"
                };
                frame.render_widget(
                    Paragraph::new(Span::styled(footer, Style::default().fg(GOLD))),
                    chunks[2],
                );
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if mode == Some("start") {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                start_menu_selection = start_menu_selection.saturating_sub(1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                start_menu_selection =
                                    (start_menu_selection + 1).min(START_MENU_ENTRIES.len() - 1);
                            }
                            KeyCode::Char(digit @ '1'..='6') => {
                                start_menu_selection =
                                    digit.to_digit(10).unwrap_or(1).saturating_sub(1) as usize;
                            }
                            KeyCode::Enter => {
                                let selection = start_menu_selection;
                                start_menu_notice =
                                    format!("OPENING {}...", START_MENU_ENTRIES[selection].0);
                                match launch_start_selection(&mut terminal, selection) {
                                    Ok(0) => {
                                        start_menu_notice = format!(
                                            "{} RETURNED TO THE READING ROOM.",
                                            START_MENU_ENTRIES[selection].0
                                        );
                                    }
                                    Ok(status) => {
                                        start_menu_notice = format!(
                                            "{} CLOSED WITH STATUS {status}.",
                                            START_MENU_ENTRIES[selection].0
                                        );
                                    }
                                    Err(error) => {
                                        start_menu_notice =
                                            format!("COULD NOT OPEN DEPARTMENT: {error}");
                                    }
                                }
                            }
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            _ => {}
                        }
                        continue;
                    }

                    if mode == Some("advisor") {
                        if child.is_some() {
                            if key.code == KeyCode::Char('q')
                                || key.code == KeyCode::Esc
                                || (key.code == KeyCode::Char('c')
                                    && key.modifiers.contains(KeyModifiers::CONTROL))
                            {
                                break;
                            }
                            continue;
                        }
                        let mut command_to_start: Option<(String, Vec<String>)> = None;
                        match advisor_view {
                            AdvisorView::Menu => match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    advisor_selection = advisor_selection.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    advisor_selection = (advisor_selection + 1).min(4);
                                }
                                KeyCode::Char('1') => advisor_selection = 0,
                                KeyCode::Char('2') => advisor_selection = 1,
                                KeyCode::Char('3') => advisor_selection = 2,
                                KeyCode::Char('4') => advisor_selection = 3,
                                KeyCode::Char('5') => advisor_selection = 4,
                                KeyCode::Char('q') | KeyCode::Esc => break,
                                KeyCode::Enter => match advisor_selection {
                                    0 => {
                                        advisor_notice =
                                            "CONSULTING PACMONSTER HELP...".to_string();
                                        command_to_start = Some((
                                            "advisor-help".to_string(),
                                            vec!["help".into(), "--json".into()],
                                        ));
                                    }
                                    1 => match project_readme() {
                                        Ok(markdown) => {
                                            advisor_readme_lines = format_readme(&markdown);
                                            advisor_view = AdvisorView::Readme;
                                            advisor_notice = "README LOADED.".to_string();
                                            scroll_offset = 0;
                                        }
                                        Err(error) => {
                                            advisor_notice =
                                                format!("README COULD NOT BE READ: {error}");
                                        }
                                    },
                                    2 => match project_license() {
                                        Ok(license) => {
                                            advisor_readme_lines = format_license(&license);
                                            advisor_view = AdvisorView::License;
                                            advisor_notice = "LICENSE LOADED.".to_string();
                                            scroll_offset = 0;
                                        }
                                        Err(error) => {
                                            advisor_notice =
                                                format!("LICENSE COULD NOT BE READ: {error}");
                                        }
                                    },
                                    3 => {
                                        advisor_view = AdvisorView::Developer;
                                        advisor_notice = "PROJECT INFORMATION.".to_string();
                                        scroll_offset = 0;
                                    }
                                    _ => break,
                                },
                                _ => {}
                            },
                            AdvisorView::Help
                            | AdvisorView::Readme
                            | AdvisorView::License
                            | AdvisorView::Developer => match key.code {
                                KeyCode::Esc | KeyCode::Backspace => {
                                    advisor_view = AdvisorView::Menu;
                                    scroll_offset = 0;
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    scroll_offset = move_scroll(scroll_offset, max_scroll, -1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    scroll_offset = move_scroll(scroll_offset, max_scroll, 1);
                                }
                                KeyCode::PageUp => {
                                    scroll_offset = move_scroll(
                                        scroll_offset,
                                        max_scroll,
                                        -i32::from(page_size),
                                    );
                                }
                                KeyCode::PageDown => {
                                    scroll_offset = move_scroll(
                                        scroll_offset,
                                        max_scroll,
                                        i32::from(page_size),
                                    );
                                }
                                KeyCode::Home => scroll_offset = 0,
                                KeyCode::End => scroll_offset = max_scroll,
                                KeyCode::Char('q') => break,
                                _ => {}
                            },
                        }

                        if let Some((action, arguments)) = command_to_start {
                            command_stdout.clear();
                            command_stderr.clear();
                            active_command = Some(action);
                            child_finished = false;
                            stdout_finished = false;
                            let borrowed_arguments: Vec<&str> =
                                arguments.iter().map(String::as_str).collect();
                            match launch_librarian_command(
                                &mut terminal,
                                &output_sender,
                                &borrowed_arguments,
                                "advisor",
                            ) {
                                Ok(command_child) => child = Some(command_child),
                                Err(error) => {
                                    active_command = None;
                                    advisor_notice = format!("HELP COULD NOT BE STARTED: {error}");
                                }
                            }
                        }
                        continue;
                    }

                    if (mode == Some("doctor") || mode == Some("archeologist")) && child.is_none() {
                        let mut command_to_start: Option<(String, Vec<String>, String)> = None;
                        if mode == Some("doctor") {
                            if doctor_show_results {
                                match key.code {
                                    KeyCode::Esc | KeyCode::Backspace => {
                                        doctor_show_results = false;
                                        doctor_checks.clear();
                                        doctor_notice = "THE INFIRMARY IS READY.".to_string();
                                        scroll_offset = 0;
                                        follow_output = true;
                                    }
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        scroll_offset = move_scroll(scroll_offset, max_scroll, -1);
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        scroll_offset = move_scroll(scroll_offset, max_scroll, 1);
                                    }
                                    KeyCode::PageUp => {
                                        scroll_offset = move_scroll(
                                            scroll_offset,
                                            max_scroll,
                                            -i32::from(page_size),
                                        );
                                    }
                                    KeyCode::PageDown => {
                                        scroll_offset = move_scroll(
                                            scroll_offset,
                                            max_scroll,
                                            i32::from(page_size),
                                        );
                                    }
                                    KeyCode::Home => scroll_offset = 0,
                                    KeyCode::End => scroll_offset = max_scroll,
                                    KeyCode::Char('q') => break,
                                    _ => {}
                                }
                            } else {
                                match key.code {
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        doctor_menu_selection =
                                            doctor_menu_selection.saturating_sub(1);
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        doctor_menu_selection = (doctor_menu_selection + 1).min(2);
                                    }
                                    KeyCode::Char('1') => doctor_menu_selection = 0,
                                    KeyCode::Char('2') => doctor_menu_selection = 1,
                                    KeyCode::Char('3') => doctor_menu_selection = 2,
                                    KeyCode::Char('q') => break,
                                    KeyCode::Esc => break,
                                    KeyCode::Enter => match doctor_menu_selection {
                                        0 => {
                                            command_to_start = Some((
                                                "doctor-diagnostics".to_string(),
                                                vec!["doctor".into(), "--json".into()],
                                                "witchdoctor".to_string(),
                                            ));
                                            doctor_notice = "EXAMINING THE HOST...".to_string();
                                        }
                                        1 => {
                                            command_to_start = Some((
                                                "doctor-installation".to_string(),
                                                vec![
                                                    "doctor".into(),
                                                    "--installation".into(),
                                                    "--json".into(),
                                                ],
                                                "witchdoctor".to_string(),
                                            ));
                                            doctor_notice =
                                                "VERIFYING PACMONSTER INSTALLATION...".to_string();
                                        }
                                        _ => break,
                                    },
                                    _ => {}
                                }
                            }
                        } else {
                            match archeologist_view {
                                ArcheologistView::Menu => match key.code {
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        archeologist_selection =
                                            archeologist_selection.saturating_sub(1);
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        archeologist_selection =
                                            (archeologist_selection + 1).min(3);
                                    }
                                    KeyCode::Enter => match archeologist_selection {
                                        0 | 1 => {
                                            let issues = archeologist_selection == 1;
                                            command_to_start = Some((
                                                if issues {
                                                    "archeologist-issues".to_string()
                                                } else {
                                                    "archeologist-all".to_string()
                                                },
                                                vec![
                                                    "archeologist".into(),
                                                    if issues { "issues" } else { "all" }.into(),
                                                    "--json".into(),
                                                ],
                                                "archeologist".to_string(),
                                            ));
                                            archeologist_notice = if issues {
                                                "SEARCHING FOR WARNINGS AND ERRORS...".to_string()
                                            } else {
                                                "READING AVAILABLE LOGS...".to_string()
                                            };
                                        }
                                        2 => {
                                            archeologist_view = ArcheologistView::SearchInput;
                                            archeologist_query.clear();
                                            archeologist_notice =
                                                "ENTER A SEARCH TERM.".to_string();
                                        }
                                        _ => break,
                                    },
                                    KeyCode::Char('1') => archeologist_selection = 0,
                                    KeyCode::Char('2') => archeologist_selection = 1,
                                    KeyCode::Char('3') => archeologist_selection = 2,
                                    KeyCode::Char('4') => archeologist_selection = 3,
                                    KeyCode::Char('q') | KeyCode::Esc => break,
                                    _ => {}
                                },
                                ArcheologistView::SearchInput => match key.code {
                                    KeyCode::Esc => {
                                        archeologist_view = ArcheologistView::Menu;
                                        archeologist_query.clear();
                                    }
                                    KeyCode::Enter if !archeologist_query.trim().is_empty() => {
                                        command_to_start = Some((
                                            "archeologist-search".to_string(),
                                            vec![
                                                "archeologist".into(),
                                                "search".into(),
                                                archeologist_query.clone(),
                                                "--json".into(),
                                            ],
                                            "archeologist".to_string(),
                                        ));
                                        archeologist_notice =
                                            "SEARCHING THE RECORDS...".to_string();
                                    }
                                    KeyCode::Backspace => {
                                        archeologist_query.pop();
                                    }
                                    KeyCode::Char(character)
                                        if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                                    {
                                        archeologist_query.push(character);
                                    }
                                    _ => {}
                                },
                                ArcheologistView::Results => match key.code {
                                    KeyCode::Esc | KeyCode::Backspace => {
                                        archeologist_view = ArcheologistView::Menu;
                                        archeologist_records.clear();
                                        archeologist_selection = 0;
                                        scroll_offset = 0;
                                    }
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        scroll_offset = move_scroll(scroll_offset, max_scroll, -1);
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        scroll_offset = move_scroll(scroll_offset, max_scroll, 1);
                                    }
                                    KeyCode::PageUp => {
                                        scroll_offset = move_scroll(
                                            scroll_offset,
                                            max_scroll,
                                            -i32::from(page_size),
                                        );
                                    }
                                    KeyCode::PageDown => {
                                        scroll_offset = move_scroll(
                                            scroll_offset,
                                            max_scroll,
                                            i32::from(page_size),
                                        );
                                    }
                                    KeyCode::Home => scroll_offset = 0,
                                    KeyCode::End => scroll_offset = max_scroll,
                                    KeyCode::Char('q') => break,
                                    _ => {}
                                },
                            }
                        }

                        if let Some((action, arguments, authorization_action)) = command_to_start {
                            output.clear();
                            command_stdout.clear();
                            command_stderr.clear();
                            active_command = Some(action.clone());
                            child_finished = false;
                            stdout_finished = false;
                            let borrowed_arguments: Vec<&str> =
                                arguments.iter().map(String::as_str).collect();
                            match launch_librarian_command(
                                &mut terminal,
                                &output_sender,
                                &borrowed_arguments,
                                &authorization_action,
                            ) {
                                Ok(command_child) => child = Some(command_child),
                                Err(error) => {
                                    active_command = None;
                                    if mode == Some("doctor") {
                                        doctor_notice =
                                            format!("DIAGNOSTIC COULD NOT START: {error}");
                                        doctor_show_results = true;
                                    } else {
                                        archeologist_notice =
                                            format!("EXCAVATION COULD NOT START: {error}");
                                        archeologist_view = ArcheologistView::Results;
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    if mode == Some("librarian") && child.is_none() {
                        let mut command_to_start: Option<(String, Vec<String>)> = None;
                        match librarian_view {
                            LibrarianView::Menu => match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    librarian_menu_selection =
                                        librarian_menu_selection.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    librarian_menu_selection =
                                        (librarian_menu_selection + 1).min(2);
                                }
                                KeyCode::Enter => match librarian_menu_selection {
                                    0 => {
                                        librarian_notice = "OPENING THE CATALOGUE...".to_string();
                                        command_to_start = Some((
                                            "librarian-list".to_string(),
                                            vec![
                                                "librarian".into(),
                                                "list".into(),
                                                "--json".into(),
                                            ],
                                        ));
                                    }
                                    1 => {
                                        librarian_notice = "ENTERING NEW VOLUMES...".to_string();
                                        command_to_start = Some((
                                            "librarian-snapshot".to_string(),
                                            vec![
                                                "librarian".into(),
                                                "snapshot".into(),
                                                "--json".into(),
                                            ],
                                        ));
                                    }
                                    _ => break,
                                },
                                KeyCode::Char('1') => {
                                    librarian_menu_selection = 0;
                                }
                                KeyCode::Char('2') => {
                                    librarian_menu_selection = 1;
                                }
                                KeyCode::Char('3') | KeyCode::Char('q') | KeyCode::Esc => break,
                                _ => {}
                            },
                            LibrarianView::SnapshotList => match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    librarian_snapshot_selection =
                                        librarian_snapshot_selection.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if !librarian_snapshots.is_empty() {
                                        librarian_snapshot_selection =
                                            (librarian_snapshot_selection + 1)
                                                .min(librarian_snapshots.len() - 1);
                                    }
                                }
                                KeyCode::Enter if !librarian_snapshots.is_empty() => {
                                    let snapshot_id = librarian_snapshots
                                        [librarian_snapshot_selection]
                                        .id
                                        .clone();
                                    librarian_notice = "RETRIEVING CATALOGUE RECORD...".to_string();
                                    command_to_start = Some((
                                        "librarian-show".to_string(),
                                        vec![
                                            "librarian".into(),
                                            "show".into(),
                                            snapshot_id,
                                            "--json".into(),
                                        ],
                                    ));
                                }
                                KeyCode::Char('s') => {
                                    librarian_notice = "ENTERING NEW VOLUMES...".to_string();
                                    command_to_start = Some((
                                        "librarian-snapshot".to_string(),
                                        vec![
                                            "librarian".into(),
                                            "snapshot".into(),
                                            "--json".into(),
                                        ],
                                    ));
                                }
                                KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('q') => {
                                    librarian_view = LibrarianView::Menu;
                                }
                                _ => {}
                            },
                            LibrarianView::SnapshotDetail => match key.code {
                                KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('q') => {
                                    librarian_view = LibrarianView::SnapshotList;
                                    scroll_offset = 0;
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    scroll_offset = move_scroll(scroll_offset, max_scroll, -1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    scroll_offset = move_scroll(scroll_offset, max_scroll, 1);
                                }
                                KeyCode::PageUp => {
                                    scroll_offset = move_scroll(
                                        scroll_offset,
                                        max_scroll,
                                        -i32::from(page_size),
                                    );
                                }
                                KeyCode::PageDown => {
                                    scroll_offset = move_scroll(
                                        scroll_offset,
                                        max_scroll,
                                        i32::from(page_size),
                                    );
                                }
                                KeyCode::Home => scroll_offset = 0,
                                KeyCode::End => scroll_offset = max_scroll,
                                _ => {}
                            },
                        }

                        if let Some((action, arguments)) = command_to_start {
                            output.clear();
                            command_stdout.clear();
                            command_stderr.clear();
                            active_command = Some(action.clone());
                            child_finished = false;
                            stdout_finished = false;
                            follow_output = true;
                            let borrowed_arguments: Vec<&str> =
                                arguments.iter().map(String::as_str).collect();
                            match launch_librarian_command(
                                &mut terminal,
                                &output_sender,
                                &borrowed_arguments,
                                "librarian",
                            ) {
                                Ok(command_child) => child = Some(command_child),
                                Err(error) => {
                                    active_command = None;
                                    librarian_notice = format!("LIBRARY ERROR: {error}");
                                }
                            }
                        }
                        continue;
                    }

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
                                let message = "An interrupt is already in progress.".to_string();
                                if mode == Some("librarian") {
                                    librarian_notice = message;
                                } else {
                                    output.push_back(message);
                                }
                            } else if let Err(error) = interrupt_update(update_child) {
                                let message = format!("Could not interrupt operation: {error}");
                                if mode == Some("librarian") {
                                    librarian_notice = message;
                                } else {
                                    output.push_back(message);
                                }
                            } else {
                                interrupt_requested = true;
                                let message =
                                    "Interrupt requested; waiting for the operation to stop safely."
                                        .to_string();
                                if mode == Some("librarian") {
                                    librarian_notice = message;
                                } else {
                                    output.push_back(message);
                                }
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
    use super::{
        START_MENU_ENTRIES, START_TUI_MODES, animated_ghost, format_license, format_readme,
        move_scroll, progress_display, wrapped_line_count,
    };
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

    #[test]
    fn ghost_animation_cycles_and_offsets_each_ghost() {
        assert_eq!(animated_ghost(0, 0), "[o.o]");
        assert_eq!(animated_ghost(1, 0), "[o_O]");
        assert_eq!(animated_ghost(0, 1), "[o_O]");
        assert_eq!(animated_ghost(4, 2), "[O_o]");
    }

    #[test]
    fn progress_display_distinguishes_estimates_from_indeterminate_work() {
        assert_eq!(progress_display(Some(70), 0, 0), "[#######---] [~70%]");
        assert!(progress_display(None, 0, 0).contains("[estimating]"));
        assert_ne!(progress_display(None, 0, 0), progress_display(None, 1, 0));
    }

    #[test]
    fn start_menu_tui_entries_match_their_launch_modes() {
        assert_eq!(START_MENU_ENTRIES.len(), START_TUI_MODES.len() + 1);
        assert_eq!(START_MENU_ENTRIES[4].0, "ADVISOR");
        assert_eq!(START_TUI_MODES[4], "--run-advisor");
        assert_eq!(START_MENU_ENTRIES[5].0, "RESTORATOR");
    }

    #[test]
    fn readme_formatter_preserves_headings_lists_and_code() {
        let lines = format_readme("# Guide\n\n- item\n\n```sh\npacmonster help\n```");

        assert!(lines[0].heading);
        assert!(lines.iter().any(|line| line.bullet && line.text == "item"));
        assert!(
            lines
                .iter()
                .any(|line| line.code && line.text == "pacmonster help")
        );
        assert!(lines.iter().any(|line| line.text.is_empty()));
    }

    #[test]
    fn readme_formatter_removes_markdown_syntax_and_formats_tables() {
        let lines = format_readme(
            "**Bold** and `inline code` with [a link](https://example.test).\n\n| Name | Command |\n|---|---|\n| Advisor | `pacmonster advisor` |",
        );

        assert_eq!(
            lines[0].text,
            "Bold and inline code with a link (https://example.test)."
        );
        assert!(
            lines
                .iter()
                .any(|line| line.text == "Advisor  •  pacmonster advisor")
        );
        assert!(lines.iter().all(|line| !line.text.contains('|')));
    }

    #[test]
    fn license_formatter_keeps_paragraph_spacing_and_text() {
        let lines = format_license("  LICENSE TITLE  \n\n First paragraph.\nSecond line.\n");

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].text, "  LICENSE TITLE");
        assert!(lines[1].text.is_empty());
        assert_eq!(lines[2].text, " First paragraph.");
        assert_eq!(lines[3].text, "Second line.");
    }
}
