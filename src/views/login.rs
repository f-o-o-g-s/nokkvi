//! Login Page Component
//!
//! Self-contained login screen with form inputs and authentication.
//! Uses message bubbling pattern to communicate login actions to root.

use iced::{
    Alignment, Element, Length, Task,
    event::Event,
    keyboard,
    keyboard::key,
    widget::{button, column, container, operation, space, text, text_input},
};

use crate::theme;

// ============================================================================
// Login State
// ============================================================================

/// Login page local state
#[derive(Debug, Clone)]
pub struct LoginPage {
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub login_in_progress: bool,
    pub error_message: Option<String>,
}

impl Default for LoginPage {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:4533".to_string(),
            username: String::new(),
            password: String::new(),
            login_in_progress: false,
            error_message: None,
        }
    }
}

impl LoginPage {
    /// Initialize with pre-filled credentials (e.g., from saved auth)
    pub fn with_credentials(server_url: String, username: String, password: String) -> Self {
        Self {
            server_url,
            username,
            password,
            login_in_progress: false,
            error_message: None,
        }
    }
}

// ============================================================================
// Messages
// ============================================================================

/// Messages for login page interactions
#[derive(Debug, Clone)]
pub enum LoginMessage {
    ServerUrlChanged(String),
    UsernameChanged(String),
    PasswordChanged(String),
    LoginPressed,
    Event(Event),
}

// ============================================================================
// Actions
// ============================================================================

/// Actions that bubble up to root for authentication processing
#[derive(Debug, Clone)]
pub enum LoginAction {
    /// Request authentication with provided credentials
    AttemptLogin {
        server_url: String,
        username: String,
        password: String,
    },
    None,
}

// ============================================================================
// Update
// ============================================================================

impl LoginPage {
    /// Update internal state and return actions for root
    pub fn update(&mut self, message: LoginMessage) -> (Task<LoginMessage>, LoginAction) {
        match message {
            LoginMessage::ServerUrlChanged(url) => {
                self.server_url = url;
                (Task::none(), LoginAction::None)
            }
            LoginMessage::UsernameChanged(name) => {
                self.username = name;
                (Task::none(), LoginAction::None)
            }
            LoginMessage::PasswordChanged(pass) => {
                self.password = pass;
                (Task::none(), LoginAction::None)
            }
            LoginMessage::LoginPressed => {
                // Validate fields before attempting login
                if self.server_url.trim().is_empty() {
                    self.error_message = Some("Server URL is required".to_string());
                    return (Task::none(), LoginAction::None);
                }
                if self.username.trim().is_empty() {
                    self.error_message = Some("Username is required".to_string());
                    return (Task::none(), LoginAction::None);
                }
                if self.password.is_empty() {
                    self.error_message = Some("Password is required".to_string());
                    return (Task::none(), LoginAction::None);
                }

                self.login_in_progress = true;
                self.error_message = None;
                (
                    Task::none(),
                    LoginAction::AttemptLogin {
                        server_url: self.server_url.clone(),
                        username: self.username.clone(),
                        password: self.password.clone(),
                    },
                )
            }
            LoginMessage::Event(Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Tab),
                modifiers,
                ..
            })) => {
                let shift = modifiers.shift();
                (
                    if shift {
                        operation::focus_previous()
                    } else {
                        operation::focus_next()
                    },
                    LoginAction::None,
                )
            }
            LoginMessage::Event(_) => (Task::none(), LoginAction::None),
        }
    }

    /// Called by root when login succeeds
    pub fn on_login_success(&mut self) {
        self.login_in_progress = false;
        self.error_message = None;
    }

    /// Called by root when login fails
    pub fn on_login_error(&mut self, error: String) {
        self.login_in_progress = false;
        self.error_message = Some(error);
    }
}

// ============================================================================
// View
// ============================================================================

impl LoginPage {
    /// Build the login view
    pub fn view(&self) -> Element<'_, LoginMessage> {
        let logo_svg = crate::embedded_svg::themed_logo_svg();
        let logo_handle = iced::widget::svg::Handle::from_memory(logo_svg.into_bytes());
        let logo = iced::widget::svg(logo_handle)
            .width(Length::Fixed(80.0))
            .height(Length::Fixed(80.0));

        let title = text("Nokkvi")
            .size(42)
            .color(theme::fg0())
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .font(iced::font::Font {
                weight: iced::font::Weight::Bold,
                ..theme::ui_font()
            });

        let subtitle = text("A sturdy hull for the endless stream.")
            .size(14)
            .font(iced::font::Font {
                style: iced::font::Style::Italic,
                ..theme::ui_font()
            })
            .color(theme::fg3())
            .width(Length::Fill)
            .align_x(Alignment::Center);

        let header = column![
            container(logo).width(Length::Fill).center_x(Length::Fill),
            title,
            subtitle
        ]
        .spacing(8)
        .align_x(Alignment::Center);

        let input_width = Length::Fill;

        let content = column![
            header,
            space().height(24),
            column![
                text("Server URL").size(14).color(theme::fg1()),
                text_input("http://navidrome.local:4533", &self.server_url)
                    .on_input(LoginMessage::ServerUrlChanged)
                    .padding(12)
                    .width(input_width)
                    .font(theme::ui_font())
                    .style(|_theme, status| {
                        let border_color = match status {
                            text_input::Status::Focused { .. } => theme::accent(),
                            _ => theme::bg3(),
                        };
                        text_input::Style {
                            background: (theme::bg0_hard()).into(),
                            border: iced::Border {
                                color: border_color,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            icon: theme::fg1(),
                            placeholder: theme::fg4(),
                            value: theme::fg0(),
                            selection: theme::selection_color(),
                        }
                    }),
            ]
            .spacing(5),
            column![
                text("Username").size(14).color(theme::fg1()),
                text_input("Username", &self.username)
                    .on_input(LoginMessage::UsernameChanged)
                    .padding(12)
                    .width(input_width)
                    .font(theme::ui_font())
                    .style(|_theme, status| {
                        let border_color = match status {
                            text_input::Status::Focused { .. } => theme::accent(),
                            _ => theme::bg3(),
                        };
                        text_input::Style {
                            background: (theme::bg0_hard()).into(),
                            border: iced::Border {
                                color: border_color,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            icon: theme::fg1(),
                            placeholder: theme::fg4(),
                            value: theme::fg0(),
                            selection: theme::selection_color(),
                        }
                    }),
            ]
            .spacing(5),
            column![
                text("Password").size(14).color(theme::fg1()),
                text_input("Password", &self.password)
                    .on_input(LoginMessage::PasswordChanged)
                    .secure(true)
                    .padding(12)
                    .width(input_width)
                    .font(theme::ui_font())
                    .on_submit(LoginMessage::LoginPressed)
                    .style(|_theme, status| {
                        let border_color = match status {
                            text_input::Status::Focused { .. } => theme::accent(),
                            _ => theme::bg3(),
                        };
                        text_input::Style {
                            background: (theme::bg0_hard()).into(),
                            border: iced::Border {
                                color: border_color,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            icon: theme::fg1(),
                            placeholder: theme::fg4(),
                            value: theme::fg0(),
                            selection: theme::selection_color(),
                        }
                    }),
            ]
            .spacing(5),
            if let Some(err) = &self.error_message {
                text(err)
                    .color(theme::danger_bright())
                    .size(14)
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
            } else {
                text(" ") // using empty text instead of nothing to preserve height, although iced handles missing fine
            },
            Element::from(
                crate::widgets::hover_overlay::HoverOverlay::<'_, LoginMessage>::new(
                    button(
                        text(if self.login_in_progress {
                            "Connecting..."
                        } else {
                            "Login"
                        })
                        .width(Length::Fill)
                        .align_x(Alignment::Center)
                    )
                    .on_press(LoginMessage::LoginPressed)
                    .padding(14)
                    .width(input_width)
                    .style(|_theme, _status| {
                        button::Style {
                            background: Some((theme::accent()).into()),
                            text_color: theme::bg0_hard(),
                            border: iced::Border {
                                color: theme::accent_border_light(),
                                width: 1.0,
                                radius: 8.0.into(),
                            },
                            shadow: iced::Shadow::default(),
                            snap: false,
                        }
                    })
                )
                .border_radius(8.0.into())
            ),
        ]
        .spacing(16)
        .width(Length::Fixed(400.0));

        let card = container(content)
            .padding(40)
            .style(|_theme| container::Style {
                background: Some((theme::bg0()).into()),
                text_color: Some(theme::fg0()),
                border: iced::Border {
                    color: theme::bg1(),
                    width: 1.0,
                    radius: 12.0.into(),
                },
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.8),
                    offset: iced::Vector::new(0.0, 10.0),
                    blur_radius: 30.0,
                },
                snap: false,
            });

        let version = env!("CARGO_PKG_VERSION");
        let version_text = text(format!("v{version}")).size(12).color(theme::fg4());

        let layout = column![card, version_text]
            .spacing(20)
            .align_x(Alignment::Center);

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_theme| container::Style {
                background: Some((theme::bg0_hard()).into()),
                text_color: Some(theme::fg0()),
                border: iced::Border::default(),
                shadow: iced::Shadow::default(),
                snap: false,
            })
            .into()
    }
}
