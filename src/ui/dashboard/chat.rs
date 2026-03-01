use ratatui::{
    Frame,
    layout::{
        Constraint,
        Direction,
        Layout,
    },
    style::{
        Color,
        Style,
    },
    widgets::{
        Block,
        Borders,
        List,
        ListState,
        ListItem,
        Paragraph,
    },
};

use crate::app::App;


pub struct Chat;

impl Chat {
    pub fn render(&mut self, app: &App, f: &mut Frame, list_state: &mut ListState) {
        let chucks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(10), Constraint::Percentage(90)])
            .split(f.size());

        let items: Vec<ListItem> = app
            .items
            .iter()
            .map(|i| ListItem::new(*i))
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Conversas").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::Blue))
            .highlight_symbol(">> ");

        list_state.select(Some(app.selected));
        f.render_stateful_widget(list, chucks[0], list_state);

        let content = format!("Sua conversa com: {}", app.items[app.selected]);
        let paragraph = Paragraph::new(content)
        .block(Block::default().title("Conteúdo").borders(Borders::ALL));

        f.render_widget(paragraph, chucks[1]);
    }
}
