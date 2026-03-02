use ratatui::{
    Frame,
    layout::{
        Alignment,
        Constraint,
        Direction,
        Layout,
        Rect,
    },
    style::{
        Color,
        Style,
    },
    text::{Line, Text},
    widgets::{
        Block,
        BorderType,
        Borders,
        Paragraph,
    },
};

pub struct Home;

impl Home {
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let content = Block::default()
            .title(Line::from("Dashboard").centered())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::White))
            .border_type(BorderType::Double);
        f.render_widget(&content, area);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(70),
            ])
            .split(content.inner(area));
        let paragraph = Paragraph::new(self.banner())
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center);
        f.render_widget(paragraph, vertical[1]);
    }

    fn banner(&self) -> Vec<Line<'static>> {
        vec![
            Line::raw("░████████   ░██                       ░██          ░██       ░██ ░██                    "),
            Line::raw("░██    ░██  ░██                       ░██          ░██       ░██                        "),
            Line::raw("░██    ░██  ░██  ░██████    ░███████  ░██    ░██   ░██  ░██  ░██ ░██░██░████  ░███████  "),
            Line::raw("░████████   ░██       ░██  ░██    ░██ ░██   ░██    ░██ ░████ ░██ ░██░███     ░██    ░██ "),
            Line::raw("░██     ░██ ░██  ░███████  ░██        ░███████     ░██░██ ░██░██ ░██░██      ░█████████ "),
            Line::raw("░██     ░██ ░██ ░██   ░██  ░██    ░██ ░██   ░██    ░████   ░████ ░██░██      ░██        "),
            Line::raw("░█████████  ░██  ░█████░██  ░███████  ░██    ░██   ░███     ░███ ░██░██       ░███████  "),
            Line::raw(""),
            Line::raw(""),
            Line::raw(""),
            Line::raw("Enter<ENT>"),
        ]
    }
}
