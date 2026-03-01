use std::io;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    widgets::ListState,
};
use crate::{
    app::App,
    ui::dashboard::{
        Chat,
        Home,
    },
};

pub enum ScreenLocal {
    Chat(Chat),
    Home(Home),
}

pub struct DrwanApp {
    pub screen: ScreenLocal,
}

impl DrwanApp {
    pub fn new() -> Self {
        Self {
            screen: ScreenLocal::Home(Home),
        }
    }

    pub fn render_app(&mut self, app: &App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, list_state: &mut ListState) -> io::Result<()> {
        terminal.draw(|f| {
            let size = f.size();

            match &mut self.screen {
                ScreenLocal::Chat(chat) => chat.render(app, f, list_state),
                ScreenLocal::Home(dashboard) => dashboard.render(f, size),
            }
        })?;

        Ok(())
    }
}
