//! Tags API Service — Navidrome native `GET /api/tag`.
//!
//! Route registered at `reference-navidrome/server/nativeapi/native_api.go`
//! (`r.Route("/tag", …)`); rows are `model/tag.go` `Tag` objects. Standard
//! REST list semantics: `_start`/`_end` paging + `X-Total-Count`.
//!
//! Two consumers: the distinct `tagName` set merges into the smart-criteria
//! [`FieldRegistry`]; per-name values (ordered by song count) feed the rule
//! form's value autocomplete. Both are projected via
//! [`crate::types::smart_criteria::TagDiscovery`].
//!
//! [`FieldRegistry`]: crate::types::smart_criteria::FieldRegistry

use anyhow::{Context, Result};
use tracing::debug;

use crate::services::api::client::ApiClient;

/// One `GET /api/tag` row (`model/tag.go`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Tag {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "tagName", default)]
    pub tag_name: String,
    #[serde(rename = "tagValue", default)]
    pub tag_value: String,
    #[serde(rename = "albumCount", default)]
    pub album_count: u64,
    #[serde(rename = "songCount", default)]
    pub song_count: u64,
}

/// Page size for the tag list sweep.
const TAG_PAGE_SIZE: u32 = 500;

/// Hard cap on total rows fetched — a runaway backstop for pathological
/// libraries (100k+ distinct tag values); discovery degrades gracefully to
/// whatever was fetched.
const TAG_ROW_CAP: usize = 20_000;

#[derive(Clone)]
pub struct TagsApiService {
    client: ApiClient,
}

impl TagsApiService {
    /// Create with a pre-authenticated ApiClient.
    pub fn new(client: ApiClient) -> Self {
        Self { client }
    }

    /// Fetch one page of tags. Returns the rows plus the `X-Total-Count`
    /// header when the server sent one.
    pub async fn list_tags_page(&self, start: u32, end: u32) -> Result<(Vec<Tag>, Option<u32>)> {
        let start_str = start.to_string();
        let end_str = end.to_string();
        let (body, total) = self
            .client
            .get_with_headers(
                "/api/tag",
                &[("_start", start_str.as_str()), ("_end", end_str.as_str())],
            )
            .await?;
        let tags: Vec<Tag> =
            serde_json::from_str(&body).context("Failed to deserialize tag list")?;
        Ok((tags, total))
    }

    /// Fetch the full tag list, paging at [`TAG_PAGE_SIZE`] until the total
    /// (or a short page) says done, capped at [`TAG_ROW_CAP`] rows.
    pub async fn list_all_tags(&self) -> Result<Vec<Tag>> {
        let mut all = Vec::new();
        let mut start = 0u32;
        loop {
            let (page, total) = self.list_tags_page(start, start + TAG_PAGE_SIZE).await?;
            let page_len = page.len();
            all.extend(page);
            let done = page_len < TAG_PAGE_SIZE as usize
                || total.is_some_and(|t| all.len() as u32 >= t)
                || all.len() >= TAG_ROW_CAP;
            if done {
                break;
            }
            start += TAG_PAGE_SIZE;
        }
        debug!(" TagsApiService: discovered {} tag rows", all.len());
        Ok(all)
    }
}
