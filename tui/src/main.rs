use std::io;

use crossterm::event;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::execute;
use crossterm::terminal;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;

use ratatui::backend::CrosstermBackend;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Cell;
use ratatui::widgets::Gauge;
use ratatui::widgets::Row;
use ratatui::widgets::Table;
use ratatui::widgets::TableState;
use ratatui::Terminal;

/// Render a mock Pacmonster dashboard.
/// Restore terminal state on panic or exit.
fn restore_terminal() {
    let _ = terminal::disable_raw_mode();

    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen
    );
}

fn main() -> io::Result<()> {

    // Restore terminal on panic.
    std::panic::set_hook(Box::new(|panic| {
        restore_terminal();
        eprintln!("{panic}");
    }));

    terminal::enable_raw_mode()?;

    // Switch to alternate screen buffer.
    execute!(
        io::stdout(),
        EnterAlternateScreen
    )?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Ensure a clean initial render.
    terminal.clear()?;

    // Hide cursor while TUI is active.
    terminal.hide_cursor()?;

    loop {

        terminal.draw(|frame| {

            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5),
                    Constraint::Length(8),
                    Constraint::Min(8),
                ])
                .split(frame.area());

            // Global progress view.
            let progress = Gauge::default()
                .block(
                    Block::default()
                        .title("PACMONSTER")
                        .borders(Borders::ALL)
                )
                .use_unicode(false)
                .percent(66)
                .label("66%");

            frame.render_widget(progress, layout[0]);

            // Active ghost pool.
            let ghost_rows = vec![
                Row::new(vec![
                    Cell::from("1"),
                    Cell::from("tvwc"),
                    Cell::from("Running"),
                ]),
                Row::new(vec![
                    Cell::from("2"),
                    Cell::from("kuma"),
                    Cell::from("Running"),
                ]),
                Row::new(vec![
                    Cell::from("3"),
                    Cell::from("storage"),
                    Cell::from("Complete"),
                ]),
                Row::new(vec![
                    Cell::from("4"),
                    Cell::from("pve"),
                    Cell::from("Running"),
                ]),
            ];

            let ghosts = Table::new(
                ghost_rows,
                [
                    Constraint::Length(8),
                    Constraint::Length(20),
                    Constraint::Length(16),
                ]
            )
            .header(Row::new(vec![
                "Ghost",
                "Target",
                "Status",
            ]))
            .block(
                Block::default()
                    .title("Ghost Pool")
                    .borders(Borders::ALL)
            );

            frame.render_stateful_widget(
                ghosts,
                layout[1],
                &mut TableState::default()
            );

            // Recent events ring buffer.
            let event_rows = vec![
                Row::new(vec![
                    Cell::from("1.2s"),
                    Cell::from("tvwc completed"),
                ]),
                Row::new(vec![
                    Cell::from("0.9s"),
                    Cell::from("storage completed"),
                ]),
                Row::new(vec![
                    Cell::from("live"),
                    Cell::from("homepage running"),
                ]),
            ];

            let events = Table::new(
                event_rows,
                [
                    Constraint::Length(12),
                    Constraint::Min(20),
                ]
            )
            .header(Row::new(vec![
                "Time",
                "Event",
            ]))
            .block(
                Block::default()
                    .title("Events")
                    .borders(Borders::ALL)
            );

            frame.render_stateful_widget(
                events,
                layout[2],
                &mut TableState::default()
            );
        })?;

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('q') {
                break;
            }
        }
    }

    // Restore terminal state.
    terminal::disable_raw_mode()?;

    execute!(
        io::stdout(),
        LeaveAlternateScreen
    )?;

    Ok(())
}
