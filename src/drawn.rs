use std::io;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{
        Constraint,
        Direction,
        Layout,
    }, style::{
        Color,
        Style,
    }, widgets::{
        Block,
        Borders,
        List,
        ListItem,
        ListState,
        Paragraph,
    }
};
use crate::app::App;

pub struct DrwanApp;

impl DrwanApp {
    pub fn new() -> Self {
        Self
    }

    pub fn render_app(&mut self, app: &App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, list_state: &mut ListState) -> io::Result<()> {
        terminal.draw(|f| {

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
        })?;

        Ok(())
    }
}
