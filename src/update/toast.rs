//! Toast notification update handler

use iced::Task;

use crate::{
    Nokkvi,
    app_message::{Message, ToastMessage},
};

impl Nokkvi {
    pub(crate) fn handle_toast(&mut self, msg: ToastMessage) -> Task<Message> {
        match msg {
            ToastMessage::Push(toast) => {
                tracing::debug!(
                    "🔔 Toast: [{}] {}",
                    toast_level_label(toast.level),
                    toast.message
                );
                self.toast.push(toast);
                Task::none()
            }
            ToastMessage::PushThen(toast, next) => {
                tracing::debug!(
                    "🔔 Toast: [{}] {}",
                    toast_level_label(toast.level),
                    toast.message
                );
                self.toast.push(toast);
                Task::done(*next)
            }
            ToastMessage::Dismiss => {
                self.toast.toasts.pop_back();
                Task::none()
            }
            ToastMessage::DismissKey(key) => {
                self.toast.dismiss_key(&key);
                Task::none()
            }
        }
    }
}

fn toast_level_label(level: nokkvi_data::types::toast::ToastLevel) -> &'static str {
    use nokkvi_data::types::toast::ToastLevel;
    match level {
        ToastLevel::Info => "INFO",
        ToastLevel::Success => "SUCCESS",
        ToastLevel::Warning => "WARN",
        ToastLevel::Error => "ERROR",
    }
}
