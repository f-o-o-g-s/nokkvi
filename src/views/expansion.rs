//! Shared expansion state for slot list views
//!
//! Provides `ExpansionState<C>` for managing inline expansion of parent items
//! into their children, and `SlotListEntry<P, C>` for the unified flattened list entries.
//! Used by Albums (→ Tracks), Artists (→ Albums), Playlists (→ Tracks), Genres (→ Tracks).

use crate::widgets::SlotListPageState;

/// Generic slot list entry — either a parent item or an expanded child item
#[derive(Debug, Clone)]
pub enum SlotListEntry<P, C> {
    Parent(P),
    /// Child within an expanded parent (child data, parent_id)
    Child(C, String),
}

/// Three-tier slot list entry for views with two levels of inline expansion.
///
/// Used by Artists (artist → albums → tracks) and Genres (genre → albums → tracks).
/// `parent_id` in `Child` is the grandparent's ID; `album_id` in `Grandchild` is the parent album's ID.
#[derive(Debug, Clone)]
pub(crate) enum ThreeTierEntry<P, C, G> {
    Parent(P),
    /// Child within an expanded parent (child data, grandparent_id)
    Child(C, String),
    /// Grandchild within an expanded child (grandchild data, album_id)
    Grandchild(G, String),
}

/// Generic inline expansion state
///
/// Tracks which parent is expanded, its loaded children, and the slot list offset
/// before expansion (for restoring on collapse).
#[derive(Debug)]
pub struct ExpansionState<C: Clone> {
    /// ID of the currently expanded parent (at most one at a time)
    pub expanded_id: Option<String>,
    /// Child items loaded for the expanded parent
    pub children: Vec<C>,
    /// Slot List offset before expansion, for restoring on collapse
    pub parent_offset: usize,
}

impl<C: Clone> Default for ExpansionState<C> {
    fn default() -> Self {
        Self {
            expanded_id: None,
            children: Vec::new(),
            parent_offset: 0,
        }
    }
}

impl<C: Clone> ExpansionState<C> {
    /// Whether any item is currently expanded
    pub fn is_expanded(&self) -> bool {
        self.expanded_id.is_some()
    }

    /// Store loaded children for the given parent
    pub fn set_children<P>(
        &mut self,
        parent_id: String,
        children: Vec<C>,
        parents: &[P],
        common: &mut SlotListPageState,
    ) {
        self.expanded_id = Some(parent_id);
        self.children = children;
        // Clear stale click-to-focus selection state so that
        // `has_multi_selection` returns to false and the center-slot
        // fallback highlight works correctly for expanded child rows.
        common.clear_multi_selection();
        // Recalculate slot list with new flattened count
        let flattened_len = self.flattened_len(parents);
        common.slot_list.set_offset(
            common
                .slot_list
                .viewport_offset
                .min(flattened_len.saturating_sub(1)),
            flattened_len,
        );
    }

    /// Build the flattened list of parents + expanded children
    pub fn build_flattened_list<P: Clone>(
        &self,
        parents: &[P],
        id_fn: impl Fn(&P) -> &str,
    ) -> Vec<SlotListEntry<P, C>> {
        let mut entries = Vec::with_capacity(parents.len() + self.children.len());
        for parent in parents {
            let pid = id_fn(parent).to_string();
            entries.push(SlotListEntry::Parent(parent.clone()));
            if Some(&pid) == self.expanded_id.as_ref() {
                for child in &self.children {
                    entries.push(SlotListEntry::Child(child.clone(), pid.clone()));
                }
            }
        }
        entries
    }

    /// Get the total flattened length without allocating the full list (O(1))
    pub fn flattened_len<P>(&self, parents: &[P]) -> usize {
        if self.expanded_id.is_some() {
            parents.len() + self.children.len()
        } else {
            parents.len()
        }
    }

    /// Count how many child entries appear before `flat_index` in the flattened list.
    ///
    /// Used by Albums/Playlists `SlotListActivateCenter` to compute the track index
    /// within the expanded parent without building the full flattened list.
    /// Returns 0 when nothing is expanded.
    pub fn count_children_before<P>(
        &self,
        flat_index: usize,
        parents: &[P],
        id_fn: impl Fn(&P) -> &str,
    ) -> usize {
        if !self.is_expanded() {
            return 0;
        }
        let mut flat_idx = 0;
        for parent in parents {
            if flat_idx >= flat_index {
                break;
            }
            flat_idx += 1;
            if Some(id_fn(parent)) == self.expanded_id.as_deref() {
                // Children are inserted after this parent
                if flat_index <= flat_idx {
                    // Before any children
                    return 0;
                }
                // Count = min(flat_index - flat_idx, children.len())
                return (flat_index - flat_idx).min(self.children.len());
            }
        }
        0
    }

    /// Resolve the entry at the slot list's current center, if any.
    ///
    /// Wraps the three-step lookup (`flattened_len → get_center_item_index →
    /// get_entry_at`) into a single call so handlers don't have to repeat
    /// the dance at every site. Returns `None` when the list is empty or the
    /// center is past the end.
    pub fn resolve_center<'a, P>(
        &'a self,
        parents: &'a [P],
        common: &SlotListPageState,
        id_fn: impl Fn(&P) -> &str,
    ) -> Option<SlotListEntry<&'a P, &'a C>> {
        let total = self.flattened_len(parents);
        let idx = common.slot_list.get_center_item_index(total)?;
        self.get_entry_at(idx, parents, id_fn)
    }

    /// Get a single entry at a flattened index without building the full list.
    ///
    /// Returns a borrowed `SlotListEntry` reference, avoiding the full Vec allocation
    /// that `build_flattened_list` requires. O(n_parents) walk to find the right slot.
    pub fn get_entry_at<'a, P>(
        &'a self,
        idx: usize,
        parents: &'a [P],
        id_fn: impl Fn(&P) -> &str,
    ) -> Option<SlotListEntry<&'a P, &'a C>> {
        if !self.is_expanded() {
            return parents.get(idx).map(SlotListEntry::Parent);
        }
        // Walk through parents, inserting children after the expanded parent
        let mut flat_idx = 0;
        for parent in parents {
            if flat_idx == idx {
                return Some(SlotListEntry::Parent(parent));
            }
            flat_idx += 1;
            if Some(id_fn(parent)) == self.expanded_id.as_deref() {
                // Children are inserted here
                let child_offset = idx.checked_sub(flat_idx)?;
                if child_offset < self.children.len() {
                    let pid = self.expanded_id.clone().unwrap_or_default();
                    return Some(SlotListEntry::Child(&self.children[child_offset], pid));
                }
                flat_idx += self.children.len();
            }
        }
        None
    }

    /// Collapse expansion and restore slot list offset to parent position
    pub fn collapse<P>(
        &mut self,
        parents: &[P],
        id_fn: impl Fn(&P) -> &str,
        common: &mut SlotListPageState,
    ) {
        let parent_idx = self
            .expanded_id
            .as_ref()
            .and_then(|id| parents.iter().position(|p| id_fn(p) == id));
        self.expanded_id = None;
        self.children.clear();
        if let Some(idx) = parent_idx {
            common.handle_set_offset(idx, parents.len());
        }
    }

    /// Silently clear expansion state (for sort/search/viewtype changes that reload data)
    pub fn clear(&mut self) {
        self.expanded_id = None;
        self.children.clear();
    }

    /// Check if a given ID is the expanded parent (for styling)
    pub fn is_expanded_parent(&self, id: &str) -> bool {
        self.expanded_id.as_deref() == Some(id)
    }

    // ── Shared update() helpers ─────────────────────────────────────────
    //
    // These methods extract identical match-arm logic that was previously
    // copy-pasted across Albums, Artists, Genres, and Playlists views.

    /// Handle navigate-up for expansion-aware views.
    /// Returns the center item index after navigation (for artwork loading).
    pub fn handle_navigate_up<P>(
        &self,
        parents: &[P],
        common: &mut SlotListPageState,
    ) -> Option<usize> {
        let len = self.flattened_len(parents);
        common.handle_navigate_up(len);
        common.get_center_item_index(len)
    }

    /// Handle navigate-down for expansion-aware views.
    /// Returns the center item index after navigation (for artwork loading).
    pub fn handle_navigate_down<P>(
        &self,
        parents: &[P],
        common: &mut SlotListPageState,
    ) -> Option<usize> {
        let len = self.flattened_len(parents);
        common.handle_navigate_down(len);
        common.get_center_item_index(len)
    }

    /// Handle set-offset for expansion-aware views.
    /// Returns the center item index after setting offset (for artwork loading).
    pub fn handle_set_offset<P>(
        &self,
        offset: usize,
        parents: &[P],
        common: &mut SlotListPageState,
    ) -> Option<usize> {
        let len = self.flattened_len(parents);
        common.handle_set_offset(offset, len);
        common.get_center_item_index(len)
    }

    /// Handle click-to-focus select for expansion-aware views.
    /// Highlights the item without moving the viewport.
    /// Returns the selected item index (for artwork loading).
    pub fn handle_select_offset<P>(
        &self,
        offset: usize,
        modifiers: iced::keyboard::Modifiers,
        parents: &[P],
        common: &mut SlotListPageState,
    ) -> Option<usize> {
        let len = self.flattened_len(parents);
        common.handle_slot_click(offset, len, modifiers);
        common.get_center_item_index(len)
    }

    /// Handle the ExpandCenter toggle pattern (Shift+Enter).
    ///
    /// Encapsulates the ~30-line toggle logic previously duplicated across
    /// Albums, Artists, Genres, and Playlists views.
    ///
    /// Returns `Some(parent_id)` if a new expansion should be requested from root.
    /// Returns `None` if the toggle was fully handled (collapsed or toggled off).
    pub fn handle_expand_center<P: Clone>(
        &mut self,
        parents: &[P],
        id_fn: impl Fn(&P) -> &str,
        common: &mut SlotListPageState,
    ) -> Option<String> {
        let total = self.flattened_len(parents);
        let center_idx = common.get_center_item_index(total)?;
        let entry = self.get_entry_at(center_idx, parents, &id_fn)?;

        if self.is_expanded() {
            // Center is on a child — collapse
            if matches!(&entry, SlotListEntry::Child(..)) {
                self.collapse(parents, &id_fn, common);
                return None;
            }
            // Center is on a parent
            if let SlotListEntry::Parent(parent) = &entry {
                let pid = id_fn(parent).to_string();
                if self.is_expanded_parent(&pid) {
                    // Same parent — toggle off (collapse)
                    self.collapse(parents, &id_fn, common);
                    return None;
                }
                // Different parent — collapse current, start new expansion
                self.collapse(parents, &id_fn, common);
                if let Some(orig_idx) = parents.iter().position(|p| id_fn(p) == pid) {
                    self.parent_offset = common.slot_list.viewport_offset;
                    common.handle_set_offset(orig_idx, parents.len());
                    return Some(pid);
                }
                return None;
            }
        }
        // Not expanded — expand the centered parent
        let parent = parents.get(center_idx)?;
        self.parent_offset = common.slot_list.viewport_offset;
        Some(id_fn(parent).to_string())
    }

    // ── Sort/search convenience methods ─────────────────────────────────
    //
    // These combine clear() + SlotListPageState delegation, replacing 5-line
    // match arms with 3-line calls in each view.

    /// Handle sort mode change: clear expansion and delegate to common.
    /// Returns `Some(new_mode)` if the mode actually changed.
    pub fn handle_sort_mode_selected(
        &mut self,
        sort_mode: crate::widgets::view_header::SortMode,
        common: &mut SlotListPageState,
    ) -> Option<crate::widgets::view_header::SortMode> {
        self.clear();
        use crate::widgets::SlotListPageAction;
        match common.handle_sort_mode_selected(sort_mode) {
            SlotListPageAction::SortModeChanged(vt) => Some(vt),
            _ => None,
        }
    }

    /// Handle sort order toggle: clear expansion and delegate to common.
    /// Returns `Some(ascending)` if order changed.
    pub fn handle_toggle_sort_order(&mut self, common: &mut SlotListPageState) -> Option<bool> {
        self.clear();
        use crate::widgets::SlotListPageAction;
        match common.handle_toggle_sort_order() {
            SlotListPageAction::SortOrderChanged(asc) => Some(asc),
            _ => None,
        }
    }

    /// Handle search query change: clear expansion and delegate to common.
    /// Returns `Some(query)` if search changed.
    pub fn handle_search_query_changed(
        &mut self,
        query: String,
        total_items: usize,
        common: &mut SlotListPageState,
    ) -> Option<String> {
        self.clear();
        use crate::widgets::SlotListPageAction;
        match common.handle_search_query_changed(query, total_items) {
            SlotListPageAction::SearchChanged(q) => Some(q),
            _ => None,
        }
    }
}

// ── Three-tier flattening helpers ──────────────────────────────────────────

/// Build a flat three-tier list: parents + album children + song grandchildren.
///
/// Used by Artists and Genres views for Artist→Album→Track / Genre→Album→Track expansion.
/// `outer` is the parent→album expansion state; `inner` is the album→track sub-expansion state.
pub(crate) fn build_three_tier_list<P: Clone, C: Clone, G: Clone>(
    parents: &[P],
    outer: &ExpansionState<C>,
    inner: &ExpansionState<G>,
    parent_id_fn: impl Fn(&P) -> &str,
    child_id_fn: impl Fn(&C) -> &str,
) -> Vec<ThreeTierEntry<P, C, G>> {
    let cap = parents.len() + outer.children.len() + inner.children.len();
    let mut entries = Vec::with_capacity(cap);
    for parent in parents {
        let pid = parent_id_fn(parent).to_string();
        entries.push(ThreeTierEntry::Parent(parent.clone()));
        if Some(&pid) == outer.expanded_id.as_ref() {
            for child in &outer.children {
                let cid = child_id_fn(child).to_string();
                entries.push(ThreeTierEntry::Child(child.clone(), pid.clone()));
                if Some(&cid) == inner.expanded_id.as_ref() {
                    for grandchild in &inner.children {
                        entries.push(ThreeTierEntry::Grandchild(grandchild.clone(), cid.clone()));
                    }
                }
            }
        }
    }
    entries
}

/// Total flattened length for a three-tier list (O(1)).
pub(crate) fn three_tier_flattened_len<P, C: Clone>(
    parents: &[P],
    outer: &ExpansionState<C>,
    inner_children_len: usize,
) -> usize {
    if outer.expanded_id.is_some() {
        parents.len() + outer.children.len() + inner_children_len
    } else {
        parents.len()
    }
}

/// Resolve the entry at the slot list's current center for a three-tier view.
///
/// Wraps the three-step lookup (`three_tier_flattened_len → get_center_item_index
/// → three_tier_get_entry_at`) into a single call. Mirrors
/// `ExpansionState::resolve_center` for the artist/genre views that have a
/// second level of inline expansion.
pub(crate) fn resolve_three_tier_center<'a, P, C: Clone, G: Clone>(
    parents: &'a [P],
    outer: &'a ExpansionState<C>,
    inner: &'a ExpansionState<G>,
    common: &SlotListPageState,
    parent_id_fn: impl Fn(&P) -> &str,
    child_id_fn: impl Fn(&C) -> &str,
) -> Option<ThreeTierEntry<&'a P, &'a C, &'a G>> {
    let total = three_tier_flattened_len(parents, outer, inner.children.len());
    let idx = common.slot_list.get_center_item_index(total)?;
    three_tier_get_entry_at(idx, parents, outer, inner, parent_id_fn, child_id_fn)
}

/// Get a single entry at a flat index in a three-tier list without allocating.
///
/// O(n_parents + n_children) walk.
pub(crate) fn three_tier_get_entry_at<'a, P, C: Clone, G: Clone>(
    idx: usize,
    parents: &'a [P],
    outer: &'a ExpansionState<C>,
    inner: &'a ExpansionState<G>,
    parent_id_fn: impl Fn(&P) -> &str,
    child_id_fn: impl Fn(&C) -> &str,
) -> Option<ThreeTierEntry<&'a P, &'a C, &'a G>> {
    if !outer.is_expanded() {
        return parents.get(idx).map(ThreeTierEntry::Parent);
    }
    let mut flat = 0usize;
    for parent in parents {
        if flat == idx {
            return Some(ThreeTierEntry::Parent(parent));
        }
        flat += 1;
        let pid = parent_id_fn(parent);
        if Some(pid) == outer.expanded_id.as_deref() {
            for child in &outer.children {
                if flat == idx {
                    return Some(ThreeTierEntry::Child(child, pid.to_string()));
                }
                flat += 1;
                let cid = child_id_fn(child);
                if Some(cid) == inner.expanded_id.as_deref() {
                    let child_offset = idx.checked_sub(flat)?;
                    if child_offset < inner.children.len() {
                        return Some(ThreeTierEntry::Grandchild(
                            &inner.children[child_offset],
                            cid.to_string(),
                        ));
                    }
                    flat += inner.children.len();
                }
            }
        }
    }
    None
}

// ── Batch payload builder ───────────────────────────────────────────────────

use nokkvi_data::types::batch::{BatchItem, BatchPayload};

/// Build a `BatchPayload` from an iterator of indices and a mapper closure.
///
/// This is the universal batch fold pattern extracted from all 5 view files.
/// Each view provides a closure that maps a flat index to an `Option<BatchItem>`
/// (resolving through its expansion state to determine the correct item type).
///
/// Returns an empty payload if no indices map successfully.
pub(crate) fn build_batch_payload(
    indices: impl IntoIterator<Item = usize>,
    mut mapper: impl FnMut(usize) -> Option<BatchItem>,
) -> BatchPayload {
    indices
        .into_iter()
        .filter_map(&mut mapper)
        .fold(BatchPayload::new(), |p, item| p.with_item(item))
}

// ── Shared child row renderers ──────────────────────────────────────────────

use iced::{
    Alignment, Element, Length,
    widget::{button, container, row, text},
};
use nokkvi_data::utils::formatters;

use crate::widgets::slot_list::{
    SLOT_LIST_SLOT_PADDING, SlotListRowContext, SlotListSlotStyle, slot_list_favorite_icon,
    slot_list_metadata_column, slot_list_text,
};

/// Wrap child row content in a clickable styled container + button.
///
/// Shared by all child row renderers (track rows and album child rows).
fn child_clickable_button<'a, M: Clone + 'a>(
    content: iced::widget::Row<'a, M>,
    ctx: &SlotListRowContext,
    style: SlotListSlotStyle,
    center_msg: M,
    offset_msg: M,
) -> Element<'a, M> {
    let clickable = container(content)
        .style(move |_theme| style.to_container_style())
        .width(Length::Fill);

    button(clickable)
        .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
            offset_msg.clone()
        } else if ctx.is_center {
            center_msg
        } else {
            offset_msg
        })
        .style(|_theme, _status| button::Style {
            background: None,
            border: iced::Border::default(),
            ..Default::default()
        })
        .padding(0)
        .width(Length::Fill)
        .into()
}

/// Render a child **track** row (used by Albums → Tracks and Playlists → Tracks).
///
/// Layout: `[indent] [track#] [title 60%] [artist 20%] [duration 12%] [star 5%]`
pub(crate) fn render_child_track_row<'a, M: Clone + 'a + 'static>(
    song: &nokkvi_data::backend::songs::SongUIViewData,
    ctx: &SlotListRowContext,
    center_msg: M,
    offset_msg: M,
    on_star_click: Option<M>,
    on_artist_click: Option<M>,
    depth: u8,
) -> Element<'a, M> {
    // Visual hierarchy comes from the per-depth `bg0/bg1/bg2` ramp inside
    // `for_slot`'s unfocused branch — not from forcing the now-playing branch.
    let style = SlotListSlotStyle::for_slot(
        ctx.is_center,
        false,
        ctx.is_selected,
        ctx.has_multi_selection,
        ctx.opacity,
        depth,
    );

    let title_size = ctx.metrics.title_size;
    let meta_size = ctx.metrics.metadata_size;
    let star_size = ctx.metrics.star_size_child;

    let track_num = song
        .track
        .map_or_else(|| "-".to_string(), |t| t.to_string());
    let duration_str = formatters::format_time(song.duration);

    let indent_width = if depth > 0 { 30.0 * depth as f32 } else { 50.0 };

    let content = row![
        container(text("")).width(Length::Fixed(indent_width)),
        container(slot_list_text(track_num, meta_size, style.subtext_color))
            .width(Length::Fixed(30.0))
            .height(Length::Fill)
            .align_y(Alignment::Center),
        container(crate::widgets::slot_list::slot_list_text_column(
            song.title.clone(),
            None, // track title doesn't navigate
            song.artist.clone(),
            on_artist_click,
            title_size,
            meta_size,
            style,
            true,
            80, // combined width 60+20
        ),)
        .width(Length::FillPortion(80))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center),
        container(slot_list_text(duration_str, meta_size, style.subtext_color))
            .width(Length::FillPortion(12))
            .height(Length::Fill)
            .align_y(Alignment::Center),
        container(slot_list_favorite_icon::<M>(
            song.is_starred,
            ctx.is_center,
            false,
            ctx.opacity,
            star_size,
            "heart",
            on_star_click,
        ))
        .width(Length::FillPortion(5))
        .padding(iced::Padding {
            left: 4.0,
            right: 4.0,
            ..Default::default()
        })
        .align_x(Alignment::Center)
        .align_y(Alignment::Center),
    ]
    .spacing(4.0)
    .padding(iced::Padding {
        left: SLOT_LIST_SLOT_PADDING,
        right: 4.0,
        top: 2.0,
        bottom: 2.0,
    })
    .align_y(Alignment::Center)
    .height(Length::Fill);

    child_clickable_button(content, ctx, style, center_msg, offset_msg)
}

/// Render a child **album** row (used by Artists → Albums and Genres → Albums).
///
/// When `show_artist` is true (Genres view), includes an artist column.
/// When `show_artwork` is true, prepends a thumbnail column sourced from
/// `artwork_handle` (typically `view_data.album_art.get(&album.id)`).
/// Layout: `[indent] [artwork?] [album name] [artist?] [year 12%] [songs 15%] [duration 12%] [star 5%]`
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_child_album_row<'a, M: Clone + 'a + 'static>(
    album: &nokkvi_data::backend::albums::AlbumUIViewData,
    ctx: &SlotListRowContext,
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    show_artwork: bool,
    center_msg: M,
    offset_msg: M,
    show_artist: bool,
    on_star_click: Option<M>,
    on_song_count_click: Option<M>,
    on_album_click: Option<M>,
    on_artist_click: Option<M>,
    depth: u8,
) -> Element<'a, M> {
    let style = SlotListSlotStyle::for_slot(
        ctx.is_center,
        false,
        ctx.is_selected,
        ctx.has_multi_selection,
        ctx.opacity,
        depth,
    );

    let title_size = ctx.metrics.title_size;
    let meta_size = ctx.metrics.metadata_size;
    let star_size = ctx.metrics.star_size_child;
    let artwork_size = ctx.metrics.artwork_size;

    let year_str = album
        .year
        .map_or_else(|| "-".to_string(), |y| y.to_string());
    let songs_str = format!("{} songs", album.song_count);
    let duration_str = album
        .duration
        .map_or_else(|| "-".to_string(), |d| formatters::format_time(d as u32));

    // Adjust album name width when artist column is shown
    let name_portion = if show_artist { 30 } else { 50 };

    let indent_width = if depth > 0 { 30.0 * depth as f32 } else { 50.0 };

    let mut content =
        iced::widget::Row::new().push(container(text("")).width(Length::Fixed(indent_width)));

    if show_artwork {
        use crate::widgets::slot_list::slot_list_artwork_column;
        content = content.push(slot_list_artwork_column(
            artwork_handle,
            artwork_size,
            ctx.is_center,
            false,
            ctx.opacity,
        ));
    }

    content = content.push(
        container(crate::widgets::slot_list::slot_list_text_column(
            album.name.clone(),
            on_album_click.clone(),
            if show_artist {
                album.artist.clone()
            } else {
                String::new()
            },
            if show_artist { on_artist_click } else { None },
            title_size,
            meta_size,
            style,
            true,
            name_portion + if show_artist { 20 } else { 0 },
        ))
        .width(Length::FillPortion(
            name_portion + if show_artist { 20 } else { 0 },
        ))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center),
    );

    let content = content
        .push(
            container(slot_list_text(year_str, meta_size, style.subtext_color))
                .width(Length::FillPortion(12))
                .height(Length::Fill)
                .align_y(Alignment::Center),
        )
        .push(slot_list_metadata_column(
            songs_str,
            on_song_count_click,
            meta_size,
            style,
            15,
        ))
        .push(
            container(slot_list_text(duration_str, meta_size, style.subtext_color))
                .width(Length::FillPortion(12))
                .height(Length::Fill)
                .align_y(Alignment::Center),
        )
        .push(
            container(slot_list_favorite_icon::<M>(
                album.is_starred,
                ctx.is_center,
                false,
                ctx.opacity,
                star_size,
                "heart",
                on_star_click,
            ))
            .width(Length::FillPortion(5))
            .padding(iced::Padding {
                left: 4.0,
                right: 4.0,
                ..Default::default()
            })
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
        )
        .spacing(4.0)
        .padding(iced::Padding {
            left: SLOT_LIST_SLOT_PADDING,
            right: 4.0,
            top: 4.0,
            bottom: 4.0,
        })
        .align_y(Alignment::Center)
        .height(Length::Fill);

    child_clickable_button(content, ctx, style, center_msg, offset_msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestParent {
        id: String,
        name: String,
    }
    #[derive(Debug, Clone, PartialEq)]
    struct TestChild {
        id: String,
        title: String,
    }

    fn parents() -> Vec<TestParent> {
        vec![
            TestParent {
                id: "a".into(),
                name: "Alpha".into(),
            },
            TestParent {
                id: "b".into(),
                name: "Beta".into(),
            },
            TestParent {
                id: "c".into(),
                name: "Gamma".into(),
            },
        ]
    }

    fn children() -> Vec<TestChild> {
        vec![
            TestChild {
                id: "c1".into(),
                title: "Track 1".into(),
            },
            TestChild {
                id: "c2".into(),
                title: "Track 2".into(),
            },
        ]
    }

    fn id_fn(p: &TestParent) -> &str {
        &p.id
    }

    #[test]
    fn default_not_expanded() {
        let state: ExpansionState<TestChild> = ExpansionState::default();
        assert!(!state.is_expanded());
        assert_eq!(state.expanded_id, None);
        assert!(state.children.is_empty());
    }

    #[test]
    fn flattened_len_no_expansion() {
        let state: ExpansionState<TestChild> = ExpansionState::default();
        assert_eq!(state.flattened_len(&parents()), 3);
    }

    #[test]
    fn flattened_len_with_expansion() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("b".into(), children(), &p, &mut common);
        assert_eq!(state.flattened_len(&p), 5);
    }

    #[test]
    fn build_flattened_list_no_expansion() {
        let state: ExpansionState<TestChild> = ExpansionState::default();
        let flat = state.build_flattened_list(&parents(), id_fn);
        assert_eq!(flat.len(), 3);
        for entry in &flat {
            assert!(matches!(entry, SlotListEntry::Parent(_)));
        }
    }

    #[test]
    fn build_flattened_list_children_after_expanded_parent() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("b".into(), children(), &p, &mut common);
        let flat = state.build_flattened_list(&p, id_fn);
        assert_eq!(flat.len(), 5);
        assert!(matches!(&flat[0], SlotListEntry::Parent(p) if p.id == "a"));
        assert!(matches!(&flat[1], SlotListEntry::Parent(p) if p.id == "b"));
        assert!(matches!(&flat[2], SlotListEntry::Child(c, pid) if c.id == "c1" && pid == "b"));
        assert!(matches!(&flat[3], SlotListEntry::Child(c, pid) if c.id == "c2" && pid == "b"));
        assert!(matches!(&flat[4], SlotListEntry::Parent(p) if p.id == "c"));
    }

    #[test]
    fn get_entry_at_no_expansion() {
        let state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        assert!(
            matches!(state.get_entry_at(1, &p, id_fn), Some(SlotListEntry::Parent(p)) if p.id == "b")
        );
        assert!(state.get_entry_at(5, &p, id_fn).is_none());
    }

    #[test]
    fn get_entry_at_with_expansion() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("a".into(), children(), &p, &mut common);
        // Flattened: [Parent(a), Child(c1), Child(c2), Parent(b), Parent(c)]
        assert!(
            matches!(state.get_entry_at(0, &p, id_fn), Some(SlotListEntry::Parent(p)) if p.id == "a")
        );
        assert!(
            matches!(state.get_entry_at(1, &p, id_fn), Some(SlotListEntry::Child(c, _)) if c.id == "c1")
        );
        assert!(
            matches!(state.get_entry_at(2, &p, id_fn), Some(SlotListEntry::Child(c, _)) if c.id == "c2")
        );
        assert!(
            matches!(state.get_entry_at(3, &p, id_fn), Some(SlotListEntry::Parent(p)) if p.id == "b")
        );
        assert!(
            matches!(state.get_entry_at(4, &p, id_fn), Some(SlotListEntry::Parent(p)) if p.id == "c")
        );
        assert!(state.get_entry_at(5, &p, id_fn).is_none());
    }

    #[test]
    fn get_entry_at_matches_build_flattened() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("b".into(), children(), &p, &mut common);
        let flat = state.build_flattened_list(&p, id_fn);
        for (i, expected) in flat.iter().enumerate() {
            let entry = state.get_entry_at(i, &p, id_fn).unwrap();
            match (expected, &entry) {
                (SlotListEntry::Parent(exp), SlotListEntry::Parent(got)) => {
                    assert_eq!(exp.id, got.id, "Parent mismatch at index {i}");
                }
                (SlotListEntry::Child(exp_c, exp_pid), SlotListEntry::Child(got_c, got_pid)) => {
                    assert_eq!(exp_c.id, got_c.id, "Child mismatch at index {i}");
                    assert_eq!(exp_pid, got_pid, "Parent ID mismatch at index {i}");
                }
                _ => panic!("Entry type mismatch at index {i}"),
            }
        }
    }

    #[test]
    fn is_expanded_parent_checks() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("b".into(), children(), &p, &mut common);
        assert!(state.is_expanded_parent("b"));
        assert!(!state.is_expanded_parent("a"));
        assert!(!state.is_expanded_parent("c"));
    }

    #[test]
    fn clear_resets_state() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("b".into(), children(), &p, &mut common);
        assert!(state.is_expanded());
        state.clear();
        assert!(!state.is_expanded());
        assert!(state.children.is_empty());
        assert_eq!(state.flattened_len(&p), 3);
    }

    #[test]
    fn collapse_clears_expansion() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("b".into(), children(), &p, &mut common);
        assert!(state.is_expanded());
        state.collapse(&p, id_fn, &mut common);
        assert!(!state.is_expanded());
        assert!(state.children.is_empty());
    }

    #[test]
    fn expand_last_parent() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("c".into(), children(), &p, &mut common);
        assert_eq!(state.flattened_len(&p), 5);
        assert!(
            matches!(state.get_entry_at(3, &p, id_fn), Some(SlotListEntry::Child(c, _)) if c.id == "c1")
        );
        assert!(
            matches!(state.get_entry_at(4, &p, id_fn), Some(SlotListEntry::Child(c, _)) if c.id == "c2")
        );
    }

    #[test]
    fn empty_children_expansion() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("a".into(), vec![], &p, &mut common);
        assert!(state.is_expanded());
        assert_eq!(state.flattened_len(&p), 3);
    }

    // ── Tests for count_children_before ──────────────────────────────────

    #[test]
    fn count_children_before_no_expansion() {
        let state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        assert_eq!(state.count_children_before(0, &p, id_fn), 0);
        assert_eq!(state.count_children_before(2, &p, id_fn), 0);
    }

    #[test]
    fn count_children_before_expanded_middle() {
        // Expand "b" → flattened: [a=0, b=1, c1=2, c2=3, c=4]
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("b".into(), children(), &p, &mut common);

        // Before parent "a" (idx 0) → 0 children before
        assert_eq!(state.count_children_before(0, &p, id_fn), 0);
        // Before parent "b" (idx 1) → 0 children before
        assert_eq!(state.count_children_before(1, &p, id_fn), 0);
        // At first child c1 (idx 2) → 0 children before it (it IS the first)
        assert_eq!(state.count_children_before(2, &p, id_fn), 0);
        // At second child c2 (idx 3) → 1 child before
        assert_eq!(state.count_children_before(3, &p, id_fn), 1);
        // At parent "c" (idx 4) → 2 children before
        assert_eq!(state.count_children_before(4, &p, id_fn), 2);
    }

    #[test]
    fn count_children_before_expanded_first() {
        // Expand "a" → flattened: [a=0, c1=1, c2=2, b=3, c=4]
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("a".into(), children(), &p, &mut common);

        assert_eq!(state.count_children_before(0, &p, id_fn), 0);
        assert_eq!(state.count_children_before(1, &p, id_fn), 0);
        assert_eq!(state.count_children_before(2, &p, id_fn), 1);
        assert_eq!(state.count_children_before(3, &p, id_fn), 2);
    }

    #[test]
    fn count_children_before_expanded_last() {
        // Expand "c" → flattened: [a=0, b=1, c=2, c1=3, c2=4]
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        state.set_children("c".into(), children(), &p, &mut common);

        assert_eq!(state.count_children_before(0, &p, id_fn), 0);
        assert_eq!(state.count_children_before(2, &p, id_fn), 0);
        assert_eq!(state.count_children_before(3, &p, id_fn), 0);
        assert_eq!(state.count_children_before(4, &p, id_fn), 1);
    }

    // ── Tests for shared update() helpers ────────────────────────────────

    #[test]
    fn handle_navigate_down_returns_center_index() {
        let state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        // Initial offset is 0, center should be 0
        let result = state.handle_navigate_down(&p, &mut common);
        assert!(result.is_some());
    }

    #[test]
    fn handle_navigate_up_returns_center_index() {
        let state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        // Navigate down first, then back up
        state.handle_navigate_down(&p, &mut common);
        let result = state.handle_navigate_up(&p, &mut common);
        assert!(result.is_some());
    }

    #[test]
    fn handle_expand_center_from_unexpanded() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        // Set slot list offset so center is on parent "a" (index 0)
        common.slot_list.set_offset(0, p.len());
        let result = state.handle_expand_center(&p, id_fn, &mut common);
        assert_eq!(result, Some("a".to_string()));
    }

    #[test]
    fn handle_expand_center_toggle_off_same_parent() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        common.slot_list.set_offset(1, p.len());
        // Expand parent "b"
        state.set_children("b".into(), children(), &p, &mut common);
        // Center on parent "b" and toggle — should collapse
        common.slot_list.set_offset(1, state.flattened_len(&p));
        let result = state.handle_expand_center(&p, id_fn, &mut common);
        assert_eq!(result, None);
        assert!(!state.is_expanded());
    }

    #[test]
    fn handle_expand_center_on_child_collapses() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        // Expand parent "a" with children
        state.set_children("a".into(), children(), &p, &mut common);
        // Center on child (index 1 should be first child of "a")
        let flat_len = state.flattened_len(&p);
        common.slot_list.set_offset(1, flat_len);
        let result = state.handle_expand_center(&p, id_fn, &mut common);
        assert_eq!(result, None);
        assert!(!state.is_expanded());
    }

    #[test]
    fn handle_expand_center_switch_parent() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();
        // Expand parent "a"
        state.set_children("a".into(), children(), &p, &mut common);
        // Center on parent "c" (flattened index: a=0, c1=1, c2=2, b=3, c=4)
        let flat_len = state.flattened_len(&p); // 5
        common.slot_list.set_offset(4, flat_len);
        let result = state.handle_expand_center(&p, id_fn, &mut common);
        // Should collapse "a" and request expansion of "c"
        assert_eq!(result, Some("c".to_string()));
        assert!(!state.is_expanded()); // old one collapsed
    }

    // ══════════════════════════════════════════════════════════════════════
    //  build_batch_payload
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn build_batch_empty_indices() {
        let payload = super::build_batch_payload(vec![], |_| None);
        assert!(payload.items.is_empty());
    }

    #[test]
    fn build_batch_filters_none() {
        let payload = super::build_batch_payload(vec![0, 1, 2], |i| {
            if i == 1 {
                None
            } else {
                Some(BatchItem::Album(format!("a{i}")))
            }
        });
        assert_eq!(payload.items.len(), 2);
    }

    #[test]
    fn build_batch_preserves_order() {
        let payload =
            super::build_batch_payload(vec![2, 0, 1], |i| Some(BatchItem::Album(format!("a{i}"))));
        assert_eq!(payload.items.len(), 3);
        // Items should match input order: a2, a0, a1
        assert!(matches!(&payload.items[0], BatchItem::Album(id) if id == "a2"));
        assert!(matches!(&payload.items[1], BatchItem::Album(id) if id == "a0"));
        assert!(matches!(&payload.items[2], BatchItem::Album(id) if id == "a1"));
    }

    #[test]
    fn build_batch_all_none_returns_empty() {
        let payload = super::build_batch_payload(vec![0, 1, 2], |_| None);
        assert!(payload.items.is_empty());
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Expansion clears stale selection state (FocusAndExpand highlight bug)
    // ══════════════════════════════════════════════════════════════════════

    /// Regression: when clicking "N songs" link on a non-selected album,
    /// `FocusAndExpand` sets `selected_indices = {clicked_index}` before
    /// expansion. When `set_children` later clears `selected_offset` but
    /// leaves `selected_indices` intact, `has_multi_selection = true` for
    /// all slots, which prevents the center-slot fallback highlight from
    /// activating on child tracks. The track at the viewport center slot
    /// loses its highlight background.
    ///
    /// Fix: `set_children` must clear `selected_indices` and `anchor_index`.
    #[test]
    fn set_children_clears_stale_selection_indices() {
        let mut state: ExpansionState<TestChild> = ExpansionState::default();
        let p = parents();
        let mut common = SlotListPageState::default();

        // Simulate FocusAndExpand: user clicked album at index 0 to expand it.
        // handle_slot_click sets selected_offset + selected_indices + anchor_index.
        common.handle_slot_click(0, p.len(), iced::keyboard::Modifiers::empty());
        assert_eq!(common.slot_list.selected_offset, Some(0));
        assert!(common.slot_list.selected_indices.contains(&0));
        assert_eq!(common.slot_list.anchor_index, Some(0));

        // TracksLoaded → set_children clears selected_offset via set_offset,
        // but must ALSO clear selected_indices/anchor_index to prevent
        // has_multi_selection from being true (which disables center highlight).
        state.set_children("a".into(), children(), &p, &mut common);

        assert!(
            common.slot_list.selected_indices.is_empty(),
            "set_children must clear selected_indices so has_multi_selection is false"
        );
        assert_eq!(
            common.slot_list.anchor_index, None,
            "set_children must clear anchor_index"
        );
        assert_eq!(
            common.slot_list.selected_offset, None,
            "set_children must clear selected_offset (via set_offset)"
        );
    }
}
