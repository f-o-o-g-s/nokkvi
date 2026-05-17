use crate::types::song::Song;

/// Length anchor used by `ItemKind`'s drift checks. Update in lockstep
/// with the `enum BatchItem` variant count.
pub const BATCH_ITEM_VARIANT_COUNT: usize = 5;

/// A single item representing a selection source. We use this to maintain the exact
/// visual Top-to-Bottom order that the user originally clicked things in, so the queue
/// will be built in the correct intuitive order.
#[derive(Debug, Clone)]
pub enum BatchItem {
    Song(Box<Song>),
    Album(String),
    Artist(String),
    Genre(String),
    Playlist(String),
}

/// A payload containing a batch of items (usually generated from a multi-selection shift+click).
#[derive(Debug, Clone, Default)]
pub struct BatchPayload {
    pub items: Vec<BatchItem>,
}

impl BatchPayload {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn with_item(mut self, item: BatchItem) -> Self {
        self.items.push(item);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anchor `BATCH_ITEM_VARIANT_COUNT` to the actual `BatchItem` variants.
    ///
    /// The `const _:` length anchors in `item_kind.rs` only pin that the
    /// constant equals `5` — they do not catch a drift where someone adds
    /// a sixth variant but forgets to bump the constant. This test does:
    /// the match below is exhaustive (no `_ =>` arm), so a new variant
    /// is a compile error here, and the `assert_eq!` then checks the
    /// constant tracks the variant count.
    #[test]
    fn batch_item_variant_count_matches_constant() {
        fn discriminant_id(v: &BatchItem) -> usize {
            match v {
                BatchItem::Song(_) => 1,
                BatchItem::Album(_) => 2,
                BatchItem::Artist(_) => 3,
                BatchItem::Genre(_) => 4,
                BatchItem::Playlist(_) => 5,
            }
        }
        let samples = [
            BatchItem::Song(Box::new(Song::test_default("s", "t"))),
            BatchItem::Album("a".to_string()),
            BatchItem::Artist("ar".to_string()),
            BatchItem::Genre("g".to_string()),
            BatchItem::Playlist("p".to_string()),
        ];
        let mut seen: Vec<usize> = samples.iter().map(discriminant_id).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), BATCH_ITEM_VARIANT_COUNT);
        assert_eq!(BATCH_ITEM_VARIANT_COUNT, 5);
    }
}
