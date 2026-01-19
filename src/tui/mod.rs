#![allow(dead_code)]

mod progress;
mod widgets;

use std::io::{self, stdout, IsTerminal};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub use progress::{run_progress_ui, ProgressMessage};
pub use widgets::*;

use crate::error::Result;

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Check if the terminal is available for TUI rendering
pub fn is_tty_available() -> bool {
    stdout().is_terminal()
}

/// Reset terminal to a clean state (useful after subprocess with inherited stdio)
pub fn reset_terminal() -> io::Result<()> {
    // Ensure we're not in raw mode
    let _ = disable_raw_mode();
    // Ensure we're not in alternate screen
    let _ = execute!(stdout(), LeaveAlternateScreen);
    Ok(())
}

/// Initialize the terminal for TUI rendering
pub fn init() -> Result<Tui> {
    // First, ensure terminal is in a clean state
    let _ = reset_terminal();

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its original state
pub fn restore() -> Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// Run a one-shot render (display once and exit)
pub fn render_once<F>(render_fn: F) -> Result<()>
where
    F: FnOnce(&mut ratatui::Frame),
{
    let mut terminal = init()?;
    terminal.draw(render_fn)?;

    // Wait for any key press
    crossterm::event::read()?;

    restore()?;
    Ok(())
}
