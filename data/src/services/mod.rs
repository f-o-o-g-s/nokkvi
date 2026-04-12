//! Backend services — API clients, persistence, and domain logic
//!
//! Includes Navidrome API client (`api/`), auth, queue management (redb persistence),
//! playback navigation (QueueNavigator with playback history), settings management,
//! artwork prefetch, task management, and state storage.

pub mod api;
pub mod artwork_prefetch;
pub mod auth;
pub mod font_discovery;
pub mod navidrome_events;
pub mod playback;
pub mod queue;
pub mod settings;
pub mod state_storage;
pub mod task_manager;
pub mod theme_loader;
pub mod toml_settings_io;
