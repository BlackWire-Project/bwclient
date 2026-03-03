use std::io;
use crossterm::{
    event::{
        self,
        Event,
        KeyCode

    },
    execute,
    terminal::{
        EnterAlternateScreen,
        LeaveAlternateScreen,
        disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{
    Terminal,
    prelude::CrosstermBackend,
    widgets::ListState
};
use crate::{
    drawn::{
        DrwanApp,
        ScreenLocal
    },
    ui::dashboard::Chat
};

#[derive(Debug, Clone)]
pub struct App {
    pub items: Vec<&'static str>,
    pub selected: usize,

    exit_app: bool,
}

impl App {

    pub fn new() -> Self {
        Self {
            items: vec!["Bob", "Filipe", "Silver"],
            selected: 0,

            exit_app: false,
        }
    }

    pub fn run(&mut self) -> Result<(), io::Error> {
        enable_raw_mode()?;

        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut drawn_app = DrwanApp::new();
        let mut list_state = ListState::default();
        list_state.select(Some(self.selected));

        while !self.exit_app {
            let result = drawn_app.render_app(&self, &mut terminal, &mut list_state);

            if let Ok(events) = event::read() {
                self.handle_events(events, &mut drawn_app);
            }

            if let Err(err) = result {
                print!("{:?}", err);
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    pub fn next_select_menu(&mut self) {
        self.selected = (self.selected + 1) % self.items.len();
    }

    pub fn previous_select_menu(&mut self) {
        if self.selected == 0 {
            self.selected = self.items.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn handle_events(&mut self, events: Event, app_render: &mut DrwanApp) -> io::Result<()> {
        if let Event::Key(key) = events {
            match key.code {
                KeyCode::Char('q') => {
                    self.exit()
                },
                KeyCode::Enter => {
                    match app_render.get_screen() {
                        ScreenLocal::Chat(d) => {},
                        ScreenLocal::Home(d) => app_render.set_screen(ScreenLocal::Chat(Chat)),
                    }
                },
                KeyCode::Down => self.next_select_menu(),
                KeyCode::Up => self.previous_select_menu(),
                _ => {}
            }
        }

        Ok(())
    }

    pub fn exit(&mut self) {
        self.exit_app = !self.exit_app;
    }

}
