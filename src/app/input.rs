use super::*;

impl App {
    pub(crate) fn handle_event(&mut self, event: Event) -> Result<()> {
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
}
