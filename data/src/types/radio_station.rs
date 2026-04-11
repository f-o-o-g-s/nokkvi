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
    fn searchable_fields(&self) -> Vec<&str> {
        // NOTE from Claude: Gemini, your impl used `matches_query` which doesn't
        // exist on the Searchable trait — it's `searchable_fields`. The generic
        // `filter_items()` in utils/search.rs handles the case-insensitive matching.
        vec![&self.name, &self.stream_url]
    }
}
