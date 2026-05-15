//! Navidrome API client — HTTP endpoints for the Subsonic/Native API
//!
//! Per-entity modules (albums, artists, songs, genres, playlists) plus shared
//! star/rating functionality and Subsonic URL helpers.

pub mod albums;
pub mod artists;
pub mod client;
pub mod genres;
pub mod playlists;
pub mod radios;
pub mod rating;
pub mod similar;
pub mod songs;
pub mod sort;
pub mod star;
pub mod subsonic;
