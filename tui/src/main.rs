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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GhosthunterView {
    Menu,
    Preview,
    Report,
    Dashboard,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
struct UpdateReport {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    summary: UpdateSummary,
    #[serde(default)]
    updated_systems: Vec<UpdateSystem>,
    #[serde(default)]
    failed_systems: Vec<UpdateSystem>,
    #[serde(default)]
    systems: Vec<UpdateSystem>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
struct UpdateSummary {
    systems_attempted: usize,
    systems_succeeded: usize,
    systems_failed: usize,
    packages_changed: usize,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
struct UpdateSystem {
    id: String,
    name: String,
    #[serde(default)]
    packages: Vec<UpdatePackage>,
    #[serde(default)]
    package_report_error: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
struct UpdatePackage {
    name: String,
    change: String,
    previous_version: Option<String>,
    current_version: Option<String>,
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
        "RESTORER",
        "Review snapshots and restore selected package states",
    ),
    (
        "ADVISOR",
        "Read command help, project guide, and developer notes",
    ),
];
const GHOSTHUNTER_MENU_ENTRIES: [(&str, &str); 3] = [
    (
        "RUN ALL UPDATES",
        "Start the planned updates without an extra confirmation prompt",
    ),
    ("RUN WITH CONFIRMATION", "Ask before the update run begins"),
    (
        "PREVIEW UPDATE",
        "Review planned package actions without changing systems",
    ),
];
const START_TUI_MODES: [&str; 6] = [
    "--run-ghosthunter",
    "--run-doctor",
    "--run-librarian",
    "--run-archeologist",
    "--run-restorer",
    "--run-advisor",
];
const LIBRARIAN_MENU_ENTRIES: [(&str, &str); 8] = [
    (
        "BROWSE THE CATALOGUE",
        "Consult saved package records by system and date",
    ),
    (
        "BROWSE ONE SYSTEM",
        "Filter snapshot records by host or container ID",
    ),
    (
        "ENTER NEW VOLUMES",
        "Preserve package versions and archives for all systems",
    ),
    (
        "ENTER ONE NEW VOLUME",
        "Create a snapshot for one selected system ID",
    ),
    (
        "REFRESH INVENTORY",
        "Rediscover supported hosts and containers",
    ),
    (
        "VIEW INVENTORY",
        "Browse the latest cached system inventory",
    ),
    ("UPDATE HISTORY", "Review previous maintenance operations"),
    ("LEAVE THE READING ROOM", "Return to the command line"),
];
const LIBRARIAN_MENU_COMMANDS: [&[&str]; 8] = [
    &[],
    &[],
    &[],
    &[],
    &["refresh"],
    &["inventory", "--json"],
    &["history", "--json"],
    &[],
];
const MENU_KEYS: &str = "↑/↓ J/K MOVE  •  1-N SELECT  •  HOME/END  •  ENTER OPEN  •  ESC/Q LEAVE";
const TEXT_KEYS: &str = "↑/↓ J/K READ  •  PGUP/PGDN PAGE  •  HOME/END  •  ESC BACK  •  Q EXIT";
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
    TargetInput,
    SnapshotList,
    SnapshotDetail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestorerView {
    Menu,
    SnapshotList,
    SnapshotDetail,
    ConfirmRestore,
    Result,
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
    command_output: bool,
    operation_lines: &[String],
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

    let lines = if show_results && command_output {
        operation_lines
            .iter()
            .map(|line| Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT))))
            .collect()
    } else if show_results {
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
            ("REFRESH STATUS", "Update cached package status"),
            ("VIEW STATUS", "Browse the latest cached update status"),
            ("VIEW DETAILED STATUS", "Include pending package names"),
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
    if !show_results {
        keep_row_visible(scroll_offset, *max_scroll, *page_size, selection * 3);
    } else if follow_output {
        *scroll_offset = *max_scroll;
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        report_area,
    );
    let footer = if show_results { TEXT_KEYS } else { MENU_KEYS };
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
                    Span::styled(
                        format_log_message(&record.message),
                        Style::default().fg(PARCHMENT),
                    ),
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
    if view == ArcheologistView::Menu {
        keep_row_visible(scroll_offset, *max_scroll, *page_size, selection * 3);
    } else {
        *scroll_offset = (*scroll_offset).min(*max_scroll);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        inner,
    );
    let footer = match view {
        ArcheologistView::Results => TEXT_KEYS,

        ArcheologistView::SearchInput => "TYPE QUERY  •  ENTER SEARCH  •  ESC BACK  •  CTRL+C QUIT",
        ArcheologistView::Menu => MENU_KEYS,
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(footer, Style::default().fg(PETROL))),
        ]),
        chunks[2],
    );
}

fn render_start_menu(
    frame: &mut ratatui::Frame<'_>,
    selection: usize,
    notice: &str,
    scroll_offset: &mut u16,
    max_scroll: &mut u16,
    page_size: &mut u16,
) {
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
    let inner = menu.inner(chunks[1]);
    *max_scroll = wrapped_line_count(&lines, inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    *page_size = inner.height.max(1);
    keep_row_visible(scroll_offset, *max_scroll, *page_size, selection * 3);
    frame.render_widget(
        Paragraph::new(lines)
            .block(menu)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(MENU_KEYS, Style::default().fg(PETROL))),
        ]),
        chunks[2],
    );
}

fn render_ghosthunter(
    frame: &mut ratatui::Frame<'_>,
    view: GhosthunterView,
    selection: usize,
    preview_lines: &[String],
    notice: &str,
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
    let header = Block::default()
        .title(" GHOSTHUNTER  •  UPDATE CONTROL ")
        .title_style(
            Style::default()
                .fg(PETROL)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PETROL));
    frame.render_widget(header, chunks[0]);
    let (title, lines, footer): (&str, Vec<Line<'static>>, &str) = match view {
        GhosthunterView::Menu => (
            " SELECT AN UPDATE ACTION ",
            GHOSTHUNTER_MENU_ENTRIES
                .iter()
                .enumerate()
                .flat_map(|(index, (label, description))| {
                    let selected = index == selection;
                    vec![
                        Line::from(Span::styled(
                            format!(
                                " {} [{}] {label}",
                                if selected { ">" } else { " " },
                                index + 1
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
            MENU_KEYS,
        ),
        GhosthunterView::Preview => (
            " PLANNED PACKAGE ACTIONS ",
            preview_lines
                .iter()
                .map(|line| Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT))))
                .collect(),
            TEXT_KEYS,
        ),
        GhosthunterView::Report => (
            " UPDATE RESULTS ",
            preview_lines
                .iter()
                .map(|line| Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT))))
                .collect(),
            TEXT_KEYS,
        ),
        GhosthunterView::Dashboard => unreachable!(),
    };
    let panel = Block::default()
        .title(title)
        .title_style(Style::default().fg(GOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PETROL));
    let inner = panel.inner(chunks[1]);
    frame.render_widget(panel, chunks[1]);
    let lines = if lines.is_empty() {
        vec![Line::from("No planned update actions.")]
    } else {
        lines
    };
    *max_scroll = wrapped_line_count(&lines, inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    *page_size = inner.height.saturating_sub(1).max(1);
    if view == GhosthunterView::Menu {
        keep_row_visible(scroll_offset, *max_scroll, *page_size, selection * 3);
    } else {
        *scroll_offset = (*scroll_offset).min(*max_scroll);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        inner,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(footer, Style::default().fg(PETROL))),
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
        keep_row_visible(scroll_offset, *max_scroll, *page_size, selection * 3);
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
        MENU_KEYS
    } else {
        TEXT_KEYS
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
    target_id: &str,
    target_snapshot: bool,
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
                let lines = LIBRARIAN_MENU_ENTRIES
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
                (lines, true)
            }
            LibrarianView::TargetInput => (
                vec![
                    Line::from(Span::styled(
                        if target_snapshot {
                            "  ENTER TARGET ID FOR A NEW SNAPSHOT"
                        } else {
                            "  ENTER TARGET ID TO FILTER SNAPSHOTS"
                        },
                        Style::default()
                            .fg(PETROL)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("  Target ID: {target_id}_"),
                        Style::default().fg(PARCHMENT),
                    )),
                ],
                false,
            ),
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
                    lines.push(Line::from(""));
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
        } else if view == LibrarianView::Menu {
            let selected_row = menu_selection * 3;
            if selected_row < usize::from(*scroll_offset) {
                *scroll_offset = selected_row as u16;
            } else if selected_row >= usize::from(*scroll_offset) + usize::from(*page_size) {
                *scroll_offset = selected_row.saturating_sub(usize::from(*page_size) - 1) as u16;
            }
        } else if view == LibrarianView::SnapshotList && !snapshots.is_empty() {
            let selected_row = 2 + snapshot_selection * 3;
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
            LibrarianView::Menu => MENU_KEYS,
            LibrarianView::TargetInput => {
                "TYPE TARGET ID  •  ENTER CONTINUE  •  ESC BACK  •  CTRL+C QUIT"
            }
            LibrarianView::SnapshotList => {
                "↑/↓ J/K • 1-9 • HOME/END • ENTER • S SNAPSHOT • ESC BACK • Q EXIT"
            }
            LibrarianView::SnapshotDetail => TEXT_KEYS,
        }
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                if view == LibrarianView::Menu {
                    LIBRARIAN_MENU_ENTRIES
                        .get(menu_selection)
                        .map(|entry| entry.1)
                        .unwrap_or(notice)
                } else {
                    notice
                },
                Style::default().fg(GOLD),
            )),
            Line::from(Span::styled(footer, Style::default().fg(PETROL))),
        ]),
        chunks[2],
    );
}

fn render_restorer(
    frame: &mut ratatui::Frame<'_>,
    view: RestorerView,
    selection: usize,
    snapshots: &[LibrarianSnapshot],
    detail_lines: &[String],
    result_lines: &[String],
    notice: &str,
    scroll_offset: &mut u16,
    max_scroll: &mut u16,
    page_size: &mut u16,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(frame.area());
    let title = Block::default()
        .title(" RESTORER  •  THE RECOVERY CHAMBER ")
        .title_style(
            Style::default()
                .fg(PETROL)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PETROL));
    let title_inner = title.inner(chunks[0]);
    frame.render_widget(title, chunks[0]);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Restore exact package versions from a Librarian snapshot.",
                Style::default().fg(PARCHMENT),
            )),
            Line::from(Span::styled(
                "A final confirmation is required before any package changes.",
                Style::default().fg(GOLD),
            )),
        ]),
        title_inner,
    );

    let (panel_title, lines, footer) = match view {
        RestorerView::Menu => (
            " RECOVERY OPTIONS ",
            [
                (
                    "BROWSE SNAPSHOTS",
                    "Inspect saved package states and choose one to restore.",
                ),
                ("LEAVE THE RECOVERY CHAMBER", "Return to the command line."),
            ]
            .iter()
            .enumerate()
            .flat_map(|(index, (label, description))| {
                let selected = index == selection;
                vec![
                    Line::from(Span::styled(
                        format!(
                            " {} [{}] {label}",
                            if selected { ">" } else { " " },
                            index + 1
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
            MENU_KEYS,
        ),
        RestorerView::SnapshotList => {
            let mut lines = vec![
                Line::from(Span::styled(
                    "  SELECT A SNAPSHOT TO INSPECT",
                    Style::default()
                        .fg(PETROL)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )),
                Line::from(""),
            ];
            if snapshots.is_empty() {
                lines.push(Line::from(
                    "  NO SNAPSHOTS FOUND. CREATE ONE WITH THE LIBRARIAN.",
                ));
            }
            for (index, snapshot) in snapshots.iter().enumerate() {
                let selected = index == selection;
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} {}  {}  {} packages  {}",
                        if selected { ">" } else { " " },
                        snapshot.target.name,
                        snapshot.created,
                        snapshot.package_count,
                        if snapshot.restorable {
                            "RESTORABLE"
                        } else {
                            "MANIFEST ONLY"
                        }
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
                lines.push(Line::from(""));
            }
            (
                " SAVED SNAPSHOTS ",
                lines,
                "↑/↓ J/K • 1-9 • HOME/END • ENTER • R RESTORE • ESC BACK • Q EXIT",
            )
        }
        RestorerView::SnapshotDetail => (
            " SNAPSHOT RECORD ",
            detail_lines
                .iter()
                .map(|line| Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT))))
                .collect(),
            "R RESTORE  •  ↑/↓ J/K READ  •  PGUP/PGDN  •  ESC BACK  •  Q EXIT",
        ),
        RestorerView::ConfirmRestore => {
            let snapshot = snapshots.get(selection);
            let mut lines = vec![
                Line::from(Span::styled(
                    "  CONFIRM PACKAGE RESTORATION",
                    Style::default()
                        .fg(ERROR_RED)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )),
                Line::from(""),
            ];
            if let Some(snapshot) = snapshot {
                lines.extend([
                    Line::from(format!("  Target: {}", snapshot.target.name)),
                    Line::from(format!("  Created: {}", snapshot.created)),
                    Line::from(format!(
                        "  Packages in snapshot: {}",
                        snapshot.package_count
                    )),
                    Line::from(format!("  Snapshot ID: {}", snapshot.id)),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Pacmonster will restore cached versions; newer extra packages remain.",
                        Style::default().fg(GOLD),
                    )),
                ]);
            }
            (
                " FINAL CHECK ",
                lines,
                "Y CONFIRM RESTORE  •  N/ESC CANCEL  •  Q EXIT",
            )
        }
        RestorerView::Result => (
            " RESTORATION REPORT ",
            result_lines
                .iter()
                .map(|line| Line::from(Span::styled(line.clone(), Style::default().fg(PARCHMENT))))
                .collect(),
            TEXT_KEYS,
        ),
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
    if view == RestorerView::SnapshotList && !snapshots.is_empty() {
        let selected_row = 2 + selection * 3;
        if selected_row < usize::from(*scroll_offset) {
            *scroll_offset = selected_row as u16;
        } else if selected_row >= usize::from(*scroll_offset) + usize::from(*page_size) {
            *scroll_offset = selected_row.saturating_sub(usize::from(*page_size) - 1) as u16;
        }
    } else if view == RestorerView::Menu {
        keep_row_visible(scroll_offset, *max_scroll, *page_size, selection * 3);
    } else if view == RestorerView::ConfirmRestore {
        *scroll_offset = 0;
    }
    *scroll_offset = (*scroll_offset).min(*max_scroll);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((*scroll_offset, 0)),
        inner,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(notice, Style::default().fg(GOLD))),
            Line::from(Span::styled(footer, Style::default().fg(PETROL))),
        ]),
        chunks[2],
    );
}

fn format_command_output(stdout: &[String], stderr: &[String]) -> Vec<String> {
    let stdout_text = stdout.join("\n");
    let mut lines = match serde_json::from_str::<serde_json::Value>(&stdout_text) {
        Ok(value) => format_json_value(&value),
        Err(error) if matches!(stdout_text.trim_start().chars().next(), Some('{' | '[')) => {
            vec![
                "Structured command output could not be interpreted.".to_string(),
                format!("Details: {error}"),
                "Raw structured data was hidden.".to_string(),
            ]
        }
        Err(_) => stdout.to_vec(),
    };
    if !stderr.is_empty() {
        let stderr_text = stderr.join("\n");
        match serde_json::from_str::<serde_json::Value>(&stderr_text) {
            Ok(value) => {
                lines.push("Additional diagnostic details:".to_string());
                lines.extend(format_json_value(&value));
            }
            Err(error) if matches!(stderr_text.trim_start().chars().next(), Some('{' | '[')) => {
                lines.push("Structured diagnostic details could not be interpreted.".to_string());
                lines.push(format!("Details: {error}"));
                lines.push("Raw structured data was hidden.".to_string());
            }
            Err(_) => lines.extend(stderr.iter().map(|line| format!("stderr: {line}"))),
        }
    }
    lines
}

fn humanize_json_key(key: &str) -> String {
    key.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_json_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "Not available".to_string(),
        serde_json::Value::Bool(true) => "Yes".to_string(),
        serde_json::Value::Bool(false) => "No".to_string(),
        serde_json::Value::String(value)
            if matches!(value.trim_start().chars().next(), Some('{' | '[')) =>
        {
            match serde_json::from_str::<serde_json::Value>(value) {
                Ok(nested) => format_json_value(&nested).join(" · "),
                Err(_) => value.clone(),
            }
        }
        serde_json::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn command_emits_structured_output(command: Option<&str>) -> bool {
    matches!(
        command,
        Some(
            "ghosthunter-preview"
                | "updates"
                | "updates-confirm"
                | "doctor-diagnostics"
                | "doctor-installation"
                | "doctor-utility"
                | "archeologist-all"
                | "archeologist-issues"
                | "archeologist-search"
                | "librarian-list"
                | "librarian-snapshot"
                | "librarian-show"
                | "librarian-utility"
                | "restorer-list"
                | "restorer-show"
                | "restorer-restore"
        )
    )
}

fn append_json_value(lines: &mut Vec<String>, value: &serde_json::Value, indent: usize) {
    let padding = " ".repeat(indent);
    match value {
        serde_json::Value::Object(fields) => {
            for (key, field) in fields {
                let label = humanize_json_key(key);
                if let serde_json::Value::Object(items) = field {
                    if key == "packages" || key == "archives" {
                        lines.push(format!(
                            "{padding}{} ({}):",
                            if key == "packages" {
                                "Packages"
                            } else {
                                "Package archives"
                            },
                            items.len()
                        ));
                        for (name, item) in items {
                            match item {
                                serde_json::Value::Object(_) => {
                                    lines.push(format!("{padding}  {name}"));
                                    append_json_value(lines, item, indent + 4);
                                }
                                _ => lines.push(format!(
                                    "{padding}  {name} — {}",
                                    format_json_scalar(item)
                                )),
                            }
                        }
                        continue;
                    }
                }
                match field {
                    serde_json::Value::Object(nested) if nested.is_empty() => {
                        lines.push(format!("{padding}{label}: None"));
                    }
                    serde_json::Value::Object(_) => {
                        lines.push(format!("{padding}{label}"));
                        append_json_value(lines, field, indent + 2);
                    }
                    serde_json::Value::Array(items) if items.is_empty() => {
                        lines.push(format!("{padding}{label}: None"));
                    }
                    serde_json::Value::Array(items) => {
                        lines.push(format!("{padding}{label} ({}):", items.len()));
                        append_json_value(lines, field, indent + 2);
                    }
                    serde_json::Value::String(text)
                        if matches!(text.trim_start().chars().next(), Some('{' | '[')) =>
                    {
                        match serde_json::from_str::<serde_json::Value>(text) {
                            Ok(nested) => {
                                lines.push(format!("{padding}{label}:"));
                                append_json_value(lines, &nested, indent + 2);
                            }
                            Err(_) => lines
                                .push(format!("{padding}{label}: {}", format_json_scalar(field))),
                        }
                    }
                    serde_json::Value::String(text) if text.contains('\n') => {
                        lines.push(format!("{padding}{label}:"));
                        lines.extend(text.lines().map(|line| format!("{padding}  {line}")));
                    }
                    _ => lines.push(format!("{padding}{label}: {}", format_json_scalar(field))),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    lines.push(String::new());
                }
                if item.is_object() {
                    lines.push(format!("{padding}Record {}", index + 1));
                    append_json_value(lines, item, indent + 2);
                } else {
                    lines.push(format!("{padding}• {}", format_json_scalar(item)));
                }
            }
        }
        _ => lines.push(format!("{padding}{}", format_json_scalar(value))),
    }
}

fn format_json_value(value: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::new();
    append_json_value(&mut lines, value, 0);
    if lines.is_empty() {
        lines.push("No records found.".to_string());
    }
    lines
}

fn format_update_report(stdout: &[String]) -> Result<Vec<String>, String> {
    let report: UpdateReport = serde_json::from_str(&stdout.join("\n"))
        .map_err(|error| format!("Could not read update report: {error}"))?;
    if report.status.as_deref() == Some("error") {
        return Ok(vec![
            "UPDATE DID NOT COMPLETE".to_string(),
            report
                .message
                .or(report.error)
                .unwrap_or_else(|| "The update command returned an error.".to_string()),
        ]);
    }
    let mut lines = vec![
        format!(
            "Systems: {} attempted, {} succeeded, {} failed",
            report.summary.systems_attempted,
            report.summary.systems_succeeded,
            report.summary.systems_failed
        ),
        format!("Packages changed: {}", report.summary.packages_changed),
        String::new(),
        "UPDATED SYSTEMS".to_string(),
    ];

    if report.updated_systems.is_empty() {
        lines.push("No package version changes detected.".to_string());
    } else {
        for system in &report.updated_systems {
            lines.push(format!("- {} (ID {})", system.name, system.id));
            for package in &system.packages {
                let previous = package
                    .previous_version
                    .as_deref()
                    .unwrap_or("not installed");
                let current = package.current_version.as_deref().unwrap_or("removed");
                lines.push(format!(
                    "  • {}: {} -> {} ({})",
                    package.name, previous, current, package.change
                ));
            }
        }
    }

    if !report.failed_systems.is_empty() {
        lines.push(String::new());
        lines.push("FAILED SYSTEMS".to_string());
        for system in &report.failed_systems {
            lines.push(format!("- {} (ID {})", system.name, system.id));
            for package in &system.packages {
                let previous = package
                    .previous_version
                    .as_deref()
                    .unwrap_or("not installed");
                let current = package.current_version.as_deref().unwrap_or("removed");
                lines.push(format!(
                    "  • {}: {} -> {} ({})",
                    package.name, previous, current, package.change
                ));
            }
        }
    }

    for system in &report.systems {
        if let Some(error) = &system.package_report_error {
            lines.push(format!(
                "Could not inspect packages on {}: {}",
                system.name, error
            ));
        }
    }
    Ok(lines)
}

fn format_log_message(message: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(message) else {
        return message.to_string();
    };
    let mut lines = Vec::new();
    append_json_value(&mut lines, &value, 0);
    lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .map(|line| line.trim().to_string())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn snapshot_is_restorable(snapshot: &LibrarianSnapshot) -> bool {
    snapshot.restorable
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
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown main-menu selection",
        ))
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

fn keep_row_visible(scroll_offset: &mut u16, maximum: u16, page_size: u16, selected_row: usize) {
    let start = usize::from(*scroll_offset);
    let page = usize::from(page_size.max(1));
    if selected_row < start {
        *scroll_offset = selected_row as u16;
    } else if selected_row >= start + page {
        *scroll_offset = selected_row.saturating_sub(page - 1) as u16;
    }
    *scroll_offset = (*scroll_offset).min(maximum);
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
    } else if std::env::args().any(|argument| argument == "--run-updates-confirm") {
        Some("updates-confirm")
    } else if std::env::args().any(|argument| argument == "--run-ghosthunter-confirm") {
        Some("ghosthunter-confirm")
    } else if std::env::args().any(|argument| argument == "--run-ghosthunter") {
        Some("ghosthunter")
    } else if std::env::args().any(|argument| argument == "--run-librarian") {
        Some("librarian")
    } else if std::env::args().any(|argument| argument == "--run-archeologist") {
        Some("archeologist")
    } else if std::env::args().any(|argument| argument == "--run-start") {
        Some("start")
    } else if std::env::args().any(|argument| argument == "--run-advisor") {
        Some("advisor")
    } else if std::env::args().any(|argument| argument == "--run-restorer") {
        Some("restorer")
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
    let mut doctor_command_output = false;
    let mut doctor_operation_lines: Vec<String> = Vec::new();
    let mut doctor_menu_selection = 0usize;
    let mut doctor_notice = "THE INFIRMARY IS READY.".to_string();
    let mut archeologist_view = ArcheologistView::Menu;
    let mut archeologist_selection = 0usize;
    let mut archeologist_records: Vec<ArcheologistRecord> = Vec::new();
    let mut archeologist_query = String::new();
    let mut archeologist_notice = "THE RECORDS ROOM IS READY.".to_string();
    let mut start_menu_selection = 0usize;
    let mut start_menu_notice = "WELCOME TO PACMONSTER.".to_string();
    let mut ghosthunter_view = GhosthunterView::Menu;
    let mut ghosthunter_selection = 0usize;
    let mut ghosthunter_preview_lines = Vec::new();
    let mut ghosthunter_report_lines = Vec::new();
    let mut ghosthunter_notice = "CHOOSE AN UPDATE ACTION.".to_string();
    let mut advisor_view = AdvisorView::Menu;
    let mut advisor_selection = 0usize;
    let mut advisor_help_lines: Vec<String> = Vec::new();
    let mut advisor_readme_lines: Vec<ReadmeLine> = Vec::new();
    let mut advisor_notice = "THE REFERENCE ROOM IS READY.".to_string();
    let mut librarian_view = LibrarianView::Menu;
    let mut librarian_menu_selection = 0usize;
    let mut librarian_target_id = String::new();
    let mut librarian_target_snapshot = false;
    let mut librarian_snapshot_selection = 0usize;
    let mut librarian_snapshots: Vec<LibrarianSnapshot> = Vec::new();
    let mut librarian_detail_lines: Vec<String> = Vec::new();
    let mut librarian_notice = "THE CATALOGUE IS READY.".to_string();
    let mut restorer_view = RestorerView::Menu;
    let mut restorer_selection = 0usize;
    let mut restorer_snapshots: Vec<LibrarianSnapshot> = Vec::new();
    let mut restorer_detail_lines: Vec<String> = Vec::new();
    let mut restorer_result_lines: Vec<String> = Vec::new();
    let mut restorer_notice = "THE RECOVERY CHAMBER IS READY.".to_string();
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

    if let Some(mode) =
        mode.filter(|mode| matches!(*mode, "updates" | "updates-confirm" | "ghosthunter-confirm"))
    {
        let (arguments, action): (&[&str], &str) = match mode {
            "updates" => (&["update", "--yes"], "updates"),
            "updates-confirm" => (&["update"], "updates-confirm"),
            "ghosthunter-confirm" => (&["update", "--json"], "updates-confirm"),
            _ => unreachable!(),
        };
        if mode == "ghosthunter-confirm" {
            ghosthunter_view = GhosthunterView::Dashboard;
        }
        match start_command(&output_sender, arguments, action) {
            Ok(command_child) => {
                let description = match mode {
                    "updates" => "Starting all planned updates (--yes)...",
                    "updates-confirm" | "ghosthunter-confirm" => {
                        "Starting updates with confirmation..."
                    }
                    _ => unreachable!(),
                };
                output.push_back(description.to_string());
                active_command = Some(action.to_string());
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
                if !(is_stdout && command_emits_structured_output(active_command.as_deref())) {
                    output.push_back(if is_stdout {
                        format_log_message(&line)
                    } else {
                        line
                    });
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
                    let mut detail = command_stderr.join(" ").trim().to_string();
                    if detail.is_empty() {
                        detail = command_stdout.join(" ").trim().to_string();
                    }
                    detail = format_log_message(&detail);
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
                if mode == Some("restorer") && exit_code != 0 {
                    restorer_result_lines = format_command_output(&command_stdout, &command_stderr);
                    restorer_view = RestorerView::Result;
                    let mut detail = command_stderr.join(" ").trim().to_string();
                    if detail.is_empty() {
                        detail = command_stdout.join(" ").trim().to_string();
                    }
                    detail = format_log_message(&detail);
                    if !detail.is_empty() {
                        restorer_notice = format!("RESTORATION ERROR: {detail}");
                    } else {
                        restorer_notice =
                            format!("RESTORATION COMMAND EXITED WITH STATUS {exit_code}.");
                    }
                }
                match active_command.as_deref() {
                    Some("ghosthunter-preview") => {
                        ghosthunter_preview_lines =
                            format_command_output(&command_stdout, &command_stderr);
                        if ghosthunter_preview_lines.is_empty() {
                            ghosthunter_preview_lines
                                .push("No update actions were returned.".into());
                        }
                        ghosthunter_view = GhosthunterView::Preview;
                        ghosthunter_notice = if exit_code == 0 {
                            "UPDATE PREVIEW READY. NO PACKAGES WERE CHANGED.".into()
                        } else {
                            format!("PREVIEW COMMAND EXITED WITH STATUS {exit_code}.")
                        };
                        scroll_offset = 0;
                        follow_output = false;
                    }
                    Some("updates" | "updates-confirm")
                        if matches!(mode, Some("ghosthunter" | "ghosthunter-confirm")) =>
                    {
                        match format_update_report(&command_stdout) {
                            Ok(lines) => {
                                ghosthunter_report_lines = lines;
                                ghosthunter_view = GhosthunterView::Report;
                                ghosthunter_notice = if exit_code == 0 {
                                    "UPDATE RUN COMPLETE.".to_string()
                                } else {
                                    format!("UPDATE RUN FINISHED WITH STATUS {exit_code}.")
                                };
                            }
                            Err(error) => {
                                ghosthunter_report_lines = vec![error.clone()];
                                ghosthunter_view = GhosthunterView::Report;
                                ghosthunter_notice = error;
                            }
                        }
                        scroll_offset = 0;
                        follow_output = false;
                    }
                    Some("advisor-help") => {
                        advisor_help_lines = command_stdout.clone();
                        advisor_view = AdvisorView::Help;
                        advisor_notice = if advisor_help_lines.is_empty() {
                            "CLI HELP RETURNED NO CONTENT.".to_string()
                        } else {
                            "CLI HELP LOADED.".to_string()
                        };
                        scroll_offset = 0;
                    }
                    Some("doctor-diagnostics" | "doctor-installation") => {
                        doctor_command_output = false;
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
                                doctor_command_output = true;
                                doctor_operation_lines =
                                    format_command_output(&command_stdout, &command_stderr);
                                doctor_notice =
                                    format!("DIAGNOSTIC RESULTS COULD NOT BE READ: {error}");
                                follow_output = false;
                            }
                        }
                    }
                    Some("doctor-utility") => {
                        doctor_operation_lines =
                            format_command_output(&command_stdout, &command_stderr);
                        doctor_command_output = true;
                        doctor_show_results = true;
                        doctor_notice = if exit_code == 0 {
                            "WITCHDOCTOR COMMAND COMPLETE.".to_string()
                        } else {
                            format!("COMMAND EXITED WITH STATUS {exit_code}.")
                        };
                        scroll_offset = 0;
                        follow_output = false;
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
                                librarian_detail_lines = format_json_value(&detail);
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
                    Some("librarian-utility") => {
                        librarian_detail_lines =
                            format_command_output(&command_stdout, &command_stderr);
                        librarian_view = LibrarianView::SnapshotDetail;
                        librarian_notice = if exit_code == 0 {
                            "LIBRARY RECORD UPDATED.".to_string()
                        } else {
                            format!("LIBRARIAN COMMAND EXITED WITH STATUS {exit_code}.")
                        };
                        scroll_offset = 0;
                        follow_output = false;
                    }
                    Some("restorer-list") if exit_code == 0 => {
                        match serde_json::from_str::<Vec<LibrarianSnapshot>>(
                            &command_stdout.join("\n"),
                        ) {
                            Ok(snapshots) => {
                                restorer_snapshots = snapshots;
                                restorer_selection = 0;
                                restorer_view = RestorerView::SnapshotList;
                                restorer_notice =
                                    format!("{} SNAPSHOT(S) AVAILABLE.", restorer_snapshots.len());
                                scroll_offset = 0;
                            }
                            Err(error) => {
                                restorer_result_lines =
                                    vec![format!("Snapshot catalogue could not be read: {error}")];
                                restorer_result_lines.extend(format_command_output(
                                    &command_stdout,
                                    &command_stderr,
                                ));
                                restorer_view = RestorerView::Result;
                            }
                        }
                    }
                    Some("restorer-show") if exit_code == 0 => {
                        restorer_detail_lines =
                            format_command_output(&command_stdout, &command_stderr);
                        restorer_view = RestorerView::SnapshotDetail;
                        restorer_notice = "REVIEW THE RECORD BEFORE RESTORING.".to_string();
                        scroll_offset = 0;
                    }
                    Some("restorer-restore") => {
                        let restore_data =
                            serde_json::from_str::<serde_json::Value>(&command_stdout.join("\n"))
                                .ok();
                        let restore_status = restore_data
                            .as_ref()
                            .and_then(|result| result.get("status"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned);
                        let output =
                            restore_data
                                .as_ref()
                                .map(format_json_value)
                                .unwrap_or_else(|| {
                                    format_command_output(&command_stdout, &command_stderr)
                                });
                        restorer_result_lines = output;
                        restorer_view = RestorerView::Result;
                        restorer_notice = match restore_status.as_deref() {
                            Some("success") => "RESTORATION COMPLETE.".to_string(),
                            Some("unchanged") => "TARGET ALREADY MATCHES THE SNAPSHOT.".to_string(),
                            Some("missing") => "REQUIRED PACKAGE ARCHIVES ARE MISSING.".to_string(),
                            Some("locked") => "ANOTHER UPDATE OR RESTORE IS ACTIVE.".to_string(),
                            Some("failed") => "PACKAGE RESTORATION FAILED.".to_string(),
                            _ if exit_code == 0 => "RESTORE COMMAND FINISHED.".to_string(),
                            _ => restorer_notice.clone(),
                        };
                        scroll_offset = 0;
                    }
                    _ => {}
                }
                if !matches!(mode, Some("ghosthunter" | "ghosthunter-confirm"))
                    && (mode == Some("updates")
                        || mode == Some("updates-confirm")
                        || matches!(
                            active_command.as_deref(),
                            Some("updates" | "updates-confirm")
                        ))
                {
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
                    render_start_menu(
                        frame,
                        start_menu_selection,
                        &start_menu_notice,
                        &mut scroll_offset,
                        &mut max_scroll,
                        &mut page_size,
                    );
                    return;
                }
                if matches!(mode, Some("ghosthunter" | "ghosthunter-confirm"))
                    && ghosthunter_view != GhosthunterView::Dashboard
                    && !completion_prompt
                {
                    let content = if ghosthunter_view == GhosthunterView::Report {
                        &ghosthunter_report_lines
                    } else {
                        &ghosthunter_preview_lines
                    };
                    render_ghosthunter(
                        frame,
                        ghosthunter_view,
                        ghosthunter_selection,
                        content,
                        &ghosthunter_notice,
                        &mut scroll_offset,
                        &mut max_scroll,
                        &mut page_size,
                    );
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
                if mode == Some("restorer") {
                    render_restorer(
                        frame,
                        restorer_view,
                        restorer_selection,
                        &restorer_snapshots,
                        &restorer_detail_lines,
                        &restorer_result_lines,
                        &restorer_notice,
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
                        &librarian_target_id,
                        librarian_target_snapshot,
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
                        doctor_command_output,
                        &doctor_operation_lines,
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
                    .title(
                        if matches!(mode, Some("ghosthunter" | "ghosthunter-confirm")) {
                            " GHOSTHUNTER  •  THE GHOST POOL "
                        } else {
                            " THE GHOST POOL "
                        },
                    )
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
                            KeyCode::Home => start_menu_selection = 0,
                            KeyCode::End => start_menu_selection = START_MENU_ENTRIES.len() - 1,
                            KeyCode::Char(digit @ '1'..='6') => {
                                start_menu_selection =
                                    digit.to_digit(10).unwrap_or(1).saturating_sub(1) as usize;
                            }
                            KeyCode::Enter => {
                                let selection = start_menu_selection;
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

                    if matches!(mode, Some("ghosthunter" | "ghosthunter-confirm"))
                        && ghosthunter_view != GhosthunterView::Dashboard
                        && child.is_none()
                        && !completion_prompt
                    {
                        match ghosthunter_view {
                            GhosthunterView::Menu => {
                                let mut command_to_start: Option<(String, Vec<String>, String)> =
                                    None;
                                match key.code {
                                    KeyCode::Up | KeyCode::Char('k') => {
                                        ghosthunter_selection =
                                            ghosthunter_selection.saturating_sub(1);
                                    }
                                    KeyCode::Down | KeyCode::Char('j') => {
                                        ghosthunter_selection = (ghosthunter_selection + 1).min(2);
                                    }
                                    KeyCode::Home => ghosthunter_selection = 0,
                                    KeyCode::End => ghosthunter_selection = 2,
                                    KeyCode::Char('1') => ghosthunter_selection = 0,
                                    KeyCode::Char('2') => ghosthunter_selection = 1,
                                    KeyCode::Char('3') => ghosthunter_selection = 2,
                                    KeyCode::Enter => match ghosthunter_selection {
                                        0 => {
                                            ghosthunter_view = GhosthunterView::Dashboard;
                                            command_to_start = Some((
                                                "updates".into(),
                                                vec![
                                                    "update".into(),
                                                    "--yes".into(),
                                                    "--json".into(),
                                                ],
                                                "RUNNING PLANNED UPDATES...".into(),
                                            ));
                                        }
                                        1 => {
                                            ghosthunter_view = GhosthunterView::Dashboard;
                                            command_to_start = Some((
                                                "updates-confirm".into(),
                                                vec!["update".into(), "--json".into()],
                                                "STARTING UPDATES WITH CONFIRMATION...".into(),
                                            ));
                                        }
                                        2 => {
                                            command_to_start = Some((
                                                "ghosthunter-preview".into(),
                                                vec!["dry-run".into(), "--json".into()],
                                                "PREPARING UPDATE PREVIEW...".into(),
                                            ));
                                        }
                                        _ => unreachable!(),
                                    },
                                    KeyCode::Char('q') | KeyCode::Esc => break,
                                    _ => {}
                                }
                                if let Some((action, arguments, notice)) = command_to_start {
                                    command_stdout.clear();
                                    command_stderr.clear();
                                    output.clear();
                                    ghosthunter_notice = notice;
                                    active_command = Some(action);
                                    child_finished = false;
                                    stdout_finished = false;
                                    let borrowed_arguments: Vec<&str> =
                                        arguments.iter().map(String::as_str).collect();
                                    match launch_librarian_command(
                                        &mut terminal,
                                        &output_sender,
                                        &borrowed_arguments,
                                        "ghosthunter",
                                    ) {
                                        Ok(command_child) => child = Some(command_child),
                                        Err(error) => {
                                            active_command = None;
                                            ghosthunter_view = GhosthunterView::Menu;
                                            ghosthunter_notice =
                                                format!("UPDATE ACTION COULD NOT START: {error}");
                                        }
                                    }
                                }
                                continue;
                            }
                            GhosthunterView::Preview | GhosthunterView::Report => match key.code {
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
                                KeyCode::Esc | KeyCode::Backspace => {
                                    ghosthunter_view = GhosthunterView::Menu;
                                    ghosthunter_notice = "CHOOSE AN UPDATE ACTION.".into();
                                }
                                KeyCode::Char('q') => break,
                                _ => {}
                            },
                            GhosthunterView::Dashboard => {}
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
                                KeyCode::Home => advisor_selection = 0,
                                KeyCode::End => advisor_selection = 4,
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
                                        command_to_start =
                                            Some(("advisor-help".to_string(), vec!["help".into()]));
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

                    if mode == Some("restorer") && child.is_none() {
                        let mut command_to_start: Option<(String, Vec<String>)> = None;
                        match restorer_view {
                            RestorerView::Menu => match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    restorer_selection = restorer_selection.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    restorer_selection = (restorer_selection + 1).min(1);
                                }
                                KeyCode::Home => restorer_selection = 0,
                                KeyCode::End => restorer_selection = 1,
                                KeyCode::Char('1') => restorer_selection = 0,
                                KeyCode::Char('2') => restorer_selection = 1,
                                KeyCode::Enter if restorer_selection == 0 => {
                                    command_to_start = Some((
                                        "restorer-list".to_string(),
                                        vec!["restorer".into(), "list".into(), "--json".into()],
                                    ));
                                    restorer_notice = "OPENING THE SNAPSHOT CATALOGUE...".into();
                                }
                                KeyCode::Enter | KeyCode::Char('q') | KeyCode::Esc => break,
                                _ => {}
                            },
                            RestorerView::SnapshotList => match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    restorer_selection = restorer_selection.saturating_sub(1);
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if !restorer_snapshots.is_empty() {
                                        restorer_selection = (restorer_selection + 1)
                                            .min(restorer_snapshots.len() - 1);
                                    }
                                }
                                KeyCode::Home => restorer_selection = 0,
                                KeyCode::End if !restorer_snapshots.is_empty() => {
                                    restorer_selection = restorer_snapshots.len() - 1;
                                }
                                KeyCode::Char(digit @ '1'..='9')
                                    if (digit.to_digit(10).unwrap_or(1) as usize)
                                        <= restorer_snapshots.len() =>
                                {
                                    restorer_selection =
                                        digit.to_digit(10).unwrap_or(1) as usize - 1;
                                }
                                KeyCode::Enter if !restorer_snapshots.is_empty() => {
                                    let snapshot = &restorer_snapshots[restorer_selection];
                                    command_to_start = Some((
                                        "restorer-show".to_string(),
                                        vec![
                                            "librarian".into(),
                                            "show".into(),
                                            snapshot.id.clone(),
                                            "--json".into(),
                                        ],
                                    ));
                                    restorer_notice = "READING SNAPSHOT DETAILS...".into();
                                }
                                KeyCode::Char('r') if !restorer_snapshots.is_empty() => {
                                    if snapshot_is_restorable(
                                        &restorer_snapshots[restorer_selection],
                                    ) {
                                        restorer_view = RestorerView::ConfirmRestore;
                                        restorer_notice =
                                            "RESTORE USES CACHED PACKAGE ARCHIVES.".into();
                                        scroll_offset = 0;
                                    } else {
                                        restorer_notice =
                                            "THIS MANIFEST HAS NO SAVED PACKAGE ARCHIVES.".into();
                                    }
                                }
                                KeyCode::Esc | KeyCode::Backspace => {
                                    restorer_view = RestorerView::Menu;
                                    restorer_selection = 0;
                                    scroll_offset = 0;
                                }
                                KeyCode::Char('q') => break,
                                _ => {}
                            },
                            RestorerView::SnapshotDetail => match key.code {
                                KeyCode::Char('r') => {
                                    if restorer_snapshots
                                        .get(restorer_selection)
                                        .is_some_and(snapshot_is_restorable)
                                    {
                                        restorer_view = RestorerView::ConfirmRestore;
                                        restorer_notice =
                                            "RESTORE USES CACHED PACKAGE ARCHIVES.".into();
                                        scroll_offset = 0;
                                    } else {
                                        restorer_notice =
                                            "THIS MANIFEST HAS NO SAVED PACKAGE ARCHIVES.".into();
                                    }
                                }
                                KeyCode::Esc | KeyCode::Backspace => {
                                    restorer_view = RestorerView::SnapshotList;
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
                            RestorerView::ConfirmRestore => match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    if let Some(snapshot) =
                                        restorer_snapshots.get(restorer_selection)
                                    {
                                        command_to_start = Some((
                                            "restorer-restore".to_string(),
                                            vec![
                                                "restorer".into(),
                                                "restore".into(),
                                                snapshot.id.clone(),
                                                "--yes".into(),
                                                "--json".into(),
                                            ],
                                        ));
                                        restorer_notice =
                                            "RESTORING THE SELECTED PACKAGE VERSIONS...".into();
                                    } else {
                                        restorer_view = RestorerView::SnapshotList;
                                        restorer_notice = "NO SNAPSHOT IS SELECTED.".into();
                                    }
                                }
                                KeyCode::Char('n')
                                | KeyCode::Char('N')
                                | KeyCode::Esc
                                | KeyCode::Backspace => {
                                    restorer_view = RestorerView::SnapshotDetail;
                                    restorer_notice = "RESTORE CANCELLED.".into();
                                    scroll_offset = 0;
                                }
                                KeyCode::Char('q') => break,
                                _ => {}
                            },
                            RestorerView::Result => match key.code {
                                KeyCode::Esc | KeyCode::Backspace => {
                                    restorer_view = RestorerView::SnapshotList;
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
                                "restorer",
                            ) {
                                Ok(command_child) => child = Some(command_child),
                                Err(error) => {
                                    active_command = None;
                                    restorer_view = RestorerView::Result;
                                    restorer_result_lines =
                                        vec![format!("Could not start operation: {error}")];
                                    restorer_notice = "RESTORATION COULD NOT START.".into();
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
                                        doctor_command_output = false;
                                        doctor_checks.clear();
                                        doctor_operation_lines.clear();
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
                                        doctor_menu_selection = (doctor_menu_selection + 1).min(5);
                                    }
                                    KeyCode::Home => doctor_menu_selection = 0,
                                    KeyCode::End => doctor_menu_selection = 5,
                                    KeyCode::Char('1') => doctor_menu_selection = 0,
                                    KeyCode::Char('2') => doctor_menu_selection = 1,
                                    KeyCode::Char('3') => doctor_menu_selection = 2,
                                    KeyCode::Char('4') => doctor_menu_selection = 3,
                                    KeyCode::Char('5') => doctor_menu_selection = 4,
                                    KeyCode::Char('6') => doctor_menu_selection = 5,
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
                                        2..=4 => {
                                            let (label, arguments) = match doctor_menu_selection {
                                                2 => {
                                                    ("REFRESHING STATUS...", vec!["refresh-status"])
                                                }
                                                3 => (
                                                    "READING CACHED STATUS...",
                                                    vec!["status", "--json"],
                                                ),
                                                _ => (
                                                    "READING DETAILED STATUS...",
                                                    vec!["status", "--verbose"],
                                                ),
                                            };
                                            command_to_start = Some((
                                                "doctor-utility".to_string(),
                                                arguments
                                                    .iter()
                                                    .map(|arg| (*arg).to_string())
                                                    .collect(),
                                                "witchdoctor".to_string(),
                                            ));
                                            doctor_notice = label.to_string();
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
                                    KeyCode::Home => archeologist_selection = 0,
                                    KeyCode::End => archeologist_selection = 3,
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
                                    KeyCode::Char('c')
                                        if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                    {
                                        break;
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
                                    librarian_menu_selection = (librarian_menu_selection + 1)
                                        .min(LIBRARIAN_MENU_ENTRIES.len() - 1);
                                }
                                KeyCode::Home => librarian_menu_selection = 0,
                                KeyCode::End => {
                                    librarian_menu_selection = LIBRARIAN_MENU_ENTRIES.len() - 1;
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
                                        librarian_target_snapshot = false;
                                        librarian_target_id.clear();
                                        librarian_view = LibrarianView::TargetInput;
                                        librarian_notice = "FILTER THE CATALOGUE BY TARGET.".into();
                                    }
                                    2 => {
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
                                    3 => {
                                        librarian_target_snapshot = true;
                                        librarian_target_id.clear();
                                        librarian_view = LibrarianView::TargetInput;
                                        librarian_notice =
                                            "CHOOSE A TARGET FOR THE NEW SNAPSHOT.".into();
                                    }
                                    4..=6 => {
                                        let arguments = LIBRARIAN_MENU_COMMANDS
                                            [librarian_menu_selection]
                                            .to_vec();
                                        command_to_start = Some((
                                            "librarian-utility".to_string(),
                                            arguments
                                                .iter()
                                                .map(|arg| (*arg).to_string())
                                                .collect(),
                                        ));
                                        librarian_notice = format!(
                                            "RUNNING {}...",
                                            LIBRARIAN_MENU_ENTRIES[librarian_menu_selection].0
                                        );
                                    }
                                    _ => break,
                                },
                                KeyCode::Char(digit @ '1'..='8') => {
                                    librarian_menu_selection =
                                        digit.to_digit(10).unwrap_or(1).saturating_sub(1) as usize;
                                }
                                KeyCode::Char('q') | KeyCode::Esc => break,
                                _ => {}
                            },
                            LibrarianView::TargetInput => match key.code {
                                KeyCode::Enter if !librarian_target_id.trim().is_empty() => {
                                    let (action, operation) = if librarian_target_snapshot {
                                        ("librarian-snapshot", "snapshot")
                                    } else {
                                        ("librarian-list", "list")
                                    };
                                    librarian_notice = if librarian_target_snapshot {
                                        "ENTERING A VOLUME FOR THE SELECTED TARGET...".into()
                                    } else {
                                        "OPENING THE SELECTED TARGET'S CATALOGUE...".into()
                                    };
                                    command_to_start = Some((
                                        action.to_string(),
                                        vec![
                                            "librarian".into(),
                                            operation.into(),
                                            librarian_target_id.trim().to_string(),
                                            "--json".into(),
                                        ],
                                    ));
                                }
                                KeyCode::Enter => {
                                    librarian_notice = "ENTER A TARGET ID FIRST.".into();
                                }
                                KeyCode::Esc => {
                                    librarian_view = LibrarianView::Menu;
                                    librarian_target_id.clear();
                                }
                                KeyCode::Char('c')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    break;
                                }
                                KeyCode::Backspace => {
                                    librarian_target_id.pop();
                                }
                                KeyCode::Char(character)
                                    if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    librarian_target_id.push(character);
                                }
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
                                KeyCode::Home if !librarian_snapshots.is_empty() => {
                                    librarian_snapshot_selection = 0;
                                }
                                KeyCode::End if !librarian_snapshots.is_empty() => {
                                    librarian_snapshot_selection = librarian_snapshots.len() - 1;
                                }
                                KeyCode::Char(digit @ '1'..='9')
                                    if (digit.to_digit(10).unwrap_or(1) as usize)
                                        <= librarian_snapshots.len() =>
                                {
                                    librarian_snapshot_selection =
                                        digit.to_digit(10).unwrap_or(1) as usize - 1;
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
                                KeyCode::Esc | KeyCode::Backspace => {
                                    librarian_view = LibrarianView::Menu;
                                }
                                KeyCode::Char('q') => break,
                                _ => {}
                            },
                            LibrarianView::SnapshotDetail => match key.code {
                                KeyCode::Esc | KeyCode::Backspace => {
                                    librarian_view = LibrarianView::SnapshotList;
                                    scroll_offset = 0;
                                }
                                KeyCode::Char('q') => break,
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
                                } else if mode == Some("restorer") {
                                    restorer_notice = message;
                                } else {
                                    output.push_back(message);
                                }
                            } else if let Err(error) = interrupt_update(update_child) {
                                let message = format!("Could not interrupt operation: {error}");
                                if mode == Some("librarian") {
                                    librarian_notice = message;
                                } else if mode == Some("restorer") {
                                    restorer_notice = message;
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
                                } else if mode == Some("restorer") {
                                    restorer_notice = message;
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
        GHOSTHUNTER_MENU_ENTRIES, LIBRARIAN_MENU_COMMANDS, LIBRARIAN_MENU_ENTRIES,
        LibrarianSnapshot, START_MENU_ENTRIES, START_TUI_MODES, SnapshotTarget, animated_ghost,
        command_emits_structured_output, format_command_output, format_json_value, format_license,
        format_log_message, format_readme, format_update_report, keep_row_visible, move_scroll,
        progress_display, snapshot_is_restorable, wrapped_line_count,
    };
    use ratatui::text::Line;

    #[test]
    fn scroll_offset_stays_within_content() {
        assert_eq!(move_scroll(3, 10, -8), 0);
        assert_eq!(move_scroll(8, 10, 5), 10);
        assert_eq!(move_scroll(5, 5, 1), 5);
    }

    #[test]
    fn menu_selection_scrolls_into_view_without_exceeding_content() {
        let mut offset = 0;
        keep_row_visible(&mut offset, 10, 4, 8);
        assert_eq!(offset, 5);

        keep_row_visible(&mut offset, 10, 4, 2);
        assert_eq!(offset, 2);

        keep_row_visible(&mut offset, 3, 4, 8);
        assert_eq!(offset, 3);
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
        assert_eq!(START_MENU_ENTRIES.len(), START_TUI_MODES.len());
        assert_eq!(START_MENU_ENTRIES[4].0, "RESTORER");
        assert_eq!(START_TUI_MODES[4], "--run-restorer");
        assert_eq!(START_MENU_ENTRIES[5].0, "ADVISOR");
        assert_eq!(START_TUI_MODES[5], "--run-advisor");
    }

    #[test]
    fn maintenance_actions_are_available_in_their_thematic_menus() {
        assert_eq!(GHOSTHUNTER_MENU_ENTRIES.len(), 3);
        assert_eq!(LIBRARIAN_MENU_ENTRIES.len(), LIBRARIAN_MENU_COMMANDS.len());
        assert!(
            LIBRARIAN_MENU_ENTRIES
                .iter()
                .all(|(_label, description)| !description.is_empty())
        );
        assert_eq!(LIBRARIAN_MENU_COMMANDS[4], ["refresh"]);
        assert_eq!(LIBRARIAN_MENU_COMMANDS[5], ["inventory", "--json"]);
        assert_eq!(LIBRARIAN_MENU_COMMANDS[6], ["history", "--json"]);
    }

    #[test]
    fn structured_results_are_presented_as_readable_fields() {
        let value = serde_json::json!({
            "target": {"name": "home-node", "kind": "host"},
            "package_count": 2,
            "packages": {"pacmonster": "0.9.6", "linux": "6.1"},
            "restorable": true
        });
        let display = format_json_value(&value).join("\n");
        assert!(display.contains("Target"));
        assert!(display.contains("Name: home-node"));
        assert!(display.contains("Package Count: 2"));
        assert!(display.contains("pacmonster — 0.9.6"));
        assert!(display.contains("Restorable: Yes"));
        assert!(!display.contains('{'));
        assert!(!display.contains('}'));
    }

    #[test]
    fn structured_log_messages_are_expanded_and_structured_streams_are_buffered() {
        let message = format_log_message(r#"{"event":"assigned","target":"web-1"}"#);
        assert!(message.contains("Event: assigned"));
        assert!(message.contains("Target: web-1"));
        assert!(!message.contains('{'));
        assert!(command_emits_structured_output(Some("librarian-show")));
        assert!(command_emits_structured_output(Some("updates")));
    }

    #[test]
    fn update_report_lists_systems_and_changed_package_versions() {
        let lines = format_update_report(&[r#"{
            "summary": {
                "systems_attempted": 2,
                "systems_succeeded": 1,
                "systems_failed": 1,
                "packages_changed": 1
            },
            "updated_systems": [{
                "id": "101",
                "name": "web-1",
                "packages": [{
                    "name": "libdemo",
                    "change": "updated",
                    "previous_version": "1.0",
                    "current_version": "2.0"
                }]
            }],
            "failed_systems": [{"id": "102", "name": "db-1", "packages": []}],
            "systems": []
        }"#
        .to_string()])
        .expect("valid update report");
        let display = lines.join("\n");
        assert!(display.contains("UPDATED SYSTEMS"));
        assert!(display.contains("web-1 (ID 101)"));
        assert!(display.contains("libdemo: 1.0 -> 2.0 (updated)"));
        assert!(display.contains("FAILED SYSTEMS"));
        assert!(display.contains("db-1 (ID 102)"));
        assert!(!display.contains('{'));
    }

    #[test]
    fn update_error_report_is_not_presented_as_an_empty_success() {
        let lines = format_update_report(&[
            r#"{"schema_version":1,"status":"error","error":"locked","message":"Another update is already running."}"#
                .to_string(),
        ])
        .expect("valid update error report");
        assert_eq!(
            lines,
            [
                "UPDATE DID NOT COMPLETE",
                "Another update is already running."
            ]
        );
    }

    #[test]
    fn structured_command_output_never_falls_back_to_a_raw_json_dump() {
        let lines = format_command_output(
            &["{\"status\":\"success\",\"restored\":[\"kernel\"]}".to_string()],
            &[],
        );
        let display = lines.join("\n");
        assert!(display.contains("Status: success"));
        assert!(display.contains("Restored (1)"));
        assert!(display.contains("kernel"));
        assert!(!display.contains('{'));
        assert!(!display.contains('}'));

        let malformed = format_command_output(&["{\"status\":".to_string()], &[]);
        let display = malformed.join("\n");
        assert!(display.contains("could not be interpreted"));
        assert!(!display.contains("{\"status\":"));
    }

    #[test]
    fn manifest_only_snapshot_cannot_be_marked_restorable() {
        let snapshot = LibrarianSnapshot {
            id: "snapshot-id".to_string(),
            created: "2026-10-25T08:00:00Z".to_string(),
            target: SnapshotTarget {
                name: "host".to_string(),
            },
            package_count: 2,
            archive_bytes: 0,
            restorable: false,
        };
        assert!(!snapshot_is_restorable(&snapshot));
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
