use std::io::{self, stdout, IsTerminal};
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use tokio::sync::mpsc;

use super::widgets::{colors, StepState};

const BANNER: &str = r#"╔═══════════════════════════╗
║  s p u f f                ║
║  ephemeral dev env        ║
╚═══════════════════════════╝"#;

/// A progress step with optional sub-steps
#[derive(Debug, Clone)]
pub struct ProgressStep {
    pub name: String,
    pub state: StepState,
    pub sub_steps: Vec<SubStep>,
}

/// A sub-step within a main step
#[derive(Debug, Clone)]
pub struct SubStep {
    pub name: String,
    pub state: StepState,
}

/// Messages to update the progress UI
#[derive(Debug, Clone)]
pub enum ProgressMessage {
    /// Update step state
    SetStep(usize, StepState),
    /// Update the detail message
    SetDetail(String),
    /// Set sub-steps for a step
    SetSubSteps(usize, Vec<String>),
    /// Update a sub-step state
    SetSubStep(usize, usize, StepState),
    /// Mark as completed successfully
    Complete(String, String), // (instance_name, ip)
    /// Mark as failed
    Failed(String),
    /// Close the TUI
    Close,
}

/// Run the progress TUI
pub async fn run_progress_ui(
    steps: Vec<String>,
    mut rx: mpsc::Receiver<ProgressMessage>,
) -> io::Result<Option<(String, String)>> {
    // Check if we have a TTY available
    if !stdout().is_terminal() {
        // Fallback to text-only progress
        return run_text_progress(steps, rx).await;
    }

    // Reset terminal to clean state before initializing TUI
    // This helps when previous commands (like cargo build) left terminal in a weird state
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);

    // Now try to initialize TUI
    if let Err(e) = enable_raw_mode() {
        eprintln!("Warning: Could not enable raw mode ({}), falling back to text progress", e);
        return run_text_progress(steps, rx).await;
    }

    if let Err(e) = execute!(stdout(), EnterAlternateScreen) {
        let _ = disable_raw_mode();
        eprintln!("Warning: Could not enter alternate screen ({}), falling back to text progress", e);
        return run_text_progress(steps, rx).await;
    }

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            let _ = disable_raw_mode();
            let _ = execute!(stdout(), LeaveAlternateScreen);
            eprintln!("Warning: Could not create terminal ({}), falling back to text progress", e);
            return run_text_progress(steps, rx).await;
        }
    };

    let mut progress_steps: Vec<ProgressStep> = steps
        .into_iter()
        .map(|name| ProgressStep {
            name,
            state: StepState::Pending,
            sub_steps: vec![],
        })
        .collect();

    let mut detail = String::new();
    let mut result: Option<(String, String)> = None;
    let mut is_complete = false;
    let mut is_failed = false;
    let mut error_msg = String::new();

    loop {
        // Draw the UI
        terminal.draw(|frame| {
            draw_progress_ui(
                frame,
                &progress_steps,
                &detail,
                is_complete,
                is_failed,
                &error_msg,
                result.as_ref(),
            );
        })?;

        // Check for messages (non-blocking)
        if let Ok(msg) = rx.try_recv() {
            match msg {
                ProgressMessage::SetStep(idx, state) => {
                    if idx < progress_steps.len() {
                        progress_steps[idx].state = state;
                    }
                }
                ProgressMessage::SetDetail(d) => {
                    detail = d;
                }
                ProgressMessage::SetSubSteps(idx, names) => {
                    if idx < progress_steps.len() {
                        progress_steps[idx].sub_steps = names
                            .into_iter()
                            .map(|name| SubStep {
                                name,
                                state: StepState::Pending,
                            })
                            .collect();
                    }
                }
                ProgressMessage::SetSubStep(step_idx, sub_idx, state) => {
                    if step_idx < progress_steps.len() {
                        let step = &mut progress_steps[step_idx];
                        if sub_idx < step.sub_steps.len() {
                            step.sub_steps[sub_idx].state = state;
                        }
                    }
                }
                ProgressMessage::Complete(name, ip) => {
                    result = Some((name, ip));
                    is_complete = true;
                }
                ProgressMessage::Failed(msg) => {
                    is_failed = true;
                    error_msg = msg;
                }
                ProgressMessage::Close => {
                    break;
                }
            }
        }

        // Check for key events (with timeout)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                    break;
                }
                // Any key to continue after completion/failure
                if is_complete || is_failed {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    Ok(result)
}

fn draw_progress_ui(
    frame: &mut Frame,
    steps: &[ProgressStep],
    detail: &str,
    is_complete: bool,
    is_failed: bool,
    error_msg: &str,
    result: Option<&(String, String)>,
) {
    let area = frame.area();

    // Calculate total lines needed (main steps + visible sub-steps)
    let total_lines: usize = steps
        .iter()
        .map(|s| {
            1 + if s.state == StepState::InProgress && !s.sub_steps.is_empty() {
                s.sub_steps.len()
            } else {
                0
            }
        })
        .sum();

    // Calculate centered area
    let width = 55.min(area.width.saturating_sub(4));
    let height = (total_lines + 14) as u16;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let centered = Rect::new(x, y, width, height.min(area.height));

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // Banner
            Constraint::Length(1),  // Spacing
            Constraint::Min(8),     // Progress steps
            Constraint::Length(2),  // Detail/status
            Constraint::Length(1),  // Hints
        ])
        .split(centered);

    // Banner
    let banner = Paragraph::new(BANNER)
        .style(Style::default().fg(colors::PRIMARY))
        .alignment(Alignment::Center);
    frame.render_widget(banner, layout[0]);

    // Progress steps
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if is_failed {
            colors::ERROR
        } else if is_complete {
            colors::SUCCESS
        } else {
            colors::PRIMARY
        }))
        .title(Span::styled(
            if is_complete {
                " Ready "
            } else if is_failed {
                " Failed "
            } else {
                " Creating Environment "
            },
            Style::default()
                .fg(if is_failed {
                    colors::ERROR
                } else if is_complete {
                    colors::SUCCESS
                } else {
                    colors::PRIMARY
                })
                .bold(),
        ));

    let inner = block.inner(layout[2]);
    frame.render_widget(block, layout[2]);

    let mut lines: Vec<Line> = Vec::new();

    for step in steps {
        // Main step line
        let style = if step.state == StepState::InProgress {
            Style::default().fg(colors::TEXT)
        } else if step.state == StepState::Done {
            Style::default().fg(colors::MUTED)
        } else {
            Style::default().fg(colors::MUTED)
        };

        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(step.state.symbol(), Style::default().fg(step.state.color())),
            Span::raw(" "),
            Span::styled(&step.name, style),
        ]));

        // Show sub-steps only when step is in progress
        if step.state == StepState::InProgress && !step.sub_steps.is_empty() {
            for sub in &step.sub_steps {
                let sub_style = match sub.state {
                    StepState::Done => Style::default().fg(colors::MUTED),
                    StepState::InProgress => Style::default().fg(colors::TEXT),
                    _ => Style::default().fg(colors::MUTED),
                };

                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(sub.state.symbol(), Style::default().fg(sub.state.color())),
                    Span::raw(" "),
                    Span::styled(&sub.name, sub_style),
                ]));
            }
        }
    }

    // Add result info if complete
    if let Some((name, ip)) = result {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(" ✓ ", Style::default().fg(colors::SUCCESS)),
            Span::styled(name, Style::default().fg(colors::TEXT).bold()),
            Span::raw(" "),
            Span::styled(format!("({})", ip), Style::default().fg(colors::MUTED)),
        ]));
    }

    // Add error message if failed
    if is_failed && !error_msg.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(" ✕ ", Style::default().fg(colors::ERROR)),
            Span::styled(error_msg, Style::default().fg(colors::ERROR)),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    // Detail line
    if !detail.is_empty() && !is_complete && !is_failed {
        let detail_line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(detail.to_string(), Style::default().fg(colors::MUTED).italic()),
        ]);
        frame.render_widget(Paragraph::new(detail_line), layout[3]);
    }

    // Hints
    let hint_text = if is_complete || is_failed {
        "Press any key to continue"
    } else {
        "Press q to cancel"
    };
    let hints = Paragraph::new(hint_text)
        .style(Style::default().fg(colors::MUTED))
        .alignment(Alignment::Center);
    frame.render_widget(hints, layout[4]);
}

/// Fallback text-only progress when TUI is not available
async fn run_text_progress(
    steps: Vec<String>,
    mut rx: mpsc::Receiver<ProgressMessage>,
) -> io::Result<Option<(String, String)>> {
    use console::style;

    let mut result: Option<(String, String)> = None;

    println!();
    println!("{}", style("Creating Environment").cyan().bold());
    println!();

    // Print initial steps
    for step in steps.iter() {
        println!("  {} {}", style("○").dim(), style(step).dim());
    }

    loop {
        match rx.recv().await {
            Some(ProgressMessage::SetStep(idx, state)) => {
                match state {
                    StepState::InProgress => {
                        if idx < steps.len() {
                            println!("  {} {}", style("→").cyan(), style(&steps[idx]).white());
                        }
                    }
                    StepState::Done => {
                        if idx < steps.len() {
                            println!("  {} {}", style("✓").green(), style(&steps[idx]).dim());
                        }
                    }
                    StepState::Failed => {
                        if idx < steps.len() {
                            println!("  {} {}", style("✕").red(), style(&steps[idx]).red());
                        }
                    }
                    _ => {}
                }
            }
            Some(ProgressMessage::SetDetail(detail)) => {
                if !detail.is_empty() {
                    println!("    {}", style(&detail).dim().italic());
                }
            }
            Some(ProgressMessage::SetSubSteps(_, _)) => {
                // Sub-steps are simplified in text mode
            }
            Some(ProgressMessage::SetSubStep(_, _, state)) => {
                if state == StepState::Done {
                    // Could print sub-step progress but keep it simple
                }
            }
            Some(ProgressMessage::Complete(name, ip)) => {
                result = Some((name.clone(), ip.clone()));
                println!();
                println!("  {} {} ({})", style("✓").green().bold(), style(&name).white().bold(), style(&ip).dim());
            }
            Some(ProgressMessage::Failed(msg)) => {
                println!();
                println!("  {} {}", style("✕").red().bold(), style(&msg).red());
            }
            Some(ProgressMessage::Close) | None => {
                break;
            }
        }
    }

    println!();
    Ok(result)
}
