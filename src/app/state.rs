use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginFocus {
    Servers,
    Profiles,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputMode {
    Command,
    Compose,
}

pub(crate) enum Screen {
    Login,
    AddServer(FormState),
    AddProfile(FormState),
    AddContact(ContactFormState),
    Main,
}

pub(crate) enum IngestOutcome {
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
pub(crate) enum ToastMode {
    AutoDismiss,
    Sticky,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastKind {
    ApiError,
    SyncError,
}

pub(crate) struct ToastState {
    pub(crate) kind: ToastKind,
    pub(crate) message: String,
    pub(crate) mode: ToastMode,
    pub(crate) expires_at: Option<Instant>,
}

pub(crate) struct FormState {
    pub(crate) title: &'static str,
    pub(crate) fields: Vec<FormField>,
    pub(crate) index: usize,
}

pub(crate) struct FormField {
    pub(crate) label: &'static str,
    pub(crate) value: String,
}

pub(crate) struct ContactFormState {
    pub(crate) display_name: String,
    pub(crate) value: String,
    pub(crate) use_username: bool,
    pub(crate) field_index: usize,
}

impl FormState {
    pub(crate) fn new(title: &'static str, labels: &[&'static str]) -> Self {
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

    pub(crate) fn current_mut(&mut self) -> &mut String {
        &mut self.fields[self.index].value
    }
}
