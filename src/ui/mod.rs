mod branding;
mod components;
mod login;
mod main;
mod overlays;

use ratatui::Frame;

use crate::app::{App, Screen};

pub(crate) use components::{
    centered_rect, message_viewport_height, selection_style, toast_body_style, toast_border_style,
    wrapped_line_height,
};

pub(crate) fn render_app(app: &App, frame: &mut Frame<'_>) {
    match &app.screen {
        Screen::Login => login::render_login(app, frame),
        Screen::AddServer(form) => {
            login::render_login(app, frame);
            overlays::render_form_overlay(frame, form);
        }
        Screen::AddProfile(form) => {
            login::render_login(app, frame);
            overlays::render_form_overlay(frame, form);
        }
        Screen::AddContact(form) => {
            main::render_main(app, frame);
            overlays::render_contact_overlay(frame, form);
        }
        Screen::Main => main::render_main(app, frame),
    }
    overlays::render_toast(app, frame);
}
