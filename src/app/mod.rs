use std::{
    collections::BTreeSet,
    io,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{
    crypto::{
        MessageHeader, available_prekeys, generate_more_prekeys, generate_profile_material,
        identity_json, parse_identity_json, prepare_initial_message, prepare_session_message,
        receive_prekey_message, receive_session_message, rotate_local_ratchet,
    },
    relay::{
        PostMessageRequest, PostMessageResponse, RegisterUserRequest, RelayClient,
        ensure_server_health,
    },
    state::{
        ContactRecord, ConversationRecord, LocalProfileRecord, MessageDirection, MessageRecord,
        MessageStatus, ServerRecord,
    },
    storage::{NewMessage, Storage, UpsertContact},
    sync::{SyncEvent, SyncService},
    ui,
};

mod actions;
mod input;
mod state;
mod sync;
pub(crate) mod view;

pub(crate) use state::{
    ContactFormState, FormState, IngestOutcome, InputMode, LoginFocus, Screen, ToastKind,
    ToastMode, ToastState,
};

pub struct App {
    pub(crate) storage: Storage,
    pub(crate) sync: SyncService,
    pub(crate) exit: bool,
    pub(crate) status: String,
    pub(crate) show_technical: bool,
    pub(crate) screen: Screen,
    pub(crate) login_focus: LoginFocus,
    pub(crate) servers: Vec<ServerRecord>,
    pub(crate) selected_server: usize,
    pub(crate) profiles: Vec<LocalProfileRecord>,
    pub(crate) selected_profile: usize,
    pub(crate) active_server: Option<ServerRecord>,
    pub(crate) active_profile: Option<LocalProfileRecord>,
    pub(crate) conversations: Vec<(ConversationRecord, ContactRecord)>,
    pub(crate) selected_conversation: usize,
    pub(crate) messages: Vec<MessageRecord>,
    pub(crate) message_scroll: u16,
    pub(crate) message_auto_follow: bool,
    pub(crate) composer: String,
    pub(crate) input_mode: InputMode,
    pub(crate) unread_conversations: BTreeSet<i64>,
    pub(crate) last_poll_count: usize,
    pub(crate) last_ingest_stored: usize,
    pub(crate) last_ingest_unresolved: usize,
    pub(crate) last_receive_error: Option<String>,
    pub(crate) toast: Option<ToastState>,
    pub(crate) started_at: Instant,
}

impl App {
    pub fn bootstrap() -> Result<Self> {
        let storage = Storage::open_default()?;
        let mut app = Self {
            storage,
            sync: SyncService::new(),
            exit: false,
            status: String::new(),
            show_technical: true,
            screen: Screen::Login,
            login_focus: LoginFocus::Servers,
            servers: Vec::new(),
            selected_server: 0,
            profiles: Vec::new(),
            selected_profile: 0,
            active_server: None,
            active_profile: None,
            conversations: Vec::new(),
            selected_conversation: 0,
            messages: Vec::new(),
            message_scroll: 0,
            message_auto_follow: true,
            composer: String::new(),
            input_mode: InputMode::Command,
            unread_conversations: BTreeSet::new(),
            last_poll_count: 0,
            last_ingest_stored: 0,
            last_ingest_unresolved: 0,
            last_receive_error: None,
            toast: None,
            started_at: Instant::now(),
        };
        app.refresh_login_data()?;
        if app.servers.is_empty() {
            app.status = "No relay configured. Press 'a' to add a server.".to_string();
        }
        Ok(app)
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while !self.exit {
            self.expire_toast_if_needed();
            self.consume_sync_events();
            terminal.draw(|frame| self.draw(frame))?;
            if event::poll(std::time::Duration::from_millis(120))? {
                let event = event::read()?;
                self.handle_event(event)?;
            }
        }
        self.sync.stop();
        Ok(())
    }

    fn draw(&self, frame: &mut ratatui::Frame<'_>) {
        ui::render_app(self, frame);
    }
}
