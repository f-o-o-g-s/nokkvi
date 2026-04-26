use serde::{Deserialize, Serialize};

use crate::utils::search::Searchable;

/// Navidrome internet radio station (from getInternetRadioStations.view)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RadioStation {
    pub id: String,
    pub name: String,
    #[serde(rename = "streamUrl")]
    pub stream_url: String,
    #[serde(rename = "homePageUrl")]
    pub home_page_url: Option<String>,
}

impl Searchable for RadioStation {
    /// Compute on the fly. Stations are typically a few dozen at most, so the
    /// per-call lowercase allocation is negligible. Caching would require
    /// either a `#[serde(skip)]` field with post-deserialize repopulation or
    /// changing the on-disk JSON shape — neither is justified at this scale.
    fn matches_query(&self, query_lower: &str) -> bool {
        self.name.to_lowercase().contains(query_lower)
            || self.stream_url.to_lowercase().contains(query_lower)
    }
}
