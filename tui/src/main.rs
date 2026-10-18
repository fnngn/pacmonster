use std::fs::File;
use std::io;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

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
    let file = File::open("/var/tmp/pacmonster-state.json").ok()?;
    serde_json::from_reader(file).ok()
}

fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

fn main() -> io::Result<()> {

    std::panic::set_hook(Box::new(|panic| {
        restore_terminal();
        eprintln!("{panic}");
    }));

    terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen
    )?;

    let backend =
        CrosstermBackend::new(io::stdout());

    let mut terminal =
        Terminal::new(backend)?;

    terminal.clear()?;
    terminal.hide_cursor()?;

    loop {

        let state = read_state();

        terminal.draw(|frame| {

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(8),
                    Constraint::Min(1),
                ])
                .split(frame.area());

            let active = state.as_ref().map(|s| s.active).unwrap_or(0);
            let idle = state.as_ref().map(|s| s.idle).unwrap_or(0);
            let available = state.as_ref()
                .map(|s| s.capacity.saturating_sub(s.active))
                .unwrap_or(0);

            let capacity_lines = vec![
                Line::from("Ghost Pool"),
                Line::from(""),
                Line::from(format!("ᗣ Active      {}", active)),
                Line::from(format!("◌ Idle        {}", idle)),
                Line::from(format!("◌ Available   {}", available)),
            ];

            let capacity_widget =
                Paragraph::new(capacity_lines);

            frame.render_widget(
                capacity_widget,
                chunks[0]
            );

            let ghost_lines: Vec<Line> = state
                .map(|s| {
                    s.ghosts
                        .into_iter()
                        .filter(|g| g.busy)
                        .map(|g| {
                            Line::from(format!(
                                "ᗣ G{:02} {}",
                                g.id,
                                g.target.unwrap_or_default()
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();

            let status_widget =
                Paragraph::new(ghost_lines);

            frame.render_widget(
                status_widget,
                chunks[1]
            );

        })?;

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('q') {
                break;
            }
        }
    }

    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen
    )?;

    Ok(())
}
