use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
};

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

pub(crate) fn selection_style() -> Style {
    Style::default()
        .bg(Color::Rgb(28, 32, 38))
        .fg(Color::Rgb(232, 236, 241))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn toast_border_style() -> Style {
    Style::default()
        .bg(Color::Rgb(19, 22, 28))
        .fg(Color::Rgb(214, 92, 92))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn toast_body_style() -> Style {
    Style::default()
        .bg(Color::Rgb(19, 22, 28))
        .fg(Color::Rgb(236, 239, 243))
}

pub(crate) fn message_viewport_height(area: Rect) -> u16 {
    area.height.saturating_sub(2)
}

pub(crate) fn wrapped_line_height(line: &Line<'_>, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }

    let visual_width = line.width().max(1);
    let width = usize::from(width);
    visual_width.div_ceil(width) as u16
}
