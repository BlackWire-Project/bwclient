use ratatui::{
    Frame, layout::Rect, style::{
        Color,
        Style,
    },
    text::{
        Line,
        Text,
    },
    widgets::{
        Block,
        BorderType,
        Borders, Paragraph,
    }
};

pub struct Home;

impl Home {
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Line::from("Dashboard").centered())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .border_type(BorderType::Double);

        let paragraph = Paragraph::new(Text::raw("Welcome"))
            .block(block)
            .centered();

        f.render_widget(paragraph, area);
    }
}
