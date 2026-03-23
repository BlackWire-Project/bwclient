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
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    crypto::{
        MessageHeader, available_prekeys, generate_more_prekeys, generate_profile_material,
        identity_json, parse_identity_json, prepare_initial_message, prepare_session_message,
        receive_prekey_message, receive_session_message, rotate_local_ratchet,
    },
    relay::{PostMessageRequest, RegisterUserRequest, RelayClient, ensure_server_health},
    state::{
        ContactRecord, ConversationRecord, LocalProfileRecord, MessageDirection, MessageRecord,
        MessageStatus, ServerRecord,
    },
    storage::{NewMessage, Storage, UpsertContact},
    sync::{SyncEvent, SyncService},
};

pub struct App {
    storage: Storage,
    sync: SyncService,
    exit: bool,
    status: String,
    show_technical: bool,
    screen: Screen,
    login_focus: LoginFocus,
    servers: Vec<ServerRecord>,
    selected_server: usize,
    profiles: Vec<LocalProfileRecord>,
    selected_profile: usize,
    active_server: Option<ServerRecord>,
    active_profile: Option<LocalProfileRecord>,
    conversations: Vec<(ConversationRecord, ContactRecord)>,
    selected_conversation: usize,
    messages: Vec<MessageRecord>,
    message_scroll: u16,
    message_auto_follow: bool,
    composer: String,
    input_mode: InputMode,
    unread_conversations: BTreeSet<i64>,
    last_poll_count: usize,
    last_ingest_stored: usize,
    last_ingest_unresolved: usize,
    last_receive_error: Option<String>,
    toast: Option<ToastState>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoginFocus {
    Servers,
    Profiles,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Command,
    Compose,
}

enum Screen {
    Login,
    AddServer(FormState),
    AddProfile(FormState),
    AddContact(ContactFormState),
    Main,
}

enum IngestOutcome {
    Stored {
        conversation_id: i64,
    },
    Unresolved {
        conversation_id: i64,
        reason: String,
    },
    Ignored,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastMode {
    AutoDismiss,
    Sticky,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    ApiError,
    SyncError,
}

struct ToastState {
    kind: ToastKind,
    message: String,
    mode: ToastMode,
    expires_at: Option<Instant>,
}

struct FormState {
    title: &'static str,
    fields: Vec<FormField>,
    index: usize,
}

struct FormField {
    label: &'static str,
    value: String,
}

struct ContactFormState {
    display_name: String,
    value: String,
    use_username: bool,
    field_index: usize,
}

impl FormState {
    fn new(title: &'static str, labels: &[&'static str]) -> Self {
        Self {
            title,
            fields: labels
                .iter()
                .map(|label| FormField {
                    label,
                    value: String::new(),
                })
                .collect(),
            index: 0,
        }
    }

    fn current_mut(&mut self) -> &mut String {
        &mut self.fields[self.index].value
    }
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
        match &self.screen {
            Screen::Login => self.draw_login(frame),
            Screen::AddServer(form) => {
                self.draw_login(frame);
                self.draw_form_overlay(frame, form);
            }
            Screen::AddProfile(form) => {
                self.draw_login(frame);
                self.draw_form_overlay(frame, form);
            }
            Screen::AddContact(form) => {
                self.draw_main(frame);
                self.draw_contact_overlay(frame, form);
            }
            Screen::Main => self.draw_main(frame),
        }
        self.draw_toast(frame);
    }

    fn draw_toast(&self, frame: &mut ratatui::Frame<'_>) {
        let Some(toast) = &self.toast else {
            return;
        };

        let area = centered_rect(56, 22, frame.area());
        let title = match toast.kind {
            ToastKind::ApiError => "API Error",
            ToastKind::SyncError => "Sync Error",
        };
        let footer = match toast.mode {
            ToastMode::AutoDismiss => "This message will close automatically.",
            ToastMode::Sticky => "Press Esc to dismiss.",
        };
        let lines = vec![
            Line::from(toast.message.as_str()),
            Line::from(""),
            Line::from(footer),
        ];

        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(toast_border_style())
                        .style(toast_body_style()),
                )
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn draw_login(&self, frame: &mut ratatui::Frame<'_>) {
        let area = frame.area();
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(3)])
            .split(area);
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(vertical[0]);

        let server_items = if self.servers.is_empty() {
            vec![ListItem::new("No servers configured")]
        } else {
            self.servers
                .iter()
                .map(|server| ListItem::new(format!("{}  {}", server.name, server.base_url)))
                .collect()
        };
        let profile_items = if self.profiles.is_empty() {
            vec![ListItem::new("No local profiles for this server")]
        } else {
            self.profiles
                .iter()
                .map(|profile| {
                    let suffix = if profile.registered {
                        ""
                    } else {
                        " (not registered)"
                    };
                    ListItem::new(format!("{}{}", profile.username, suffix))
                })
                .collect()
        };

        let mut server_state = ListState::default();
        if !self.servers.is_empty() {
            server_state.select(Some(self.selected_server.min(self.servers.len() - 1)));
        }
        let mut profile_state = ListState::default();
        if !self.profiles.is_empty() {
            profile_state.select(Some(self.selected_profile.min(self.profiles.len() - 1)));
        }

        let server_block = Block::default()
            .title("Servers")
            .borders(Borders::ALL)
            .border_style(self.focus_style(LoginFocus::Servers));
        let profile_block = Block::default()
            .title("Profiles")
            .borders(Borders::ALL)
            .border_style(self.focus_style(LoginFocus::Profiles));

        frame.render_stateful_widget(
            List::new(server_items)
                .block(server_block)
                .highlight_style(selection_style())
                .highlight_symbol(">> "),
            split[0],
            &mut server_state,
        );
        frame.render_stateful_widget(
            List::new(profile_items)
                .block(profile_block)
                .highlight_style(selection_style())
                .highlight_symbol(">> "),
            split[1],
            &mut profile_state,
        );

        let status = Paragraph::new(Text::from(vec![
            Line::from("Keys: a add server | n new profile | Tab switch | Enter login | q quit"),
            Line::from(self.status.as_str()),
        ]))
        .block(Block::default().title("Status").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
        frame.render_widget(status, vertical[1]);
    }

    fn draw_main(&self, frame: &mut ratatui::Frame<'_>) {
        let area = frame.area();
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),
                Constraint::Length(4),
                Constraint::Length(3),
            ])
            .split(area);
        let constraints = if self.show_technical {
            vec![
                Constraint::Percentage(24),
                Constraint::Percentage(48),
                Constraint::Percentage(28),
            ]
        } else {
            vec![Constraint::Percentage(28), Constraint::Percentage(72)]
        };
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(vertical[0]);

        let conversation_items = if self.conversations.is_empty() {
            vec![ListItem::new("No conversations yet")]
        } else {
            self.conversations
                .iter()
                .map(|(conversation, contact)| {
                    let label = match (&contact.username, &contact.inbox_id) {
                        (Some(username), _) => username.clone(),
                        (None, Some(inbox)) => format!("inbox: {}", shorten(inbox, 18)),
                        (None, None) => contact.display_name.clone(),
                    };
                    let prefix = if self.unread_conversations.contains(&conversation.id) {
                        "* "
                    } else {
                        ""
                    };
                    ListItem::new(format!("{prefix}{label}"))
                })
                .collect()
        };
        let mut conversation_state = ListState::default();
        if !self.conversations.is_empty() {
            conversation_state.select(Some(
                self.selected_conversation
                    .min(self.conversations.len().saturating_sub(1)),
            ));
        }
        frame.render_stateful_widget(
            List::new(conversation_items)
                .block(
                    Block::default()
                        .title("Conversations")
                        .borders(Borders::ALL),
                )
                .highlight_style(selection_style())
                .highlight_symbol(">> "),
            horizontal[0],
            &mut conversation_state,
        );

        let message_lines = self.message_lines();
        let message_viewport_height = message_viewport_height(horizontal[1]);
        let max_scroll = self.max_message_scroll(horizontal[1], &message_lines);
        let message_scroll = if self.message_auto_follow {
            max_scroll
        } else {
            self.message_scroll.min(max_scroll)
        };
        frame.render_widget(
            Paragraph::new(Text::from(message_lines))
                .block(
                    Block::default()
                        .title(format!(
                            "Messages [PgUp/PgDn/Home/End] ({message_viewport_height} lines)"
                        ))
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false })
                .scroll((message_scroll, 0)),
            horizontal[1],
        );

        if self.show_technical {
            frame.render_widget(self.technical_panel(), horizontal[2]);
        }

        frame.render_widget(
            Paragraph::new(self.composer.as_str())
                .block(
                    Block::default()
                        .title(match self.input_mode {
                            InputMode::Command => "Composer [Command]",
                            InputMode::Compose => "Composer [Compose]",
                        })
                        .borders(Borders::ALL)
                        .border_style(match self.input_mode {
                            InputMode::Command => Style::default(),
                            InputMode::Compose => Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        }),
                )
                .wrap(Wrap { trim: false }),
            vertical[1],
        );

        let status = Paragraph::new(Text::from(vec![
            Line::from(
                "F1 add contact | F2 tech | F5 sync | F6 logout | F9 compose | PgUp/PgDn scroll | q quit",
            ),
            Line::from(format!(
                "poll={} stored={} unresolved={}",
                self.last_poll_count, self.last_ingest_stored, self.last_ingest_unresolved
            )),
            Line::from(self.status.as_str()),
        ]))
        .block(Block::default().title("Status").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
        frame.render_widget(status, vertical[2]);
    }

    fn technical_panel(&self) -> Paragraph<'_> {
        let mut lines = Vec::new();
        if let Some(server) = &self.active_server {
            lines.push(Line::from(format!("server: {}", server.base_url)));
        }
        if let Some(profile) = &self.active_profile {
            lines.push(Line::from(format!("profile: {}", profile.username)));
            lines.push(Line::from(format!("inbox: {}", profile.inbox_id)));
            lines.push(Line::from(format!(
                "local prekeys: {}",
                available_prekeys(&profile.keys)
            )));
        }
        if let Some((conversation, contact)) = self.current_conversation() {
            lines.push(Line::from(""));
            lines.push(Line::from(format!("contact: {}", contact.display_name)));
            if let Some(username) = &contact.username {
                lines.push(Line::from(format!("username: {username}")));
            }
            if let Some(inbox_id) = &contact.inbox_id {
                lines.push(Line::from(format!("remote inbox: {inbox_id}")));
            }
            if let Some(session) = &conversation.session {
                lines.push(Line::from(format!("session: {}", session.session_id)));
                lines.push(Line::from(format!("send_count: {}", session.send_count)));
                lines.push(Line::from(format!("recv_count: {}", session.receive_count)));
                lines.push(Line::from(format!(
                    "await_send_ratchet: {}",
                    session.pending_send_ratchet
                )));
            } else {
                lines.push(Line::from("session: none"));
            }
        }
        if let Some(message) = self.messages.last() {
            lines.push(Line::from(""));
            lines.push(Line::from(format!("last kind: {}", message.relay_kind)));
            lines.push(Line::from(format!(
                "last header: {}",
                shorten(&message.header, 120)
            )));
            if let Some(error_reason) = &message.error_reason {
                lines.push(Line::from(format!("last error: {error_reason}")));
            }
        }
        if let Some(error) = &self.last_receive_error {
            lines.push(Line::from(""));
            lines.push(Line::from(format!("receive error: {error}")));
        }
        lines.push(Line::from(format!(
            "last poll/stored/unresolved: {}/{}/{}",
            self.last_poll_count, self.last_ingest_stored, self.last_ingest_unresolved
        )));

        Paragraph::new(Text::from(lines))
            .block(Block::default().title("Technical").borders(Borders::ALL))
            .wrap(Wrap { trim: true })
    }

    fn draw_form_overlay(&self, frame: &mut ratatui::Frame<'_>, form: &FormState) {
        let popup = centered_rect(60, 40, frame.area());
        frame.render_widget(Clear, popup);
        let mut lines = Vec::new();
        for (idx, field) in form.fields.iter().enumerate() {
            let marker = if idx == form.index { ">" } else { " " };
            lines.push(Line::from(format!(
                "{marker} {}: {}",
                field.label, field.value
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Enter submit | Tab next field | Esc cancel"));

        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(Block::default().title(form.title).borders(Borders::ALL))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }

    fn draw_contact_overlay(&self, frame: &mut ratatui::Frame<'_>, form: &ContactFormState) {
        let popup = centered_rect(60, 40, frame.area());
        frame.render_widget(Clear, popup);
        let mode = if form.use_username {
            "username"
        } else {
            "inbox_id"
        };
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(format!(
                    "{} display_name: {}",
                    if form.field_index == 0 { ">" } else { " " },
                    form.display_name
                )),
                Line::from(format!(
                    "{} {}: {}",
                    if form.field_index == 1 { ">" } else { " " },
                    mode,
                    form.value
                )),
                Line::from(""),
                Line::from("F2 toggle username/inbox mode"),
                Line::from("Enter submit | Tab next field | Esc cancel"),
            ]))
            .block(Block::default().title("Add Contact").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
            popup,
        );
    }

    fn handle_event(&mut self, event: Event) -> Result<()> {
        if let Event::Key(key) = event {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }
            if matches!(key.code, KeyCode::Esc) && self.dismiss_toast() {
                return Ok(());
            }
            let screen = std::mem::replace(&mut self.screen, Screen::Login);
            match screen {
                Screen::Login => {
                    self.screen = Screen::Login;
                    self.handle_login_key(key);
                }
                Screen::AddServer(mut form) => {
                    if self.handle_form_key(key, &mut form) {
                        self.screen = Screen::AddServer(form);
                    }
                }
                Screen::AddProfile(mut form) => {
                    if self.handle_form_key(key, &mut form) {
                        self.screen = Screen::AddProfile(form);
                    }
                }
                Screen::AddContact(mut form) => {
                    if self.handle_contact_form_key(key, &mut form) {
                        self.screen = Screen::AddContact(form);
                    }
                }
                Screen::Main => {
                    self.screen = Screen::Main;
                    self.handle_main_key(key);
                }
            }
        }
        Ok(())
    }

    fn handle_login_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.exit = true,
            KeyCode::Tab => {
                self.login_focus = if self.login_focus == LoginFocus::Servers {
                    LoginFocus::Profiles
                } else {
                    LoginFocus::Servers
                };
            }
            KeyCode::Down => match self.login_focus {
                LoginFocus::Servers => {
                    if !self.servers.is_empty() {
                        self.selected_server = (self.selected_server + 1) % self.servers.len();
                        let _ = self.refresh_profiles();
                    }
                }
                LoginFocus::Profiles => {
                    if !self.profiles.is_empty() {
                        self.selected_profile = (self.selected_profile + 1) % self.profiles.len();
                    }
                }
            },
            KeyCode::Up => match self.login_focus {
                LoginFocus::Servers => {
                    if !self.servers.is_empty() {
                        self.selected_server = if self.selected_server == 0 {
                            self.servers.len() - 1
                        } else {
                            self.selected_server - 1
                        };
                        let _ = self.refresh_profiles();
                    }
                }
                LoginFocus::Profiles => {
                    if !self.profiles.is_empty() {
                        self.selected_profile = if self.selected_profile == 0 {
                            self.profiles.len() - 1
                        } else {
                            self.selected_profile - 1
                        };
                    }
                }
            },
            KeyCode::Char('a') => {
                self.screen = Screen::AddServer(FormState::new("Add Server", &["name", "base_url"]))
            }
            KeyCode::Char('n') => {
                if self.selected_server_record().is_none() {
                    self.status = "Create a server first.".to_string();
                } else {
                    self.screen =
                        Screen::AddProfile(FormState::new("Create Profile", &["username"]));
                }
            }
            KeyCode::Enter => {
                if let Some(profile) = self.selected_profile_record() {
                    match self.activate_profile(profile.id) {
                        Ok(_) => {}
                        Err(error) => {
                            self.show_api_error_toast(error.to_string(), ToastMode::Sticky)
                        }
                    }
                } else {
                    self.status = "Select a profile first.".to_string();
                }
            }
            _ => {}
        }
    }

    fn handle_form_key(&mut self, key: KeyEvent, form: &mut FormState) -> bool {
        let mut keep_open = true;
        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::Login;
                keep_open = false;
            }
            KeyCode::Tab => form.index = (form.index + 1) % form.fields.len(),
            KeyCode::Backspace => {
                form.current_mut().pop();
            }
            KeyCode::Enter => {
                let outcome = match form.title {
                    "Add Server" => self.submit_add_server(form),
                    "Create Profile" => self.submit_add_profile(form),
                    _ => Ok(()),
                };
                match outcome {
                    Ok(_) => keep_open = false,
                    Err(error) => self.show_api_error_toast(error.to_string(), ToastMode::Sticky),
                }
            }
            KeyCode::Char(ch) => form.current_mut().push(ch),
            _ => {}
        }
        keep_open
    }

    fn handle_contact_form_key(&mut self, key: KeyEvent, form: &mut ContactFormState) -> bool {
        let mut keep_open = true;
        match key.code {
            KeyCode::Esc => {
                self.screen = Screen::Main;
                keep_open = false;
            }
            KeyCode::Tab => form.field_index = (form.field_index + 1) % 2,
            KeyCode::Backspace => {
                if form.field_index == 0 {
                    form.display_name.pop();
                } else {
                    form.value.pop();
                }
            }
            KeyCode::F(2) => form.use_username = !form.use_username,
            KeyCode::Enter => match self.submit_add_contact(form) {
                Ok(_) => keep_open = false,
                Err(error) => self.show_api_error_toast(error.to_string(), ToastMode::Sticky),
            },
            KeyCode::Char(ch) => {
                if form.field_index == 0 {
                    form.display_name.push(ch);
                } else {
                    form.value.push(ch);
                }
            }
            _ => {}
        }
        keep_open
    }

    fn handle_main_key(&mut self, key: KeyEvent) {
        match self.input_mode {
            InputMode::Command => match key.code {
                KeyCode::Char('q') => self.exit = true,
                KeyCode::F(1) => {
                    self.screen = Screen::AddContact(ContactFormState {
                        display_name: String::new(),
                        value: String::new(),
                        use_username: true,
                        field_index: 0,
                    });
                }
                KeyCode::F(2) => self.show_technical = !self.show_technical,
                KeyCode::F(5) => self.sync.trigger(),
                KeyCode::F(6) => {
                    self.sync.stop();
                    self.active_profile = None;
                    self.active_server = None;
                    self.conversations.clear();
                    self.messages.clear();
                    self.screen = Screen::Login;
                    self.input_mode = InputMode::Command;
                    let _ = self.refresh_login_data();
                }
                KeyCode::F(9) => self.input_mode = InputMode::Compose,
                KeyCode::PageUp => self.scroll_messages_page(-1),
                KeyCode::PageDown => self.scroll_messages_page(1),
                KeyCode::Home => self.scroll_messages_to_top(),
                KeyCode::End => self.scroll_messages_to_bottom(),
                KeyCode::Down => {
                    if !self.conversations.is_empty() {
                        self.selected_conversation =
                            (self.selected_conversation + 1) % self.conversations.len();
                        self.message_auto_follow = true;
                        self.message_scroll = u16::MAX;
                        let _ = self.reload_messages();
                    }
                }
                KeyCode::Up => {
                    if !self.conversations.is_empty() {
                        self.selected_conversation = if self.selected_conversation == 0 {
                            self.conversations.len() - 1
                        } else {
                            self.selected_conversation - 1
                        };
                        self.message_auto_follow = true;
                        self.message_scroll = u16::MAX;
                        let _ = self.reload_messages();
                    }
                }
                _ => {}
            },
            InputMode::Compose => match key.code {
                KeyCode::Esc => self.input_mode = InputMode::Command,
                KeyCode::Backspace => {
                    self.composer.pop();
                }
                KeyCode::Enter => match self.send_current_message() {
                    Ok(_) => {}
                    Err(error) => self.show_api_error_toast(error.to_string(), ToastMode::Sticky),
                },
                KeyCode::Char(ch) => self.composer.push(ch),
                KeyCode::Tab => self.composer.push('\t'),
                _ => {}
            },
        }
    }

    fn submit_add_server(&mut self, form: &FormState) -> Result<()> {
        let name = form.fields[0].value.trim();
        let base_url = form.fields[1].value.trim().trim_end_matches('/');
        if name.is_empty() || base_url.is_empty() {
            return Err(anyhow!("server name and base_url are required"));
        }
        ensure_server_health(base_url)?;
        let ws_url = Storage::default_ws_url(base_url)?;
        self.storage.create_server(name, base_url, Some(&ws_url))?;
        self.refresh_login_data()?;
        self.status = format!("Server '{name}' added.");
        self.screen = Screen::Login;
        Ok(())
    }

    fn submit_add_profile(&mut self, form: &FormState) -> Result<()> {
        let username = form.fields[0].value.trim();
        if username.is_empty() {
            return Err(anyhow!("username is required"));
        }
        let server = self
            .selected_server_record()
            .cloned()
            .ok_or_else(|| anyhow!("select a server first"))?;
        let relay = RelayClient::new(server.base_url.clone())?;
        let (keys, inbox_id) = generate_profile_material(16)?;
        relay.register_user(&RegisterUserRequest {
            username: username.to_string(),
            identity_key: identity_json(&keys)?,
            signed_prekey: keys.signed_prekey_public.clone(),
            signed_prekey_signature: keys.signed_prekey_signature.clone(),
            inbox_id: inbox_id.clone(),
            one_time_prekeys: keys
                .one_time_prekeys
                .iter()
                .map(|prekey| prekey.public_key.clone())
                .collect(),
        })?;
        self.storage
            .create_profile(server.id, username, &inbox_id, true, &keys)?;
        self.refresh_profiles()?;
        self.status = format!("Profile '{username}' registered on {}.", server.name);
        self.screen = Screen::Login;
        Ok(())
    }

    fn submit_add_contact(&mut self, form: &ContactFormState) -> Result<()> {
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        let server = self
            .active_server
            .clone()
            .ok_or_else(|| anyhow!("no active server"))?;
        let relay = RelayClient::new(server.base_url.clone())?;
        let display_name = if form.display_name.trim().is_empty() {
            if form.use_username {
                form.value.trim().to_string()
            } else {
                shorten(form.value.trim(), 18)
            }
        } else {
            form.display_name.trim().to_string()
        };
        if form.value.trim().is_empty() {
            return Err(anyhow!("username or inbox_id is required"));
        }

        let contact = if form.use_username {
            let bundle = relay.get_bundle(form.value.trim())?;
            self.storage.upsert_contact(
                profile.id,
                &UpsertContact {
                    username: Some(bundle.username.clone()),
                    inbox_id: Some(bundle.inbox_id.clone()),
                    display_name,
                    identity_key: Some(bundle.identity_key.clone()),
                },
            )?
        } else {
            self.storage.upsert_contact(
                profile.id,
                &UpsertContact {
                    username: None,
                    inbox_id: Some(form.value.trim().to_string()),
                    display_name,
                    identity_key: None,
                },
            )?
        };
        self.storage.ensure_conversation(profile.id, contact.id)?;
        self.reload_conversations()?;
        self.status = format!("Contact '{}' added.", contact.display_name);
        self.screen = Screen::Main;
        Ok(())
    }

    fn activate_profile(&mut self, profile_id: i64) -> Result<()> {
        let profile = self.storage.get_profile(profile_id)?;
        let server = self.storage.get_server(profile.server_id)?;
        self.active_profile = Some(profile.clone());
        self.active_server = Some(server.clone());
        self.screen = Screen::Main;
        self.input_mode = InputMode::Command;
        self.composer.clear();
        self.reload_conversations()?;
        self.replenish_prekeys_if_needed()?;
        self.sync.start(server, profile);
        self.sync.trigger();
        self.status = "Profile activated.".to_string();
        Ok(())
    }

    fn refresh_login_data(&mut self) -> Result<()> {
        self.servers = self.storage.list_servers()?;
        if self.servers.is_empty() {
            self.selected_server = 0;
            self.profiles.clear();
            self.selected_profile = 0;
            return Ok(());
        }
        self.selected_server = self
            .selected_server
            .min(self.servers.len().saturating_sub(1));
        self.refresh_profiles()
    }

    fn refresh_profiles(&mut self) -> Result<()> {
        if let Some(server) = self.selected_server_record() {
            self.profiles = self.storage.list_profiles_by_server(server.id)?;
            self.selected_profile = self
                .selected_profile
                .min(self.profiles.len().saturating_sub(1));
        } else {
            self.profiles.clear();
            self.selected_profile = 0;
        }
        Ok(())
    }

    fn reload_conversations(&mut self) -> Result<()> {
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        self.conversations = self.storage.list_conversations(profile.id)?;
        self.selected_conversation = self
            .selected_conversation
            .min(self.conversations.len().saturating_sub(1));
        self.reload_messages()
    }

    fn reload_messages(&mut self) -> Result<()> {
        let was_auto_follow = self.message_auto_follow;
        self.messages.clear();
        if let Some(conversation_id) = self
            .current_conversation()
            .map(|(conversation, _)| conversation.id)
        {
            self.unread_conversations.remove(&conversation_id);
            self.messages = self.storage.get_messages(conversation_id)?;
        }
        if was_auto_follow {
            self.message_scroll = u16::MAX;
        }
        self.normalize_message_scroll();
        Ok(())
    }

    fn send_current_message(&mut self) -> Result<()> {
        if self.composer.trim().is_empty() {
            return Ok(());
        }
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        let server = self
            .active_server
            .clone()
            .ok_or_else(|| anyhow!("no active server"))?;
        let (conversation, contact) = self
            .current_conversation()
            .cloned()
            .ok_or_else(|| anyhow!("select a conversation"))?;
        let relay = RelayClient::new(server.base_url.clone())?;
        let plaintext = self.composer.trim().to_string();

        let prepared = if let Some(session) = conversation.session.clone() {
            let mut session = session;
            if session.pending_send_ratchet {
                rotate_local_ratchet(&mut session)?;
            }
            prepare_session_message(&session, &profile.username, &profile.inbox_id, &plaintext)?
        } else {
            let username = contact.username.clone().ok_or_else(|| {
                anyhow!("this contact has no username; bootstrap requires bundle discovery")
            })?;
            let bundle = relay.get_bundle(&username)?;
            let prepared = prepare_initial_message(
                &profile.username,
                &profile.inbox_id,
                &profile.keys,
                &bundle,
                &plaintext,
            )?;
            let identity = parse_identity_json(&bundle.identity_key)?;
            self.storage.upsert_contact(
                profile.id,
                &UpsertContact {
                    username: Some(bundle.username.clone()),
                    inbox_id: Some(bundle.inbox_id.clone()),
                    display_name: contact.display_name.clone(),
                    identity_key: Some(serde_json::to_string(&identity)?),
                },
            )?;
            prepared
        };

        let response = relay.post_message(&PostMessageRequest {
            inbox_id: prepared.relay_inbox_id.clone(),
            kind: prepared.relay_kind.clone(),
            header: prepared.header_json.clone(),
            ciphertext: prepared.ciphertext.clone(),
            used_prekey_id: prepared.used_prekey_id.clone(),
            client_message_id: prepared.client_message_id.clone(),
        })?;

        self.storage
            .set_session(conversation.id, &prepared.session)?;
        self.storage.insert_message(
            conversation.id,
            &NewMessage {
                relay_message_id: Some(response.id),
                client_message_id: prepared.client_message_id,
                direction: MessageDirection::Outgoing,
                body: Some(plaintext),
                header: prepared.header_json,
                ciphertext: prepared.ciphertext,
                relay_kind: prepared.relay_kind,
                created_at: response.created_at,
                status: MessageStatus::Sent,
                error_reason: None,
            },
        )?;

        self.composer.clear();
        self.reload_conversations()?;
        self.sync.trigger();
        self.status = format!("Message sent. Expires at {}.", response.expires_at);
        Ok(())
    }

    fn consume_sync_events(&mut self) {
        let active_profile = self.active_profile.as_ref().map(|profile| profile.id);
        for event in self.sync.drain() {
            match event {
                SyncEvent::Messages { profile_id, items } if Some(profile_id) == active_profile => {
                    if let Err(error) = self.ingest_messages(items) {
                        self.show_sync_error_toast(error.to_string(), ToastMode::AutoDismiss);
                    }
                }
                SyncEvent::Status {
                    profile_id,
                    message,
                } if Some(profile_id) == active_profile => {
                    self.status = message;
                }
                SyncEvent::Error {
                    profile_id,
                    message,
                } if Some(profile_id) == active_profile => {
                    self.show_sync_error_toast(message, ToastMode::AutoDismiss);
                }
                _ => {}
            }
        }
    }

    fn ingest_messages(&mut self, items: Vec<crate::relay::RelayMessage>) -> Result<()> {
        self.last_poll_count = items.len();
        let mut inserted = 0usize;
        let mut unresolved = 0usize;
        for item in items {
            if self.storage.has_relay_message(&item.id)? {
                continue;
            }
            match self.ingest_message(item)? {
                IngestOutcome::Stored { conversation_id } => {
                    inserted += 1;
                    self.unread_conversations.insert(conversation_id);
                }
                IngestOutcome::Unresolved {
                    conversation_id,
                    reason,
                } => {
                    unresolved += 1;
                    self.unread_conversations.insert(conversation_id);
                    self.last_receive_error = Some(reason);
                }
                IngestOutcome::Ignored => {}
            }
        }
        self.last_ingest_stored = inserted;
        self.last_ingest_unresolved = unresolved;
        self.reload_conversations()?;
        self.replenish_prekeys_if_needed()?;
        if inserted > 0 || unresolved > 0 {
            self.status = format!(
                "sync ok: stored={inserted} unresolved={unresolved} from poll={}",
                self.last_poll_count
            );
        }
        Ok(())
    }

    fn ingest_message(&mut self, item: crate::relay::RelayMessage) -> Result<IngestOutcome> {
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        let header: MessageHeader = match serde_json::from_str(&item.header) {
            Ok(header) => header,
            Err(error) => {
                let conversation_id = self.store_unresolved_message(
                    &profile,
                    "Relay Diagnostics",
                    None,
                    None,
                    &item,
                    MessageStatus::UnsupportedHeader,
                    format!("header parse failed: {error}"),
                )?;
                return Ok(IngestOutcome::Unresolved {
                    conversation_id,
                    reason: "header parse failed".to_string(),
                });
            }
        };

        match header.message_type.as_str() {
            "prekey_message" => {
                let mut profile_with_keys = profile.clone();
                let decrypted = match receive_prekey_message(
                    &mut profile_with_keys.keys,
                    &profile.username,
                    &profile.inbox_id,
                    &item.header,
                    &item.ciphertext,
                ) {
                    Ok(decrypted) => decrypted,
                    Err(error) => {
                        let conversation_id = self.store_unresolved_message(
                            &profile,
                            &header.sender_username,
                            Some(header.sender_username.clone()),
                            Some(header.sender_inbox_id.clone()),
                            &item,
                            MessageStatus::DecryptFailed,
                            format!("prekey decrypt failed: {error}"),
                        )?;
                        return Ok(IngestOutcome::Unresolved {
                            conversation_id,
                            reason: "prekey decrypt failed".to_string(),
                        });
                    }
                };
                let mut session = decrypted.session;
                session.session_id = header.session_id.clone();
                session.peer_username = Some(header.sender_username.clone());
                session.peer_inbox_id = header.sender_inbox_id.clone();

                self.storage
                    .update_profile_keys(profile.id, &profile_with_keys.keys)?;
                self.active_profile = Some(profile_with_keys.clone());

                let contact = self.storage.upsert_contact(
                    profile.id,
                    &UpsertContact {
                        username: Some(header.sender_username.clone()),
                        inbox_id: Some(header.sender_inbox_id.clone()),
                        display_name: header.sender_username.clone(),
                        identity_key: Some(decrypted.sender_identity_json),
                    },
                )?;
                let conversation = self.storage.ensure_conversation(profile.id, contact.id)?;
                self.storage.set_session(conversation.id, &session)?;
                self.storage.insert_message(
                    conversation.id,
                    &NewMessage {
                        relay_message_id: Some(item.id),
                        client_message_id: item.client_message_id,
                        direction: MessageDirection::Incoming,
                        body: Some(decrypted.plaintext),
                        header: item.header,
                        ciphertext: item.ciphertext,
                        relay_kind: item.kind,
                        created_at: item.created_at,
                        status: MessageStatus::Received,
                        error_reason: None,
                    },
                )?;
                return Ok(IngestOutcome::Stored {
                    conversation_id: conversation.id,
                });
            }
            "ratchet_message" => {
                let conversations = self.storage.list_conversations(profile.id)?;
                let Some((conversation, _)) = conversations
                    .iter()
                    .find(|(conversation, contact)| {
                        conversation
                            .session
                            .as_ref()
                            .map(|session| session.session_id == header.session_id)
                            .unwrap_or(false)
                            || contact
                                .inbox_id
                                .as_ref()
                                .map(|inbox_id| inbox_id == &header.sender_inbox_id)
                                .unwrap_or(false)
                    })
                    .cloned()
                else {
                    let conversation_id = self.store_unresolved_message(
                        &profile,
                        &header.sender_username,
                        Some(header.sender_username.clone()),
                        Some(header.sender_inbox_id.clone()),
                        &item,
                        MessageStatus::SessionMissing,
                        format!(
                            "session missing for ratchet {} / {}",
                            header.session_id, header.sender_inbox_id
                        ),
                    )?;
                    return Ok(IngestOutcome::Unresolved {
                        conversation_id,
                        reason: "session missing".to_string(),
                    });
                };
                let session = conversation
                    .session
                    .clone()
                    .ok_or_else(|| anyhow!("missing session for ratchet message"))?;
                let decrypted =
                    match receive_session_message(&session, &item.header, &item.ciphertext) {
                        Ok(decrypted) => decrypted,
                        Err(error) => {
                            let conversation_id = self.store_unresolved_message(
                                &profile,
                                &header.sender_username,
                                Some(header.sender_username.clone()),
                                Some(header.sender_inbox_id.clone()),
                                &item,
                                MessageStatus::DecryptFailed,
                                format!("ratchet decrypt failed: {error}"),
                            )?;
                            return Ok(IngestOutcome::Unresolved {
                                conversation_id,
                                reason: "ratchet decrypt failed".to_string(),
                            });
                        }
                    };
                self.storage
                    .set_session(conversation.id, &decrypted.session)?;
                self.storage.insert_message(
                    conversation.id,
                    &NewMessage {
                        relay_message_id: Some(item.id),
                        client_message_id: item.client_message_id,
                        direction: MessageDirection::Incoming,
                        body: Some(decrypted.plaintext),
                        header: item.header,
                        ciphertext: item.ciphertext,
                        relay_kind: item.kind,
                        created_at: item.created_at,
                        status: MessageStatus::Received,
                        error_reason: None,
                    },
                )?;
                return Ok(IngestOutcome::Stored {
                    conversation_id: conversation.id,
                });
            }
            _ => {
                let conversation_id = self.store_unresolved_message(
                    &profile,
                    "Relay Diagnostics",
                    Some(header.sender_username),
                    Some(header.sender_inbox_id),
                    &item,
                    MessageStatus::UnsupportedHeader,
                    format!("unsupported message type: {}", header.message_type),
                )?;
                return Ok(IngestOutcome::Unresolved {
                    conversation_id,
                    reason: "unsupported message type".to_string(),
                });
            }
        }
    }

    fn replenish_prekeys_if_needed(&mut self) -> Result<()> {
        let Some(profile) = self.active_profile.clone() else {
            return Ok(());
        };
        if available_prekeys(&profile.keys) >= 5 {
            return Ok(());
        }
        let Some(server) = self.active_server.clone() else {
            return Ok(());
        };
        let relay = RelayClient::new(server.base_url.clone())?;
        let mut profile = profile;
        let new_prekeys = generate_more_prekeys(12);
        let public_keys: Vec<String> = new_prekeys
            .iter()
            .map(|prekey| prekey.public_key.clone())
            .collect();
        relay.add_prekeys(&profile.username, &public_keys)?;
        profile.keys.one_time_prekeys.extend(new_prekeys);
        self.storage
            .update_profile_keys(profile.id, &profile.keys)?;
        self.active_profile = Some(profile);
        self.status = "Uploaded more one-time prekeys.".to_string();
        Ok(())
    }

    fn store_unresolved_message(
        &mut self,
        profile: &LocalProfileRecord,
        display_name: &str,
        username: Option<String>,
        inbox_id: Option<String>,
        item: &crate::relay::RelayMessage,
        status: MessageStatus,
        reason: String,
    ) -> Result<i64> {
        let contact = self.storage.upsert_contact(
            profile.id,
            &UpsertContact {
                username,
                inbox_id,
                display_name: display_name.to_string(),
                identity_key: None,
            },
        )?;
        let conversation = self.storage.ensure_conversation(profile.id, contact.id)?;
        self.storage.insert_message(
            conversation.id,
            &NewMessage {
                relay_message_id: Some(item.id.clone()),
                client_message_id: item.client_message_id.clone(),
                direction: MessageDirection::Incoming,
                body: None,
                header: item.header.clone(),
                ciphertext: item.ciphertext.clone(),
                relay_kind: item.kind.clone(),
                created_at: item.created_at.clone(),
                status,
                error_reason: Some(reason),
            },
        )?;
        Ok(conversation.id)
    }

    fn selected_server_record(&self) -> Option<&ServerRecord> {
        self.servers.get(self.selected_server)
    }

    fn show_api_error_toast(&mut self, message: String, mode: ToastMode) {
        self.status = message.clone();
        self.toast = Some(ToastState {
            kind: ToastKind::ApiError,
            message: friendly_error_message(&message),
            mode,
            expires_at: toast_expiration(mode),
        });
    }

    fn show_sync_error_toast(&mut self, message: String, mode: ToastMode) {
        self.status = message.clone();
        self.toast = Some(ToastState {
            kind: ToastKind::SyncError,
            message: friendly_error_message(&message),
            mode,
            expires_at: toast_expiration(mode),
        });
    }

    fn dismiss_toast(&mut self) -> bool {
        if self.toast.is_some() {
            self.toast = None;
            return true;
        }
        false
    }

    fn expire_toast_if_needed(&mut self) {
        let should_clear = self
            .toast
            .as_ref()
            .and_then(|toast| toast.expires_at)
            .map(|expires_at| Instant::now() >= expires_at)
            .unwrap_or(false);
        if should_clear {
            self.toast = None;
        }
    }

    fn message_lines(&self) -> Vec<Line<'static>> {
        if self.messages.is_empty() {
            return vec![Line::from("No messages yet")];
        }

        self.messages
            .iter()
            .map(|message| {
                let incoming = match message.direction {
                    MessageDirection::Incoming => true,
                    MessageDirection::Outgoing => false,
                };
                let prefix = if incoming { "<" } else { ">" };
                let body = message.body.as_deref().unwrap_or("<raw envelope>");
                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", message.created_at),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        prefix,
                        if incoming {
                            Style::default().fg(Color::Blue)
                        } else {
                            Style::default().fg(Color::LightGreen)
                        },
                    ),
                    Span::raw(format!(" {}", body)),
                    Span::styled(
                        message
                            .error_reason
                            .as_ref()
                            .map(|error| format!(" ({error})"))
                            .unwrap_or_default(),
                        Style::default().fg(Color::Yellow),
                    ),
                ])
            })
            .collect()
    }

    fn max_message_scroll(&self, area: Rect, message_lines: &[Line<'static>]) -> u16 {
        let visible_height = message_viewport_height(area);
        if visible_height == 0 {
            return 0;
        }

        let total_height = message_lines
            .iter()
            .map(|line| wrapped_line_height(line, area.width.saturating_sub(2)))
            .sum::<u16>();

        total_height.saturating_sub(visible_height)
    }

    fn scroll_messages_page(&mut self, direction: i32) {
        let page = message_viewport_height(self.current_message_area()).max(1);
        if direction < 0 {
            self.message_auto_follow = false;
            self.message_scroll = self.message_scroll.saturating_sub(page);
        } else {
            self.message_scroll = self.message_scroll.saturating_add(page);
        }
        self.normalize_message_scroll();
    }

    fn scroll_messages_to_top(&mut self) {
        self.message_auto_follow = false;
        self.message_scroll = 0;
    }

    fn scroll_messages_to_bottom(&mut self) {
        self.message_auto_follow = true;
        self.message_scroll = u16::MAX;
        self.normalize_message_scroll();
    }

    fn normalize_message_scroll(&mut self) {
        let area = self.current_message_area();
        let lines = self.message_lines();
        let max_scroll = self.max_message_scroll(area, &lines);

        if self.message_auto_follow || self.message_scroll >= max_scroll {
            self.message_auto_follow = true;
            self.message_scroll = max_scroll;
        } else {
            self.message_scroll = self.message_scroll.min(max_scroll);
        }
    }

    fn current_message_area(&self) -> Rect {
        let Ok((width, height)) = crossterm::terminal::size() else {
            return Rect::default();
        };

        let area = Rect::new(0, 0, width, height);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),
                Constraint::Length(4),
                Constraint::Length(3),
            ])
            .split(area);
        let constraints = if self.show_technical {
            vec![
                Constraint::Percentage(24),
                Constraint::Percentage(48),
                Constraint::Percentage(28),
            ]
        } else {
            vec![Constraint::Percentage(28), Constraint::Percentage(72)]
        };
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(vertical[0]);

        horizontal.get(1).copied().unwrap_or_default()
    }

    fn selected_profile_record(&self) -> Option<&LocalProfileRecord> {
        self.profiles.get(self.selected_profile)
    }

    fn current_conversation(&self) -> Option<&(ConversationRecord, ContactRecord)> {
        self.conversations.get(self.selected_conversation)
    }

    fn focus_style(&self, focus: LoginFocus) -> Style {
        if self.login_focus == focus {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }
    }
}

fn shorten(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let shortened = value.chars().take(max).collect::<String>();
    format!("{shortened}...")
}

fn message_viewport_height(area: Rect) -> u16 {
    area.height.saturating_sub(2)
}

fn wrapped_line_height(line: &Line<'_>, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }

    let visual_width = line.width().max(1);
    let width = usize::from(width);
    visual_width.div_ceil(width) as u16
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn selection_style() -> Style {
    Style::default()
        .bg(Color::Rgb(28, 32, 38))
        .fg(Color::Rgb(232, 236, 241))
        .add_modifier(Modifier::BOLD)
}

fn toast_border_style() -> Style {
    Style::default()
        .bg(Color::Rgb(19, 22, 28))
        .fg(Color::Rgb(214, 92, 92))
        .add_modifier(Modifier::BOLD)
}

fn toast_body_style() -> Style {
    Style::default()
        .bg(Color::Rgb(19, 22, 28))
        .fg(Color::Rgb(236, 239, 243))
}

fn toast_expiration(mode: ToastMode) -> Option<Instant> {
    match mode {
        ToastMode::AutoDismiss => Some(Instant::now() + Duration::from_secs(5)),
        ToastMode::Sticky => None,
    }
}

fn friendly_error_message(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("already exists") || lower.contains("username already exists") {
        return "Username already exists on this relay.".to_string();
    }
    if lower.contains("failed to call relay health endpoint") {
        return "Could not reach the relay health endpoint.".to_string();
    }
    if lower.contains("failed to call post /users") {
        return "Profile registration request failed.".to_string();
    }
    if lower.contains("failed to fetch bundle") {
        return "Could not fetch the contact bundle from the relay.".to_string();
    }
    if lower.contains("failed to call post /messages") {
        return "Message send request failed.".to_string();
    }
    if lower.contains("poll failed:") {
        return message.replacen("poll failed:", "Relay polling failed:", 1);
    }
    message.to_string()
}
