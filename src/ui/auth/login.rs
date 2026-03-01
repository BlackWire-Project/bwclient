use ratatui::{
    layout::Rect,
    widgets::{
        Block,
        BorderType,
        Paragraph,
    },
    Frame,
};


pub struct Login {
    username: String,
}

impl Login {
    pub fn new() -> Self {
        Self {
            username: String::new(),
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let content = format!(
            "Username: {}",
            self.username
        );

        let widget = Paragraph::new(content)
            .block(Block::default().title("Login").border_type(BorderType::Double));

        f.render_widget(widget, area);
    }
}
