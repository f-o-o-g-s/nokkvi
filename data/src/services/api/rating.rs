//! Shared rating functionality for any item type (song, album)
//!
//! The Subsonic API `setRating` endpoint is generic — the `id` parameter
//! identifies the item uniquely regardless of type.

use std::sync::Arc;

use anyhow::Result;

use crate::services::api::subsonic;

/// Set the rating for an item (song, album, etc.)
///
/// Rating values: 0 = unrated, 1-5 = star rating.
/// The Subsonic API's `setRating` endpoint requires both `id` and `rating` params.
pub async fn set_rating(
    http_client: &Arc<reqwest::Client>,
    server_url: &str,
    subsonic_credential: &str,
    item_id: &str,
    rating: u32,
) -> Result<()> {
    let rating_str = rating.to_string();
    subsonic::subsonic_post_ok(
        http_client,
        server_url,
        "setRating",
        subsonic_credential,
        &[("id", item_id), ("rating", &rating_str)],
        &format!("Failed to set rating for {item_id}"),
    )
    .await
}
