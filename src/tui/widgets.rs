#![allow(dead_code)]

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Brand colors for spuff
pub mod colors {
    use ratatui::style::Color;

    pub const PRIMARY: Color = Color::Cyan;
    pub const SUCCESS: Color = Color::Green;
    pub const WARNING: Color = Color::Yellow;
    pub const ERROR: Color = Color::Red;
    pub const MUTED: Color = Color::DarkGray;
    pub const TEXT: Color = Color::White;
}

/// The spuff banner as ASCII art
pub const BANNER: &str = r#"
╔═══════════════════════════╗
║  s p u f f                ║
║  ephemeral dev env        ║
╚═══════════════════════════╝"#;

/// Render the banner widget
pub fn render_banner(frame: &mut Frame, area: Rect) {
    let banner = Paragraph::new(BANNER.trim())
        .style(Style::default().fg(colors::PRIMARY))
        .alignment(Alignment::Center);
    frame.render_widget(banner, area);
}

/// Status indicator for instance state
#[derive(Clone, Copy)]
pub enum StatusIndicator {
    Active,
    Starting,
    Stopped,
    Error,
}

impl StatusIndicator {
    pub fn symbol(&self) -> &'static str {
        match self {
            StatusIndicator::Active => "●",
            StatusIndicator::Starting => "◐",
            StatusIndicator::Stopped => "○",
            StatusIndicator::Error => "✕",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            StatusIndicator::Active => colors::SUCCESS,
            StatusIndicator::Starting => colors::WARNING,
            StatusIndicator::Stopped => colors::MUTED,
            StatusIndicator::Error => colors::ERROR,
        }
    }
}

/// Information about an instance to display
pub struct InstanceInfo {
    pub name: String,
    pub ip: String,
    pub provider: String,
    pub region: String,
    pub size: String,
    pub uptime: String,
    pub status: StatusIndicator,
}

/// Render an instance info card
pub fn render_instance_card(frame: &mut Frame, area: Rect, info: &InstanceInfo) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::PRIMARY))
        .title(Span::styled(
            " Instance ",
            Style::default().fg(colors::PRIMARY).bold(),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(vec![
            Span::styled(info.status.symbol(), Style::default().fg(info.status.color())),
            Span::raw(" "),
            Span::styled(&info.name, Style::default().fg(colors::TEXT).bold()),
            Span::raw(" "),
            Span::styled(format!("({})", &info.ip), Style::default().fg(colors::MUTED)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Provider  ", Style::default().fg(colors::MUTED)),
            Span::styled(&info.provider, Style::default().fg(colors::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Region    ", Style::default().fg(colors::MUTED)),
            Span::styled(&info.region, Style::default().fg(colors::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Size      ", Style::default().fg(colors::MUTED)),
            Span::styled(&info.size, Style::default().fg(colors::TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Uptime    ", Style::default().fg(colors::MUTED)),
            Span::styled(&info.uptime, Style::default().fg(colors::WARNING)),
        ]),
    ];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the "no instance" state
pub fn render_no_instance(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::MUTED));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(vec![
            Span::styled("○", Style::default().fg(colors::MUTED)),
            Span::raw(" "),
            Span::styled("No active environment", Style::default().fg(colors::MUTED)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  Run "),
            Span::styled("spuff up", Style::default().fg(colors::PRIMARY)),
            Span::raw(" to create one."),
        ]),
    ];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render keyboard hints at the bottom
pub fn render_hints(frame: &mut Frame, area: Rect, hints: &[(&str, &str)]) {
    let spans: Vec<Span> = hints
        .iter()
        .enumerate()
        .flat_map(|(i, (key, desc))| {
            let mut parts = vec![
                Span::styled(*key, Style::default().fg(colors::PRIMARY).bold()),
                Span::raw(" "),
                Span::styled(*desc, Style::default().fg(colors::MUTED)),
            ];
            if i < hints.len() - 1 {
                parts.push(Span::raw("  │  "));
            }
            parts
        })
        .collect();

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

/// Progress step state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepState {
    Pending,
    InProgress,
    Done,
    Failed,
}

impl StepState {
    pub fn symbol(&self) -> &'static str {
        match self {
            StepState::Pending => "○",
            StepState::InProgress => "◐",
            StepState::Done => "✓",
            StepState::Failed => "✕",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            StepState::Pending => colors::MUTED,
            StepState::InProgress => colors::PRIMARY,
            StepState::Done => colors::SUCCESS,
            StepState::Failed => colors::ERROR,
        }
    }
}


/// Create a centered layout with max width
pub fn centered_rect(max_width: u16, height: u16, area: Rect) -> Rect {
    let width = max_width.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;

    Rect::new(x, y, width, height.min(area.height))
}

/// Layout helper for main content
pub fn main_layout(area: Rect) -> (Rect, Rect, Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Banner
            Constraint::Min(8),     // Content
            Constraint::Length(1),  // Hints
        ])
        .split(area);

    (layout[0], layout[1], layout[2])
}
