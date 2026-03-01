use std::io;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    widgets::ListState,
};
use crate::{
    app::App,
    ui::{
        dashboard::Chat,
    },
};

pub enum ScreenLocal {
    Dashboard(Chat),
}

pub struct DrwanApp {
    pub screen: ScreenLocal,
}

impl DrwanApp {
    pub fn new() -> Self {
        Self {
            screen: ScreenLocal::Dashboard(Chat),
        }
    }

    pub fn render_app(&mut self, app: &App, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, list_state: &mut ListState) -> io::Result<()> {
        terminal.draw(|f| {
            let size = f.size();

            match &mut self.screen {
                ScreenLocal::Dashboard(wid) => wid.render(app, f, list_state),
            }
        })?;

        Ok(())
    }
}
