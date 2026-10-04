//! Trawl mix-builder state: the modal, the persistent crate and the search
//! stale-drop generation.

/// Everything Trawl keeps on `Nokkvi`.
#[derive(Debug, Default)]
pub struct TrawlState {
    /// Modal overlay state. `Some` = modal is open. The crate being edited
    /// lives on [`Self::mix`] and survives closing.
    pub modal: Option<crate::widgets::trawl_modal::TrawlModalState>,
    /// The persistent Trawl crate: seeds + blend + the tray filters.
    /// Root-owned so the Harbour row and context menus can accrue seeds while
    /// the modal is closed; cleared on logout (seeds reference server ids).
    pub mix: nokkvi_data::types::trawl::TrawlCrate,
    /// Stale-drop generation for the modal's search fan-outs. Kept here, NOT
    /// on `TrawlModalState`, so close/reopen can never re-mint a generation
    /// an in-flight fan-out already captured.
    pub search_generation: super::StaleDropGen,
}

impl TrawlState {
    /// Logout: the seeds and search results reference the old server's ids.
    /// The search generation carries forward so a fan-out still in flight is
    /// dropped. (Logout also discards the modal through `ActiveModal::STACK`;
    /// clearing it here keeps this reset whole on its own.)
    pub fn reset_for_session(&mut self) {
        *self = Self {
            search_generation: self.search_generation.carried_forward(),
            ..Self::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::{batch::BatchItem, trawl::TrawlSeed};

    use super::*;

    #[test]
    fn reset_for_session_clears_the_crate_and_carries_the_generation() {
        let mut trawl = TrawlState::default();
        trawl.mix.add(TrawlSeed::new(
            BatchItem::Album("al1".into()),
            "A",
            "Artist",
        ));
        trawl.modal = Some(Default::default());
        let in_flight = trawl.search_generation.bump();

        trawl.reset_for_session();

        assert!(trawl.mix.is_empty());
        assert!(trawl.modal.is_none());
        assert!(!trawl.search_generation.accepts(in_flight));
        assert_eq!(trawl.search_generation.current(), in_flight + 1);
    }
}
