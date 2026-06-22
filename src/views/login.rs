//! Login Page Component
//!
//! Self-contained login screen with form inputs and authentication.
//! Uses message bubbling pattern to communicate login actions to root.
//!
//! The layout is responsive (mirroring the rest of the app): a centered card
//! that reflows between a two-pane "branding + form" arrangement on wide windows
//! and a single stacked column on narrow ones — directly analogous to how the
//! slot-list artwork column appears/hides by width. The whole card lives inside
//! a `scrollable`, so a window that is too short to fit it scrolls instead of
//! culling the Login button (important under tiling compositors like Hyprland,
//! which force windows below any min-size the app requests).

use iced::{
    Alignment, Element, Length, Task,
    event::Event,
    keyboard,
    keyboard::key,
    widget::{
        column, container, mouse_area, operation, responsive, row, scrollable, text, text_input,
    },
};

use crate::theme;

// ============================================================================
// Layout constants
// ============================================================================

/// Stable `text_input` ids — let the form auto-focus the first empty field and
/// keep focus traversal deterministic across reflows.
const LOGIN_SERVER_INPUT_ID: &str = "login_server_input";
const LOGIN_USERNAME_INPUT_ID: &str = "login_username_input";
const LOGIN_PASSWORD_INPUT_ID: &str = "login_password_input";

/// Width at/above which the card switches to the two-pane (branding | form)
/// layout. Below it, the card stacks to a single column.
const LOGIN_TWO_PANE_MIN_WIDTH: f32 = 720.0;
/// Width cap for the single-column card; it shrinks below this on narrow
/// windows but never grows past it on wide ones.
const LOGIN_CARD_MAX_WIDTH: f32 = 420.0;
/// Floor for the single-column card width on very narrow windows. Kept low so
/// the card keeps shrinking (rather than overflowing the viewport) when a
/// tiling compositor forces a very narrow window — the scrollable is
/// vertical-only, so a too-wide card would clip horizontally.
const LOGIN_CARD_MIN_WIDTH: f32 = 120.0;
/// Width cap for the wider two-pane card.
const LOGIN_TWO_PANE_MAX_WIDTH: f32 = 760.0;
/// Breathing room between the card and the window edges.
const LOGIN_PAGE_PAD: f32 = 24.0;
/// Viewport heights at/above which the card is dead-centered vertically; below
/// these it is top-aligned inside the scrollable so nothing clips. Set
/// generously above the tallest the card can be (error line + cleartext warning
/// both shown) so we bias toward the scroll path rather than risk clipping the
/// centered (non-scrolling) card.
const LOGIN_FIT_HEIGHT_SINGLE: f32 = 760.0;
const LOGIN_FIT_HEIGHT_TWO_PANE: f32 = 520.0;

// ============================================================================
// Login State
// ============================================================================

/// Which field an error should highlight. `Credentials` highlights both the
/// username and password rows (a 401 can't tell which one was wrong).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginField {
    ServerUrl,
    Username,
    Password,
    Credentials,
}

/// A user-facing login error: the message to show plus the field to highlight.
#[derive(Debug, Clone)]
pub struct LoginError {
    pub message: String,
    pub field: Option<LoginField>,
}

impl LoginError {
    fn field(message: impl Into<String>, field: LoginField) -> Self {
        Self {
            message: message.into(),
            field: Some(field),
        }
    }

    /// Whether the given field should render with the error (danger) border.
    fn highlights(&self, field: LoginField) -> bool {
        match self.field {
            Some(LoginField::Credentials) => {
                matches!(field, LoginField::Username | LoginField::Password)
            }
            Some(f) => f == field,
            None => false,
        }
    }
}

/// Login page local state
#[derive(Debug, Clone)]
pub struct LoginPage {
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub login_in_progress: bool,
    pub error: Option<LoginError>,
}

impl Default for LoginPage {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:4533".to_string(),
            username: String::new(),
            password: String::new(),
            login_in_progress: false,
            error: None,
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
            error: None,
        }
    }

    /// The id of the first empty field, so the form can auto-focus on mount.
    fn first_focus_id(&self) -> &'static str {
        if self.server_url.trim().is_empty() {
            LOGIN_SERVER_INPUT_ID
        } else if self.username.trim().is_empty() {
            LOGIN_USERNAME_INPUT_ID
        } else {
            LOGIN_PASSWORD_INPUT_ID
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
    /// Focus the first empty field (dispatched when the login screen mounts).
    FocusFirstField,
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
// Error classification
// ============================================================================

/// Map a raw login-failure string into a friendly message + the field to
/// blame. Keyed primarily on the auth service's own context strings (stable,
/// nokkvi-authored), with reqwest-internal markers as secondary fallbacks.
fn classify_login_error(raw: &str) -> LoginError {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("invalid username or password") || lower.contains("unauthorized") {
        LoginError::field(
            "Wrong username or password. Please try again.",
            LoginField::Credentials,
        )
    } else if lower.contains("invalid server response")
        || lower.contains("missing required authentication")
    {
        LoginError::field(
            "That doesn't look like a Navidrome server. Check the URL.",
            LoginField::ServerUrl,
        )
    } else if lower.contains("network error")
        || lower.contains("check your server url")
        || lower.contains("error sending request")
        || lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("timed out")
        || lower.contains("timeout")
    {
        LoginError::field(
            "Can't reach the server. Check the URL and that it's running.",
            LoginField::ServerUrl,
        )
    } else {
        // "Authentication failed (Status: NNN)" and anything unexpected: show
        // the server's own message, stripped of any redundant prefix.
        let cleaned = raw.trim().trim_start_matches("Login failed:").trim();
        LoginError {
            message: if cleaned.is_empty() {
                "Login failed. Please try again.".to_string()
            } else {
                cleaned.to_string()
            },
            field: None,
        }
    }
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
            LoginMessage::FocusFirstField => {
                (operation::focus(self.first_focus_id()), LoginAction::None)
            }
            LoginMessage::LoginPressed => {
                // Guard against double-submit: a second Enter/click while the
                // first attempt is in flight would spawn an overlapping
                // AppService::new()/login() (risky with redb's exclusive lock).
                if self.login_in_progress {
                    return (Task::none(), LoginAction::None);
                }

                // Validate fields before attempting login, highlighting the
                // first offending field.
                if self.server_url.trim().is_empty() {
                    self.error = Some(LoginError::field(
                        "Server URL is required",
                        LoginField::ServerUrl,
                    ));
                    return (operation::focus(LOGIN_SERVER_INPUT_ID), LoginAction::None);
                }
                if self.username.trim().is_empty() {
                    self.error = Some(LoginError::field(
                        "Username is required",
                        LoginField::Username,
                    ));
                    return (operation::focus(LOGIN_USERNAME_INPUT_ID), LoginAction::None);
                }
                if self.password.is_empty() {
                    self.error = Some(LoginError::field(
                        "Password is required",
                        LoginField::Password,
                    ));
                    return (operation::focus(LOGIN_PASSWORD_INPUT_ID), LoginAction::None);
                }

                self.login_in_progress = true;
                self.error = None;
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
        self.error = None;
    }

    /// Called by root when login fails
    pub fn on_login_error(&mut self, error: String) {
        self.login_in_progress = false;
        self.error = Some(classify_login_error(&error));
    }
}

// ============================================================================
// View
// ============================================================================

/// Border/styling for a login text input, turning the border danger-red when
/// the field is flagged by the current error.
fn login_input_appearance(status: text_input::Status, error: bool) -> text_input::Style {
    let border_color = if error {
        theme::danger_bright()
    } else if matches!(status, text_input::Status::Focused { .. }) {
        theme::accent()
    } else {
        theme::border()
    };
    text_input::Style {
        background: theme::bg0_hard().into(),
        border: iced::Border {
            color: border_color,
            width: 1.0,
            radius: theme::ui_radius_sm(),
        },
        icon: theme::fg1(),
        placeholder: theme::fg4(),
        value: theme::fg0(),
        selection: theme::selection_color(),
    }
}

/// One labelled input row. `on_input` is the message constructor for the field;
/// every field submits on Enter so the user can hit Return from anywhere.
fn input_field<'a>(
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    id: &'static str,
    secure: bool,
    error: bool,
    on_input: fn(String) -> LoginMessage,
) -> iced::widget::Column<'a, LoginMessage> {
    column![
        text(label).size(13).color(theme::fg1()),
        text_input(placeholder, value)
            .id(id)
            .on_input(on_input)
            .on_submit(LoginMessage::LoginPressed)
            .secure(secure)
            .padding(12)
            .width(Length::Fill)
            .font(theme::ui_font())
            .style(move |_theme, status| login_input_appearance(status, error)),
    ]
    .spacing(5)
}

impl LoginPage {
    /// Build the login view
    pub fn view(&self) -> Element<'_, LoginMessage> {
        responsive(move |size| {
            let two_pane = size.width >= LOGIN_TWO_PANE_MIN_WIDTH;
            let card = self.card(two_pane, size.width);

            let fits = if two_pane {
                size.height >= LOGIN_FIT_HEIGHT_TWO_PANE
            } else {
                size.height >= LOGIN_FIT_HEIGHT_SINGLE
            };

            // When the card provably fits, dead-center it both axes. When the
            // window is too short, top-align inside a scrollable so the Login
            // button is always reachable rather than clipped.
            let body: Element<'_, LoginMessage> = if fits {
                container(card)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(LOGIN_PAGE_PAD)
                    .into()
            } else {
                scrollable(
                    container(card)
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(LOGIN_PAGE_PAD),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::settings_scrollable_style)
                .into()
            };

            let page: Element<'_, LoginMessage> = container(body)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme| container::Style {
                    background: Some(theme::bg0_hard().into()),
                    text_color: Some(theme::fg0()),
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: false,
                })
                .into();
            page
        })
        .into()
    }

    /// Branding column: logo, wordmark, tagline. `logo_px` scales the mark for
    /// the two-pane (larger) vs single-column (smaller) arrangement.
    fn branding(&self, logo_px: f32, title_px: f32) -> iced::widget::Column<'_, LoginMessage> {
        let logo_svg = crate::embedded_svg::themed_logo_svg();
        let logo_handle = iced::widget::svg::Handle::from_memory(logo_svg.into_bytes());
        let logo = iced::widget::svg(logo_handle)
            .width(Length::Fixed(logo_px))
            .height(Length::Fixed(logo_px));

        let title = text("Nokkvi")
            .size(title_px)
            .color(theme::fg0())
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .font(theme::weighted_ui_font(iced::font::Weight::Bold));

        let subtitle = text("A sturdy hull for the endless stream.")
            .size(14)
            .font(iced::font::Font {
                style: iced::font::Style::Italic,
                ..theme::ui_font()
            })
            .color(theme::fg3())
            .width(Length::Fill)
            .align_x(Alignment::Center);

        column![
            container(logo).width(Length::Fill).center_x(Length::Fill),
            title,
            subtitle,
        ]
        .spacing(8)
        .align_x(Alignment::Center)
    }

    /// The form column: the three fields, error line, and Login button.
    fn form(&self) -> iced::widget::Column<'_, LoginMessage> {
        let err = self.error.as_ref();
        let highlights = |field: LoginField| err.is_some_and(|e| e.highlights(field));

        // Server URL field, with an optional cleartext-HTTP advisory below it.
        let mut server_block = input_field(
            "Server URL",
            "http://navidrome.local:4533",
            &self.server_url,
            LOGIN_SERVER_INPUT_ID,
            false,
            highlights(LoginField::ServerUrl),
            LoginMessage::ServerUrlChanged,
        );
        if nokkvi_data::utils::server_url::is_cleartext_http_url(&self.server_url) {
            server_block = server_block.push(
                text("Unencrypted connection. Credentials will be sent in clear text.")
                    .size(11)
                    .color(theme::warning())
                    .width(Length::Fill),
            );
        }

        let username_block = input_field(
            "Username",
            "Username",
            &self.username,
            LOGIN_USERNAME_INPUT_ID,
            false,
            highlights(LoginField::Username),
            LoginMessage::UsernameChanged,
        );

        let password_block = input_field(
            "Password",
            "Password",
            &self.password,
            LOGIN_PASSWORD_INPUT_ID,
            true,
            highlights(LoginField::Password),
            LoginMessage::PasswordChanged,
        );

        let mut form = column![server_block, username_block, password_block].spacing(16);

        if let Some(err) = err {
            form = form.push(
                text(&err.message)
                    .color(theme::danger_bright())
                    .size(13)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
            );
        }

        form.push(self.login_button())
    }

    /// The accent-filled Login button (doubles as the "Connecting…" indicator).
    fn login_button(&self) -> Element<'_, LoginMessage> {
        let label = if self.login_in_progress {
            "Connecting..."
        } else {
            "Login"
        };
        mouse_area(
            crate::widgets::hover_overlay::HoverOverlay::<'_, LoginMessage>::new(
                container(text(label).width(Length::Fill).align_x(Alignment::Center))
                    .padding(14)
                    .width(Length::Fill)
                    .style(|_theme| container::Style {
                        background: Some(theme::accent().into()),
                        text_color: Some(theme::bg0_hard()),
                        border: iced::Border {
                            color: theme::accent_border_light(),
                            width: 1.0,
                            radius: theme::ui_radius_sm(),
                        },
                        shadow: iced::Shadow::default(),
                        snap: false,
                    }),
            )
            .border_radius(theme::ui_radius_sm()),
        )
        .on_press(LoginMessage::LoginPressed)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
    }

    /// Version label, centered (shown at the bottom of the branding pane in
    /// two-pane mode and the bottom of the card in single-column mode).
    fn version_label(&self) -> Element<'_, LoginMessage> {
        container(
            text(format!("v{}", env!("CARGO_PKG_VERSION")))
                .size(12)
                .color(theme::fg4()),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    }

    /// The framed card: two-pane (branding | form) on wide windows, a single
    /// stacked column on narrow ones.
    ///
    /// The Fixed card width lives on the OUTER container, but the flex content
    /// (`row!` / `column!`) carries `Length::Fill` so its `Fill` / `FillPortion`
    /// children resolve against a definite width. A `Shrink`-width flex would
    /// compress those children to ~0 intrinsic width (the form fields would
    /// collapse) — see iced's flex layout compression rules.
    fn card(&self, two_pane: bool, avail_width: f32) -> Element<'_, LoginMessage> {
        let (content, card_width): (Element<'_, LoginMessage>, f32) = if two_pane {
            // Branding pane (left): logo / wordmark / tagline / version,
            // vertically centered against the taller form via the row's
            // align_y below.
            let branding_pane = column![self.branding(96.0, 34.0), self.version_label()]
                .width(Length::FillPortion(4))
                .spacing(18)
                .align_x(Alignment::Center);
            let form_pane = container(self.form()).width(Length::FillPortion(5));

            let card_w = (avail_width - 2.0 * LOGIN_PAGE_PAD).min(LOGIN_TWO_PANE_MAX_WIDTH);
            (
                row![branding_pane, form_pane]
                    .width(Length::Fill)
                    .spacing(32)
                    .align_y(Alignment::Center)
                    .into(),
                card_w,
            )
        } else {
            let card_w = (avail_width - 2.0 * LOGIN_PAGE_PAD)
                .clamp(LOGIN_CARD_MIN_WIDTH, LOGIN_CARD_MAX_WIDTH);
            (
                column![self.branding(80.0, 42.0), self.form(), self.version_label()]
                    .width(Length::Fill)
                    .spacing(24)
                    .align_x(Alignment::Center)
                    .into(),
                card_w,
            )
        };

        container(content)
            .padding(36)
            .width(Length::Fixed(card_width))
            .style(|_theme| container::Style {
                background: Some(theme::bg0().into()),
                text_color: Some(theme::fg0()),
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::ui_radius_md(),
                },
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.8),
                    offset: iced::Vector::new(0.0, 10.0),
                    blur_radius: 30.0,
                },
                snap: false,
            })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_wrong_credentials_highlights_both() {
        let err = classify_login_error("Invalid username or password. Please try again.");
        assert_eq!(err.field, Some(LoginField::Credentials));
        assert!(err.highlights(LoginField::Username));
        assert!(err.highlights(LoginField::Password));
        assert!(!err.highlights(LoginField::ServerUrl));
    }

    #[test]
    fn classify_network_blames_server_url() {
        let err = classify_login_error(
            "Network error. Please check your server URL.: error sending request",
        );
        assert_eq!(err.field, Some(LoginField::ServerUrl));
    }

    #[test]
    fn classify_bad_response_blames_server_url() {
        let err = classify_login_error("Invalid server response. Please check your server URL.");
        assert_eq!(err.field, Some(LoginField::ServerUrl));
    }

    #[test]
    fn classify_unknown_preserves_message_without_field() {
        let err = classify_login_error("Authentication failed (Status: 500). Please try again.");
        assert_eq!(err.field, None);
        assert!(err.message.contains("Status: 500"));
    }

    #[test]
    fn double_submit_is_ignored_while_in_progress() {
        let mut page = LoginPage {
            server_url: "http://localhost:4533".into(),
            username: "alice".into(),
            password: "hunter2".into(),
            login_in_progress: true,
            error: None,
        };
        let (_task, action) = page.update(LoginMessage::LoginPressed);
        assert!(matches!(action, LoginAction::None));
    }

    #[test]
    fn first_press_attempts_login() {
        let mut page = LoginPage {
            server_url: "http://localhost:4533".into(),
            username: "alice".into(),
            password: "hunter2".into(),
            login_in_progress: false,
            error: None,
        };
        let (_task, action) = page.update(LoginMessage::LoginPressed);
        assert!(matches!(action, LoginAction::AttemptLogin { .. }));
        assert!(page.login_in_progress);
    }

    #[test]
    fn empty_field_flags_that_field() {
        let mut page = LoginPage {
            server_url: String::new(),
            username: String::new(),
            password: String::new(),
            login_in_progress: false,
            error: None,
        };
        let (_task, _action) = page.update(LoginMessage::LoginPressed);
        assert_eq!(
            page.error.as_ref().and_then(|e| e.field),
            Some(LoginField::ServerUrl)
        );
    }
}
