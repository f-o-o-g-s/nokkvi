//! Shared star/unstar functionality for any item type (song, album, artist)
//!
//! The Subsonic API uses the same endpoints for all item types - the `id` parameter
//! identifies the item uniquely regardless of type.

use std::sync::Arc;

use anyhow::Result;

use crate::services::api::subsonic;

/// Star an item (mark as favorite)
///
/// Works for any item type: song, album, artist, etc.
/// The Subsonic API's star endpoint is generic.
pub async fn star_item(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    subsonic_credential: &str,
    item_id: &str,
    item_type: &str, // "song", "album", "artist" - for error messages
) -> Result<()> {
    subsonic::subsonic_post_ok(
        http_client,
        server_url,
        "star",
        subsonic_credential,
        &[("id", item_id)],
        &format!("Failed to star {item_type}"),
    )
    .await
}

/// Unstar an item (remove from favorites)
///
/// Works for any item type: song, album, artist, etc.
/// The Subsonic API's unstar endpoint is generic.
pub async fn unstar_item(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    subsonic_credential: &str,
    item_id: &str,
    item_type: &str, // "song", "album", "artist" - for error messages
) -> Result<()> {
    subsonic::subsonic_post_ok(
        http_client,
        server_url,
        "unstar",
        subsonic_credential,
        &[("id", item_id)],
        &format!("Failed to unstar {item_type}"),
    )
    .await
}

/// Toggle star status for an item
///
/// Convenience method that calls star or unstar based on current status.
pub async fn toggle_star(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    subsonic_credential: &str,
    item_id: &str,
    item_type: &str,
    currently_starred: bool,
) -> Result<bool> {
    if currently_starred {
        unstar_item(
            http_client,
            server_url,
            subsonic_credential,
            item_id,
            item_type,
        )
        .await?;
        Ok(false)
    } else {
        star_item(
            http_client,
            server_url,
            subsonic_credential,
            item_id,
            item_type,
        )
        .await?;
        Ok(true)
    }
}
