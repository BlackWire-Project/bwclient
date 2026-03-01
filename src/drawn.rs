use std::io;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    widgets::ListState,
};
use crate::{
    app::App,
    ui::{
        auth::Login,
        user::Chat
    },
};

pub enum ScreenLocal {
    Login(Login),
    Chat(Chat),
}

pub struct DrwanApp {
    pub screen: ScreenLocal,
}

impl DrwanApp {
    pub fn new() -> Self {
        Self {
            screen: ScreenLocal::Login(Login::new()),
        }
    }

    pub fn render_app(&mut self, app: &App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, list_state: &mut ListState) -> io::Result<()> {
        terminal.draw(|f| {
            let size = f.size();

            match &mut self.screen {
                ScreenLocal::Login(wid) => wid.render(f, size),
                ScreenLocal::Chat(wid) => wid.render(app, f, list_state),
            }
        })?;

        Ok(())
    }
}
