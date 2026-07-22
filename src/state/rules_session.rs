//! Rules-session UI state — the smart-playlist rules editor mounted inside
//! `View::PlaylistEditor` as `EditorSessionKind::Rules`.
//!
//! ## The three-mode focus machine (test-pinned; an M4 merge blocker)
//!
//! iced delivers no focus events on mouse click (the documented Trawl
//! limitation), so keyboard focus is an EXPLICIT state machine:
//!
//! - **Cursor mode** (default): Up/Down move the form cursor across the
//!   flattened row list — the edit-bar band is row 0. Tab steps the focused
//!   CELL within the row (wrapping); Left/Right CYCLE the focused cell's
//!   value through its `const ALL` for enum cells (conjunction, field,
//!   operator, sort direction, limit mode) and step cells on the edit-bar
//!   band (nothing there cycles). Enter: input cells → Editing mode;
//!   field/operator/playlist cells → sub-picker; toggles → flip; add-rows →
//!   append + move onto the new row; the JSON row → JSON mode. Delete
//!   removes the cursor row (rule / group header / sort key). Escape =
//!   Discard with dirty-confirm.
//! - **Editing mode** (a cell's text_input focused): keystrokes go to the
//!   input; Enter/Tab commit and return to cursor mode; Escape reverts the
//!   CELL and defocuses only — it NEVER discards the session.
//! - **JSON mode** (the raw editor focused): Escape with a clean parse
//!   applies + exits; Escape with a parse error offers keep-editing /
//!   revert-to-snapshot; Ctrl+Enter parses + validates then previews only
//!   when clean.
//!
//! Mouse clicks into inputs reach no handler, so the mode mirror can go
//! stale; the session adopts Editing lazily on the first Captured key event
//! (the Trawl self-heal). The staleness window is cosmetic, never a data
//! hazard.

use nokkvi_data::{
    backend::queue::QueueSongUIViewData,
    types::{
        rules_session::{DraftInfo, RulesTarget},
        smart_criteria::{
            Conjunction, CriteriaGroup, CriteriaNode, Diagnostic, DiagnosticLocation,
            FieldRegistry, RuleLeaf, RuleOperator, ServerCaps, Severity, SmartRules, SortKey,
            TagDiscovery, validate,
        },
    },
};

/// The post-auth server-capability lifecycle (lives on `Nokkvi`). Gating
/// treats `Unfetched`/`FetchFailed` as ALL-FALSE — conservative =
/// feature-hidden, never feature-enabled. `FetchFailed` is distinguished
/// from `Fetched(<0.61)`: a transient failure renders the dimmed
/// "unavailable — retry" create entry; a genuinely-incapable server hides
/// the smart entries outright (nothing to retry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapsState {
    #[default]
    Unfetched,
    FetchFailed,
    Fetched(ServerCaps),
}

impl CapsState {
    /// The effective capability set — all-false unless fetched.
    pub fn caps(&self) -> ServerCaps {
        match self {
            CapsState::Fetched(caps) => *caps,
            CapsState::Unfetched | CapsState::FetchFailed => ServerCaps::default(),
        }
    }

    /// Whether the smart-playlist feature surfaces render (≥0.61).
    pub fn smart_available(&self) -> bool {
        self.caps().rules_via_rest
    }

    /// The dimmed-retry lane: fetch failed, so capability is UNKNOWN (vs a
    /// known-incapable server).
    pub fn fetch_failed(&self) -> bool {
        matches!(self, CapsState::FetchFailed)
    }
}

/// Which pane holds keyboard focus (Shift+Tab hops — a DELIBERATE
/// divergence from Trawl's control-ring Shift+Tab: a tall 2D form wants
/// Up/Down for control-stepping and Shift+Tab for the pane hop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulesPane {
    Form,
    Results,
}

/// The focus machine's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMode {
    Cursor,
    Editing,
    Json,
}

/// One row of the FLATTENED form, recomputed from the rules tree each time
/// it changes ([`RulesSessionUi::rebuild_rows`]). Paths address nodes from
/// the root group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormRow {
    /// The edit-bar band: name / comment / public / Save-as-new.
    EditBar,
    /// `Match [All/Any] of the following`.
    Match,
    /// A rule leaf (or read-only Unknown pill) at this node path.
    Rule(Vec<usize>),
    /// A sub-group header (its conjunction pill; Delete removes the whole
    /// sub-block).
    GroupHeader(Vec<usize>),
    /// Trailing add-row of the group at this path (`[]` = root).
    AddRule(Vec<usize>),
    /// Root-only add-row that wraps a fresh sub-group (seeded with one
    /// rule) into the root. Root-only by construction: a group inside a
    /// group would be depth 2, which locks the form to read-only.
    AddGroup,
    /// One sort key row (`[key] [asc/desc]`), index into effective keys.
    SortKey(usize),
    /// Trailing add-row of the sort builder.
    AddSortKey,
    /// `Limit [n] [#/%]` + offset.
    Limit,
    /// The two-way raw JSON toggle row.
    JsonToggle,
}

/// A cell within the cursor row. Not every row has every cell; the legal
/// cells per row come from [`RulesSessionUi::cells_of_row`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormCell {
    // Edit-bar columns (Left/Right step these).
    Name,
    Comment,
    Public,
    SaveAsNew,
    // Match / group-header rows.
    ConjunctionPill,
    // Rule rows.
    Field,
    Operator,
    /// Value slot 0 (single inputs; the first of a pair).
    Value,
    /// Value slot 1 (the second of a pair/date-pair).
    Value2,
    Remove,
    // Sort rows.
    SortField,
    SortDirection,
    // Limit row.
    LimitValue,
    LimitMode,
    OffsetValue,
    /// Single-cell rows (add-rows, JSON toggle).
    RowAction,
}

/// The cell being edited in Editing mode, with its live buffer and the
/// pre-edit snapshot for Escape-revert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditingCell {
    pub row: FormRow,
    pub cell: FormCell,
    pub buffer: String,
    /// The value as it was when editing began — Escape restores this.
    pub revert: String,
}

/// Which searchable sub-picker overlay is open (the default_playlist_picker
/// modal pattern; joins the outer hotkey modal gate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubPickerKind {
    /// Field picker over the full whitelist (quick rows + "More fields…").
    Field { row: FormRow },
    /// Operator picker for the row's field class.
    Operator { row: FormRow },
    /// Playlist picker for inPlaylist/notInPlaylist values — its row source
    /// EXCLUDES the session target and every draft row.
    Playlist { row: FormRow },
    /// Tag-value picker (genre/mood/…): the library's discovered values for
    /// this tag, so the user picks an exact match instead of typing one
    /// (server tag matching is case-sensitive).
    TagValue { row: FormRow, tag: String },
    /// Star-rating picker (rating/albumrating/artistrating): a fixed 0–5
    /// list, so the user picks the star count instead of typing a number.
    RatingValue { row: FormRow },
    /// Calendar date picker (Date/DatePair value cells): the user picks a day
    /// off a themed month grid instead of hand-typing `YYYY-MM-DD`. `slot2`
    /// selects which endpoint of a date range is being edited; `year`/`month`
    /// track the displayed month (mutated by month-nav, seeded from the cell's
    /// current value or today). The focused day lives in `SubPicker::cursor`.
    DateValue {
        row: FormRow,
        slot2: bool,
        year: i32,
        month: u32,
    },
    /// Sort-key field picker.
    SortField { index: usize },
    /// Preset picker is not a sub-picker — presets render as empty-state
    /// rows.
    None,
}

/// Sub-picker overlay state: immediate search + slot cursor over the
/// filtered entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubPicker {
    pub kind: SubPickerKind,
    pub query: String,
    pub cursor: usize,
}

/// JSON-mode state: the live text, the pre-entry snapshot for the revert
/// lane, and the current parse error (pins the mode when Some).
#[derive(Debug)]
pub struct JsonModeState {
    pub content: iced::widget::text_editor::Content,
    /// The last-good rules snapshot taken at mode entry — the revert lane.
    pub snapshot: SmartRules,
    pub parse_error: Option<String>,
    /// Escape hit a parse error — the two-choice line (keep editing /
    /// revert) is showing.
    pub revert_offer: bool,
}

/// The results pane's lifecycle — six honest states, per-cause copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewPhase {
    /// Nothing evaluated yet. M4 interim copy differs by target (create:
    /// "Save to see matches", edit: "Showing the saved rules' matches");
    /// M5 swaps to "Preview to see matches".
    PreFirst,
    /// A read/evaluation is in flight (style-only variance in the strip).
    Evaluating,
    /// Rows + count + stamp present (rows may be empty ⇒ no-matches copy).
    Loaded,
    /// The last read failed — last-good rows/stamp retained, retry line.
    Failed,
    /// Draft creation failed (M5): authoring-only mode with Retry.
    Unavailable,
}

/// The evaluated results shown in the left pane.
#[derive(Debug, Default)]
pub struct PreviewState {
    pub phase: Option<PreviewPhase>,
    pub rows: Vec<QueueSongUIViewData>,
    /// The FULL fetched songs backing `rows` — Enter-to-play hands these
    /// to `AppService::play_songs` (SongSource::Preloaded).
    pub songs: Vec<nokkvi_data::types::song::Song>,
    /// The playlist id the current evaluation was read from (the draft in
    /// M5; the target for draft-less edge reads) — the paging source.
    pub source_id: Option<String>,
    /// A `_start > 0` page fetch is in flight (the set-loading-before-fetch
    /// discipline — rapid scrolls must not double-fetch).
    pub page_loading: bool,
    /// The server's X-Total-Count — the evaluated match count.
    pub total: Option<u32>,
    /// The playlist's `evaluatedAt` (raw ISO-8601; age-aware formatting at
    /// render).
    pub evaluated_at: Option<String>,
    /// Results-pane center cursor (Up/Down after a Shift+Tab hop).
    pub cursor: usize,
}

impl PreviewState {
    pub fn phase(&self) -> PreviewPhase {
        self.phase.clone().unwrap_or(PreviewPhase::PreFirst)
    }
}

// Configurable columns for the preview/results pane. Like Similar, the preview
// surface has no `View` enum variant, so it owns its own column set + a
// dedicated `OpenMenu::CheckboxDropdownPreview` discriminator. Every field the
// cells read (`starred` / `rating` / `play_count` / `genre` / `duration`)
// already rides `QueueSongUIViewData` — the toggles just gate rendering. All
// five default ON (duration was always rendered before columns existed; the
// rest surface the metadata a rules author most wants to see).
crate::views::define_view_columns! {
    PreviewColumn => PreviewColumnVisibility {
        Stars("Stars"): stars = true => set_preview_show_stars @ preview_show_stars,
        Love("Love"): love = true => set_preview_show_love @ preview_show_love,
        Plays("Plays"): plays = true => set_preview_show_plays @ preview_show_plays,
        Genre("Genre"): genre = true => set_preview_show_genre @ preview_show_genre,
        Duration("Duration"): duration = true => set_preview_show_duration @ preview_show_duration,
    }
}

/// The whole rules-editor session (UI half — the domain half lives in
/// `nokkvi_data::types::{smart_criteria, rules_session}`).
#[derive(Debug)]
pub struct RulesSessionUi {
    pub target: RulesTarget,
    /// The working rules tree.
    pub rules: SmartRules,
    /// The draft workspace playlist (M5 engine; `None` throughout M4 and
    /// until a blank-create's first valid Preview in M5).
    pub draft: Option<DraftInfo>,
    pub mode: FormMode,
    pub pane: RulesPane,
    /// The flattened form rows, rebuilt on every tree change.
    pub rows: Vec<FormRow>,
    /// Cursor row index into `rows`.
    pub cursor: usize,
    /// Focused cell within the cursor row.
    pub cell: FormCell,
    pub editing: Option<EditingCell>,
    pub sub_picker: Option<SubPicker>,
    pub json: Option<JsonModeState>,
    pub diagnostics: Vec<Diagnostic>,
    /// FALSE until the session-open playlists-list fetch resolves — the
    /// dangling-ref + duplicate-name diagnostics stay suppressed while
    /// false (never false-fire against an unloaded list).
    pub playlists_loaded: bool,
    /// The loaded `(id, name)` list feeding the inPlaylist sub-picker and
    /// the diagnostics above.
    pub session_playlists: Vec<(String, String)>,
    pub preview: PreviewState,
    /// The generation captured by in-flight preview tasks (the root owns
    /// the live counter — `rules_preview_generation` on `Nokkvi` — so
    /// close/reopen can't re-mint captured generations).
    pub captured_generation: u64,
    pub caps: ServerCaps,
    pub registry: FieldRegistry,
    pub tag_discovery: Option<TagDiscovery>,
    /// Tag discovery failed — validation hedges its unknown-field copy.
    pub discovery_failed: bool,
    /// Escape-in-cursor-mode dirty confirm is showing.
    pub confirm_discard: bool,
    /// Any authoring change since open (drives the discard confirm).
    pub dirty: bool,
    /// Whether a Save round-trip is in flight (blocks double-submit).
    pub saving: bool,
    /// Save conflict: the target's updatedAt moved under us (reload /
    /// overwrite choice).
    pub save_conflict: bool,
    /// Save target vanished (404) — single recovery: "Save as new…".
    pub save_target_gone: bool,
    /// Post-save observe loop: remaining bounded re-polls while the stamp
    /// hasn't advanced (≤3 over ~12 s — the container ruling's honest
    /// staleness-aware observe).
    pub observe_retries_left: u8,
    /// The evaluation stamp captured just before Save — "advanced" means
    /// the server re-evaluated the newly saved rules.
    pub stamp_before_save: Option<String>,
    /// The rules value as last WRITTEN to the draft (POST or PUT) — the
    /// changed-vs-unchanged split every Preview press makes. `None` before
    /// the first draft write.
    pub last_written_rules: Option<serde_json::Value>,
    /// A mid-session draft 404 already triggered one transparent
    /// recreate — the retry loop guard. Reset on every successful read.
    pub draft_recreate_attempted: bool,
    /// Keyboard cursor into the blank-create empty-state list (Start empty /
    /// Import / presets). Distinct from `cursor` (which indexes the form
    /// rows) so keyboard nav drives the visible list, not the hidden form.
    pub empty_state_cursor: usize,
}

impl RulesSessionUi {
    /// Open a session. Create targets seed a placeholder name via the
    /// surrounding `PlaylistEditState` (the edit-bar collects the real
    /// one) and start in EDITING mode on the name input; edit targets
    /// start in cursor mode on the match row (the rules are the object of
    /// the visit).
    pub fn open(target: RulesTarget, rules: SmartRules, caps: ServerCaps) -> Self {
        let is_create = matches!(target, RulesTarget::Create);
        let mut session = Self {
            target,
            rules,
            draft: None,
            mode: if is_create {
                FormMode::Editing
            } else {
                FormMode::Cursor
            },
            pane: RulesPane::Form,
            rows: Vec::new(),
            cursor: 0,
            cell: FormCell::ConjunctionPill,
            editing: None,
            sub_picker: None,
            json: None,
            diagnostics: Vec::new(),
            playlists_loaded: false,
            session_playlists: Vec::new(),
            preview: PreviewState::default(),
            captured_generation: 0,
            caps,
            registry: FieldRegistry::with_default_tags(),
            tag_discovery: None,
            discovery_failed: false,
            confirm_discard: false,
            dirty: false,
            saving: false,
            save_conflict: false,
            save_target_gone: false,
            observe_retries_left: 0,
            stamp_before_save: None,
            last_written_rules: None,
            draft_recreate_attempted: false,
            empty_state_cursor: 0,
        };
        session.rebuild_rows();
        if is_create {
            // Seed focus: Editing mode ON the edit-bar name input.
            session.cursor = 0;
            session.cell = FormCell::Name;
            session.editing = Some(EditingCell {
                row: FormRow::EditBar,
                cell: FormCell::Name,
                buffer: String::new(),
                revert: String::new(),
            });
        } else {
            // Seed focus: cursor mode on the match row.
            session.cursor = session
                .rows
                .iter()
                .position(|r| matches!(r, FormRow::Match))
                .unwrap_or(0);
            session.cell = FormCell::ConjunctionPill;
        }
        session
    }

    /// Whether the typed form is editable: trees nested deeper than
    /// flat-plus-one lock the form to read-only + edit-as-JSON (the FULL
    /// shape still renders at every depth — visible always, editable as
    /// JSON: a stated scope line, not a hidden loss).
    pub fn form_editable(&self) -> bool {
        self.rules.max_depth() <= 1
    }

    /// Recompute the flattened row list from the rules tree. Every
    /// mutation calls this; the cursor is clamped, not reset.
    pub fn rebuild_rows(&mut self) {
        let mut rows = vec![FormRow::EditBar, FormRow::Match];
        if let Some(root) = &self.rules.root {
            flatten_group(&root.nodes, &mut Vec::new(), &mut rows);
        }
        rows.push(FormRow::AddRule(Vec::new()));
        rows.push(FormRow::AddGroup);
        for (i, _) in self.rules.effective_sort_keys().iter().enumerate() {
            rows.push(FormRow::SortKey(i));
        }
        rows.push(FormRow::AddSortKey);
        rows.push(FormRow::Limit);
        rows.push(FormRow::JsonToggle);
        self.rows = rows;
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
        self.normalize_cell();
    }

    /// The legal cells of a row, in Tab order.
    pub fn cells_of_row(&self, row: &FormRow) -> Vec<FormCell> {
        match row {
            FormRow::EditBar => vec![
                FormCell::Name,
                FormCell::Comment,
                FormCell::Public,
                FormCell::SaveAsNew,
            ],
            FormRow::Match | FormRow::GroupHeader(_) => vec![FormCell::ConjunctionPill],
            FormRow::Rule(path) => match self.node_at(path) {
                Some(CriteriaNode::Leaf(leaf)) => {
                    let mut cells = vec![FormCell::Field, FormCell::Operator];
                    match leaf.operator.value_shape(self.field_class_of(&leaf.field)) {
                        nokkvi_data::types::smart_criteria::ValueShape::Pair
                        | nokkvi_data::types::smart_criteria::ValueShape::DatePair => {
                            cells.push(FormCell::Value);
                            cells.push(FormCell::Value2);
                        }
                        nokkvi_data::types::smart_criteria::ValueShape::FieldFlag => {}
                        _ => cells.push(FormCell::Value),
                    }
                    cells.push(FormCell::Remove);
                    cells
                }
                // Unknown pills: read-only, remove-only.
                _ => vec![FormCell::Remove],
            },
            FormRow::SortKey(_) => vec![
                FormCell::SortField,
                FormCell::SortDirection,
                FormCell::Remove,
            ],
            FormRow::Limit => vec![
                FormCell::LimitValue,
                FormCell::LimitMode,
                FormCell::OffsetValue,
            ],
            FormRow::AddRule(_) | FormRow::AddGroup | FormRow::AddSortKey | FormRow::JsonToggle => {
                vec![FormCell::RowAction]
            }
        }
    }

    /// Clamp `cell` to a legal cell of the cursor row.
    pub fn normalize_cell(&mut self) {
        let Some(row) = self.rows.get(self.cursor).cloned() else {
            return;
        };
        let cells = self.cells_of_row(&row);
        if !cells.contains(&self.cell) {
            self.cell = cells.first().copied().unwrap_or(FormCell::RowAction);
        }
    }

    /// Cursor-mode Up/Down.
    pub fn move_cursor(&mut self, down: bool) {
        if down {
            if self.cursor + 1 < self.rows.len() {
                self.cursor += 1;
            }
        } else if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.normalize_cell();
    }

    /// Cursor-mode Tab: step the focused cell within the row (wrapping).
    pub fn step_cell(&mut self) {
        let Some(row) = self.rows.get(self.cursor).cloned() else {
            return;
        };
        let cells = self.cells_of_row(&row);
        if cells.is_empty() {
            return;
        }
        let idx = cells.iter().position(|c| *c == self.cell).unwrap_or(0);
        self.cell = cells[(idx + 1) % cells.len()];
    }

    /// Step the edit-bar column with Left/Right (the band's ruled motion).
    pub fn step_edit_bar(&mut self, right: bool) {
        let order = [
            FormCell::Name,
            FormCell::Comment,
            FormCell::Public,
            FormCell::SaveAsNew,
        ];
        let idx = order.iter().position(|c| *c == self.cell).unwrap_or(0);
        let next = if right {
            (idx + 1).min(order.len() - 1)
        } else {
            idx.saturating_sub(1)
        };
        self.cell = order[next];
    }

    /// Resolve a node path against the working tree.
    pub fn node_at(&self, path: &[usize]) -> Option<&CriteriaNode> {
        let root = self.rules.root.as_ref()?;
        let mut nodes = &root.nodes;
        let mut node = None;
        for &idx in path {
            node = nodes.get(idx);
            match node {
                Some(CriteriaNode::Group(group)) => nodes = &group.nodes,
                Some(_) => {}
                None => return None,
            }
        }
        node
    }

    /// Mutable node resolution.
    pub fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut CriteriaNode> {
        let root = self.rules.root.as_mut()?;
        let mut nodes = &mut root.nodes;
        for (i, &idx) in path.iter().enumerate() {
            if i + 1 == path.len() {
                return nodes.get_mut(idx);
            }
            match nodes.get_mut(idx) {
                Some(CriteriaNode::Group(group)) => nodes = &mut group.nodes,
                _ => return None,
            }
        }
        None
    }

    /// If the value cell at `path` belongs to a TAG field with discovered
    /// library values (genre/mood/…), the canonical tag name to pick from —
    /// else `None`, so free-text columns keep the typed input.
    pub fn tag_for_value_picker(&self, path: &[usize]) -> Option<String> {
        // Only a single-text value slot picks from the tag list. A range
        // (`inTheRange` → Pair), day-window (`inTheLast` → Days) or other
        // multi-slot shape must never open the scalar tag picker — committing
        // one entry would clobber the `[x, y]` array into a lone string (or
        // land a genre in a days slot). Field validity alone is not enough;
        // an operator swap can leave a tag field carrying a non-text shape.
        if self.value_shape_at(path) != Some(nokkvi_data::types::smart_criteria::ValueShape::Text) {
            return None;
        }
        let field = match self.node_at(path) {
            Some(CriteriaNode::Leaf(leaf)) => leaf.field.as_str(),
            _ => return None,
        };
        let tag = self.registry.resolved_tag_name(field)?;
        let has_values = self
            .tag_discovery
            .as_ref()
            .and_then(|d| d.values_by_tag.get(&tag))
            .is_some_and(|v| !v.is_empty());
        has_values.then_some(tag)
    }

    /// Whether the value cell at `path` is a SINGLE star-rating value
    /// (rating/albumrating/artistrating with a scalar Number shape — not a
    /// range) — those get a fixed 0–5 picker instead of a typed number.
    /// `averagerating` is deliberately excluded: it's a fractional average.
    pub fn is_single_rating_value(&self, path: &[usize]) -> bool {
        if self.value_shape_at(path) != Some(nokkvi_data::types::smart_criteria::ValueShape::Number)
        {
            return false;
        }
        match self.node_at(path) {
            Some(CriteriaNode::Leaf(leaf)) => {
                matches!(
                    leaf.field.as_str(),
                    "rating" | "albumrating" | "artistrating"
                )
            }
            _ => false,
        }
    }

    /// A fresh create session with no rules yet — the state that renders the
    /// preset / Start-empty / Import list instead of the form body. Keyboard
    /// nav drives `empty_state_cursor` here (the form rows are hidden).
    pub fn is_blank_create(&self) -> bool {
        matches!(self.target, RulesTarget::Create)
            && self.rules.root.as_ref().is_none_or(|r| r.nodes.is_empty())
            && !self.dirty
    }

    /// The field class driving a leaf's value shape (unknown fields edit
    /// as text).
    pub fn field_class_of(&self, field: &str) -> nokkvi_data::types::smart_criteria::FieldClass {
        self.registry
            .lookup(field)
            .map_or(nokkvi_data::types::smart_criteria::FieldClass::Text, |k| {
                k.class()
            })
    }

    /// Append a fresh default rule leaf to the group at `path` and move
    /// the cursor onto it. The default rule mirrors the corpus's most
    /// common shape: `rating is 5`.
    pub fn add_rule(&mut self, group_path: &[usize]) {
        let leaf = default_rule_leaf();
        let new_path = if group_path.is_empty() {
            let root = self
                .rules
                .root
                .get_or_insert_with(|| CriteriaGroup::new(Conjunction::All));
            root.nodes.push(leaf);
            vec![root.nodes.len() - 1]
        } else {
            let Some(CriteriaNode::Group(group)) = self.node_at_mut(group_path) else {
                return;
            };
            group.nodes.push(leaf);
            let mut p = group_path.to_vec();
            p.push(group.nodes.len() - 1);
            p
        };
        self.dirty = true;
        self.rebuild_rows();
        self.focus_rule(&new_path);
    }

    /// Append a fresh sub-group to the ROOT, seeded with one default rule,
    /// and move the cursor onto that rule. This is the affordance that lets
    /// the typed form author mixed polarity — `A and (B or C)` — without
    /// dropping into raw JSON.
    ///
    /// The new group takes the OPPOSITE conjunction of the root: nesting a
    /// group that matches its parent's polarity is a logical no-op, so the
    /// only useful default is the flip.
    ///
    /// Root-only by construction. The typed form edits at most
    /// flat-plus-one ([`Self::form_editable`]), so authoring a group inside
    /// a group would lock the very form that created it.
    pub fn add_group(&mut self) {
        let root = self
            .rules
            .root
            .get_or_insert_with(|| CriteriaGroup::new(Conjunction::All));
        let mut group = CriteriaGroup::new(match root.conjunction {
            Conjunction::All => Conjunction::Any,
            Conjunction::Any => Conjunction::All,
        });
        // Seeded, never empty: an empty group is a Save-blocking validation
        // error ("Empty group — add a rule or remove it"), so handing the
        // user one would make the button author an invalid tree.
        group.nodes.push(default_rule_leaf());
        root.nodes.push(CriteriaNode::Group(group));
        let new_path = vec![root.nodes.len() - 1, 0];
        self.dirty = true;
        self.rebuild_rows();
        self.focus_rule(&new_path);
    }

    /// Park the cursor on the rule row at `path`, on its field cell. Shared
    /// by every rule-appending path so a new row always lands the same way.
    fn focus_rule(&mut self, path: &[usize]) {
        if let Some(idx) = self
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::Rule(p) if p == path))
        {
            self.cursor = idx;
            self.cell = FormCell::Field;
        }
    }

    /// Remove the node at `path` (a rule, an Unknown pill, or a whole
    /// group when the path names a group header). Recoverable via Discard.
    pub fn remove_node(&mut self, path: &[usize]) {
        if path.is_empty() {
            return;
        }
        let (parent_path, last) = path.split_at(path.len() - 1);
        let removed = if parent_path.is_empty() {
            if let Some(root) = self.rules.root.as_mut()
                && last[0] < root.nodes.len()
            {
                root.nodes.remove(last[0]);
                true
            } else {
                false
            }
        } else if let Some(CriteriaNode::Group(group)) = self.node_at_mut(parent_path) {
            if last[0] < group.nodes.len() {
                group.nodes.remove(last[0]);
                true
            } else {
                false
            }
        } else {
            false
        };
        if removed {
            self.dirty = true;
            self.rebuild_rows();
        }
    }

    /// Append a fresh sort key (dateadded desc — the corpus favorite) and
    /// move onto it.
    pub fn add_sort_key(&mut self) {
        let mut keys = self.rules.effective_sort_keys();
        keys.push(SortKey {
            field: "dateadded".to_owned(),
            descending: true,
        });
        self.rules.edit_sort(keys);
        self.dirty = true;
        self.rebuild_rows();
        let last = self.rules.effective_sort_keys().len().saturating_sub(1);
        if let Some(idx) = self
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::SortKey(i) if *i == last))
        {
            self.cursor = idx;
            self.cell = FormCell::SortField;
        }
    }

    /// Remove sort key `index`.
    pub fn remove_sort_key(&mut self, index: usize) {
        let mut keys = self.rules.effective_sort_keys();
        if index < keys.len() {
            keys.remove(index);
            self.rules.edit_sort(keys);
            self.dirty = true;
            self.rebuild_rows();
        }
    }

    /// Shift+↑/↓ reorder of a sort key (the reorderable family).
    pub fn move_sort_key(&mut self, index: usize, up: bool) {
        let mut keys = self.rules.effective_sort_keys();
        let target = if up {
            index.checked_sub(1)
        } else {
            (index + 1 < keys.len()).then_some(index + 1)
        };
        let Some(target) = target else { return };
        keys.swap(index, target);
        self.rules.edit_sort(keys);
        self.dirty = true;
        self.rebuild_rows();
        if let Some(idx) = self
            .rows
            .iter()
            .position(|r| matches!(r, FormRow::SortKey(i) if *i == target))
        {
            self.cursor = idx;
        }
    }

    /// Re-run validation into `diagnostics` (immediate, like search).
    pub fn revalidate(&mut self, name: &str, session_target_id: Option<&str>, libraries: &[i32]) {
        let ctx = nokkvi_data::types::smart_criteria::ValidationContext {
            caps: self.caps,
            playlists: &self.session_playlists,
            playlists_loaded: self.playlists_loaded,
            session_target_id,
            known_library_ids: libraries,
            discovery_failed: self.discovery_failed,
            name,
        };
        self.diagnostics = validate(&self.rules, &self.registry, &ctx);
    }

    /// Any Error-severity diagnostic — blocks Preview AND Save.
    pub fn has_blocking_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Diagnostics anchored to a given location (the row renderers pull
    /// their under-row lines through this).
    pub fn diagnostics_at<'a>(
        &'a self,
        location: &'a DiagnosticLocation,
    ) -> impl Iterator<Item = &'a Diagnostic> {
        self.diagnostics
            .iter()
            .filter(move |d| d.location == *location)
    }
}

/// The default rule every add-path seeds: `rating is 5`, the corpus's most
/// common shape. One definition so the root add-row, a group's add-row, and
/// a freshly authored group can't drift apart.
fn default_rule_leaf() -> CriteriaNode {
    CriteriaNode::Leaf(RuleLeaf::new(
        RuleOperator::Is,
        "rating",
        serde_json::json!(5),
    ))
}

fn flatten_group(nodes: &[CriteriaNode], path: &mut Vec<usize>, rows: &mut Vec<FormRow>) {
    for (i, node) in nodes.iter().enumerate() {
        path.push(i);
        match node {
            CriteriaNode::Group(group) => {
                rows.push(FormRow::GroupHeader(path.clone()));
                flatten_group(&group.nodes, path, rows);
                rows.push(FormRow::AddRule(path.clone()));
            }
            CriteriaNode::Leaf(_) | CriteriaNode::Unknown(_) => {
                rows.push(FormRow::Rule(path.clone()));
            }
        }
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::smart_criteria::ValueShape;
    use serde_json::json;

    use super::*;

    fn edit_target() -> RulesTarget {
        RulesTarget::Edit {
            playlist_id: "sp1".into(),
            file_backed: false,
            sync: false,
            loaded_updated_at: "T0".into(),
        }
    }

    fn tiered_rules() -> SmartRules {
        SmartRules::parse(&json!({
            "any": [
                { "all": [ { "is": { "rating": 5 } }, { "notInTheLast": { "lastplayed": 15 } } ] },
                { "is": { "loved": true } }
            ],
            "sort": "lastplayed",
            "limit": 100
        }))
    }

    /// Create sessions seed EDITING mode on the edit-bar name input; edit
    /// sessions seed cursor mode on the match row. (Both pinned — the
    /// one-screen create flow's contract.)
    #[test]
    fn seed_focus_by_target() {
        let create = RulesSessionUi::open(
            RulesTarget::Create,
            SmartRules::new_empty(),
            ServerCaps::default(),
        );
        assert_eq!(create.mode, FormMode::Editing);
        assert_eq!(create.cell, FormCell::Name);
        assert!(matches!(
            create.editing,
            Some(EditingCell {
                cell: FormCell::Name,
                ..
            })
        ));

        let edit = RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        assert_eq!(edit.mode, FormMode::Cursor);
        assert!(matches!(edit.rows[edit.cursor], FormRow::Match));
    }

    /// The flattened row list: edit-bar row 0, match row, rules (group
    /// headers + children + per-group add-rows), root add-row, root
    /// add-GROUP row, sort rows + add, limit, JSON toggle.
    #[test]
    fn flattened_rows_cover_the_full_form() {
        let session = RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        let rows = &session.rows;
        assert_eq!(rows[0], FormRow::EditBar, "the edit-bar band is row 0");
        assert_eq!(rows[1], FormRow::Match);
        assert!(matches!(rows[2], FormRow::GroupHeader(ref p) if p == &vec![0]));
        assert!(matches!(rows[3], FormRow::Rule(ref p) if p == &vec![0, 0]));
        assert!(matches!(rows[4], FormRow::Rule(ref p) if p == &vec![0, 1]));
        assert!(
            matches!(rows[5], FormRow::AddRule(ref p) if p == &vec![0]),
            "each group carries its own trailing add-row"
        );
        assert!(matches!(rows[6], FormRow::Rule(ref p) if p == &vec![1]));
        assert!(matches!(rows[7], FormRow::AddRule(ref p) if p.is_empty()));
        assert_eq!(
            rows[8],
            FormRow::AddGroup,
            "the add-group row is root-only and trails the root add-rule"
        );
        assert_eq!(rows[9], FormRow::SortKey(0));
        assert_eq!(rows[10], FormRow::AddSortKey);
        assert_eq!(rows[11], FormRow::Limit);
        assert_eq!(rows[12], FormRow::JsonToggle);
    }

    /// Up from the match row lands on the edit-bar band; the band's cells
    /// step name → comment → public → Save-as-new with Left/Right.
    #[test]
    fn edit_bar_is_cursor_row_zero() {
        let mut session =
            RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        assert!(matches!(session.rows[session.cursor], FormRow::Match));
        session.move_cursor(false);
        assert!(matches!(session.rows[session.cursor], FormRow::EditBar));
        assert_eq!(session.cell, FormCell::Name);
        session.step_edit_bar(true);
        assert_eq!(session.cell, FormCell::Comment);
        session.step_edit_bar(true);
        assert_eq!(session.cell, FormCell::Public);
        session.step_edit_bar(true);
        assert_eq!(session.cell, FormCell::SaveAsNew);
        session.step_edit_bar(true);
        assert_eq!(session.cell, FormCell::SaveAsNew, "clamped at the end");
        session.step_edit_bar(false);
        assert_eq!(session.cell, FormCell::Public);
    }

    /// add_rule appends to the addressed group and moves the cursor onto
    /// the new row with the field cell focused — the keyboard-complete
    /// authoring loop's first step.
    #[test]
    fn add_rule_moves_cursor_onto_new_row() {
        let mut session =
            RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        session.add_rule(&[]);
        assert!(
            matches!(&session.rows[session.cursor], FormRow::Rule(p) if p == &vec![2]),
            "cursor lands on the appended root rule"
        );
        assert_eq!(session.cell, FormCell::Field);
        assert!(session.dirty);

        // Group add-row appends INSIDE the group.
        session.add_rule(&[0]);
        assert!(matches!(&session.rows[session.cursor], FormRow::Rule(p) if p == &vec![0, 2]));
    }

    /// Delete removes the cursor row's node; removing a group header
    /// removes the whole sub-block.
    #[test]
    fn remove_node_covers_rules_and_groups() {
        let mut session =
            RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        session.remove_node(&[1]); // the loved leaf
        let root_len = session.rules.root.as_ref().map(|r| r.nodes.len());
        assert_eq!(root_len, Some(1));
        session.remove_node(&[0]); // the whole tier group
        let root_len = session.rules.root.as_ref().map(|r| r.nodes.len());
        assert_eq!(root_len, Some(0));
        assert!(
            session
                .rows
                .iter()
                .all(|r| !matches!(r, FormRow::Rule(_) | FormRow::GroupHeader(_))),
            "no rule rows survive"
        );
    }

    /// Sort keys: add lands on the new row, Shift-move swaps, remove
    /// drops — all through the canonicalizing edit_sort path.
    #[test]
    fn sort_key_lifecycle() {
        let mut session =
            RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        session.add_sort_key();
        assert!(matches!(session.rows[session.cursor], FormRow::SortKey(1)));
        let keys = session.rules.effective_sort_keys();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[1].field, "dateadded");

        session.move_sort_key(1, true);
        let keys = session.rules.effective_sort_keys();
        assert_eq!(keys[0].field, "dateadded", "Shift+Up swapped the keys");

        session.remove_sort_key(0);
        let keys = session.rules.effective_sort_keys();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].field, "lastplayed");
    }

    /// cells_of_row adapts to the leaf's value shape: pairs get two value
    /// cells, presence ops get none, unknown pills are remove-only.
    #[test]
    fn cells_follow_value_shape() {
        let mut session = RulesSessionUi::open(
            edit_target(),
            SmartRules::parse(&json!({
                "all": [
                    { "inTheRange": { "year": [1990, 1999] } },
                    { "isMissing": { "lyrics": true } },
                    { "futureOp": { "x": 1 } }
                ]
            })),
            ServerCaps::default(),
        );
        session.rebuild_rows();
        let range_cells = session.cells_of_row(&FormRow::Rule(vec![0]));
        assert!(range_cells.contains(&FormCell::Value));
        assert!(range_cells.contains(&FormCell::Value2));
        let presence_cells = session.cells_of_row(&FormRow::Rule(vec![1]));
        assert!(!presence_cells.contains(&FormCell::Value));
        let unknown_cells = session.cells_of_row(&FormRow::Rule(vec![2]));
        assert_eq!(unknown_cells, vec![FormCell::Remove], "unknown = read-only");

        // Sanity: the range leaf's shape resolves as Pair on a number
        // field.
        let Some(CriteriaNode::Leaf(leaf)) = session.node_at(&[0]) else {
            panic!("leaf expected")
        };
        assert_eq!(
            leaf.operator
                .value_shape(session.field_class_of(&leaf.field)),
            ValueShape::Pair
        );
    }

    /// Deep trees lock the typed form (read-only + edit-as-JSON) while the
    /// full shape stays flattened and visible.
    #[test]
    fn deep_nesting_locks_form() {
        let session = RulesSessionUi::open(
            edit_target(),
            SmartRules::parse(&json!({
                "all": [ { "any": [ { "all": [ { "is": { "loved": true } } ] } ] } ]
            })),
            ServerCaps::default(),
        );
        assert!(!session.form_editable(), "depth 2 locks the typed form");
        assert!(
            session
                .rows
                .iter()
                .any(|r| matches!(r, FormRow::Rule(p) if p == &vec![0, 0, 0])),
            "the deep leaf still renders (visible always)"
        );

        let flat = RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        assert!(flat.form_editable(), "flat-plus-one stays editable");
    }

    /// revalidate + list-loaded gating: the duplicate-name warning fires
    /// only after the session playlists list genuinely loads.
    #[test]
    fn revalidate_respects_list_loading() {
        let mut session =
            RulesSessionUi::open(edit_target(), tiered_rules(), ServerCaps::default());
        session.session_playlists = vec![("p9".into(), "Road Trip".into())];

        session.revalidate("Road Trip", Some("sp1"), &[]);
        assert!(
            !session
                .diagnostics
                .iter()
                .any(|d| d.location == DiagnosticLocation::Name),
            "duplicate-name suppressed while playlists_loaded is false"
        );

        session.playlists_loaded = true;
        session.revalidate("Road Trip", Some("sp1"), &[]);
        assert!(
            session
                .diagnostics
                .iter()
                .any(|d| d.location == DiagnosticLocation::Name && d.severity == Severity::Warning),
            "…and fires once loaded"
        );
    }
}
