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
    pub search_generation: u64,
}
