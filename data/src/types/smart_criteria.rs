//! Smart-playlist criteria domain model — the typed view over the raw
//! `rules` JSON carried on [`crate::types::playlist::Playlist::rules`].
//!
//! Pinned against `reference-navidrome/model/criteria/*` (fields.go,
//! json.go, operators.go, sort.go) and `persistence/criteria_sql.go`. Every
//! server behavior relied on here is cited at the point of use; re-verify
//! the pins on server-version bumps.
//!
//! ## Round-trip contract
//!
//! `SmartRules::parse(&value)` followed by `to_value()` with NO edits in
//! between MUST reproduce the input `serde_json::Value` exactly (Value
//! equality — key order is irrelevant to `Value`). Everything the typed
//! model doesn't understand is preserved verbatim: unknown top-level keys
//! ride in [`SmartRules::extra`], unknown operators become
//! [`CriteriaNode::Unknown`], the raw sort string and the legacy top-level
//! `order` key survive untouched until the user EDITS the sort (the one
//! deliberate canonicalization point — [`SmartRules::edit_sort`]).

use std::collections::HashMap;

use serde_json::{Map, Value};

// =========================================================================
// Operators
// =========================================================================

/// The 17 server operators — a CLOSED enum pinned to
/// `model/criteria/json.go` `unmarshalExpression` (keys are lowercased on
/// parse there, so matching is case-insensitive; the canonical wire
/// spelling comes from `operators.go` `marshalExpression` calls). There is
/// NO `notInTheRange` (an nsp.py phantom the server never accepted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleOperator {
    Is,
    IsNot,
    Gt,
    Lt,
    Before,
    After,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    InTheRange,
    InTheLast,
    NotInTheLast,
    InPlaylist,
    NotInPlaylist,
    IsMissing,
    IsPresent,
}

/// What kind of value editor a rule row renders — resolved from the
/// operator AND the field's class (dates flip scalar shapes to date
/// shapes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    /// Free text input.
    Text,
    /// Numeric input.
    Number,
    /// `YYYY-MM-DD` date input with immediate format validation.
    Date,
    /// Two numeric inputs (`inTheRange` on non-date fields).
    Pair,
    /// Two date inputs (`inTheRange` on date fields).
    DatePair,
    /// Number + a literal "days" suffix (`inTheLast`/`notInTheLast`).
    Days,
    /// Playlist sub-picker (`inPlaylist`/`notInPlaylist`).
    PlaylistRef,
    /// On/Off pills (boolean fields under is/isNot).
    Toggle,
    /// The unary presence ops — value is the field name flagged `true`.
    FieldFlag,
}

impl RuleOperator {
    /// Render-order list (the Trawl `const ALL` pattern) — Left/Right value
    /// cycling and the operator sub-picker both walk this.
    pub const ALL: [RuleOperator; 17] = [
        RuleOperator::Is,
        RuleOperator::IsNot,
        RuleOperator::Contains,
        RuleOperator::NotContains,
        RuleOperator::StartsWith,
        RuleOperator::EndsWith,
        RuleOperator::Gt,
        RuleOperator::Lt,
        RuleOperator::InTheRange,
        RuleOperator::Before,
        RuleOperator::After,
        RuleOperator::InTheLast,
        RuleOperator::NotInTheLast,
        RuleOperator::InPlaylist,
        RuleOperator::NotInPlaylist,
        RuleOperator::IsMissing,
        RuleOperator::IsPresent,
    ];

    /// The canonical wire spelling (`marshalExpression` names).
    pub fn wire_key(self) -> &'static str {
        match self {
            RuleOperator::Is => "is",
            RuleOperator::IsNot => "isNot",
            RuleOperator::Gt => "gt",
            RuleOperator::Lt => "lt",
            RuleOperator::Before => "before",
            RuleOperator::After => "after",
            RuleOperator::Contains => "contains",
            RuleOperator::NotContains => "notContains",
            RuleOperator::StartsWith => "startsWith",
            RuleOperator::EndsWith => "endsWith",
            RuleOperator::InTheRange => "inTheRange",
            RuleOperator::InTheLast => "inTheLast",
            RuleOperator::NotInTheLast => "notInTheLast",
            RuleOperator::InPlaylist => "inPlaylist",
            RuleOperator::NotInPlaylist => "notInPlaylist",
            RuleOperator::IsMissing => "isMissing",
            RuleOperator::IsPresent => "isPresent",
        }
    }

    /// Parse an operator key the way the server does: lowercased.
    pub fn from_wire_key(key: &str) -> Option<Self> {
        Some(match key.to_lowercase().as_str() {
            "is" => RuleOperator::Is,
            "isnot" => RuleOperator::IsNot,
            "gt" => RuleOperator::Gt,
            "lt" => RuleOperator::Lt,
            "before" => RuleOperator::Before,
            "after" => RuleOperator::After,
            "contains" => RuleOperator::Contains,
            "notcontains" => RuleOperator::NotContains,
            "startswith" => RuleOperator::StartsWith,
            "endswith" => RuleOperator::EndsWith,
            "intherange" => RuleOperator::InTheRange,
            "inthelast" => RuleOperator::InTheLast,
            "notinthelast" => RuleOperator::NotInTheLast,
            "inplaylist" => RuleOperator::InPlaylist,
            "notinplaylist" => RuleOperator::NotInPlaylist,
            "ismissing" => RuleOperator::IsMissing,
            "ispresent" => RuleOperator::IsPresent,
            _ => return None,
        })
    }

    /// Human label for pickers.
    pub fn label(self) -> &'static str {
        match self {
            RuleOperator::Is => "is",
            RuleOperator::IsNot => "is not",
            RuleOperator::Gt => "is greater than",
            RuleOperator::Lt => "is less than",
            RuleOperator::Before => "is before",
            RuleOperator::After => "is after",
            RuleOperator::Contains => "contains",
            RuleOperator::NotContains => "doesn't contain",
            RuleOperator::StartsWith => "starts with",
            RuleOperator::EndsWith => "ends with",
            RuleOperator::InTheRange => "is in the range",
            RuleOperator::InTheLast => "in the last (days)",
            RuleOperator::NotInTheLast => "not in the last (days)",
            RuleOperator::InPlaylist => "is in playlist",
            RuleOperator::NotInPlaylist => "is not in playlist",
            RuleOperator::IsMissing => "is missing",
            RuleOperator::IsPresent => "is present",
        }
    }

    /// The value editor this operator wants, given the field's class.
    pub fn value_shape(self, class: FieldClass) -> ValueShape {
        match self {
            RuleOperator::Is | RuleOperator::IsNot => match class {
                FieldClass::Bool => ValueShape::Toggle,
                FieldClass::Number => ValueShape::Number,
                FieldClass::Date => ValueShape::Date,
                FieldClass::Text => ValueShape::Text,
            },
            RuleOperator::Gt | RuleOperator::Lt => match class {
                FieldClass::Date => ValueShape::Date,
                FieldClass::Text => ValueShape::Text,
                FieldClass::Number | FieldClass::Bool => ValueShape::Number,
            },
            RuleOperator::Before | RuleOperator::After => ValueShape::Date,
            RuleOperator::Contains
            | RuleOperator::NotContains
            | RuleOperator::StartsWith
            | RuleOperator::EndsWith => ValueShape::Text,
            RuleOperator::InTheRange => match class {
                FieldClass::Date => ValueShape::DatePair,
                _ => ValueShape::Pair,
            },
            RuleOperator::InTheLast | RuleOperator::NotInTheLast => ValueShape::Days,
            RuleOperator::InPlaylist | RuleOperator::NotInPlaylist => ValueShape::PlaylistRef,
            RuleOperator::IsMissing | RuleOperator::IsPresent => ValueShape::FieldFlag,
        }
    }
}

// =========================================================================
// Field registry — three tiers
// =========================================================================

/// Semantic class of a criteria field, driving value shapes and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldClass {
    Text,
    Number,
    Date,
    Bool,
}

/// One static-column row, mirrored from
/// `reference-navidrome/model/criteria/fields.go` (`fieldMap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDef {
    /// Canonical wire name (lowercase).
    pub name: &'static str,
    /// Picker label.
    pub label: &'static str,
    pub class: FieldClass,
    /// Mirrors fields.go `Nullable` — the presence ops are valid on this
    /// COLUMN only when true AND the server is ≥0.63 (see
    /// [`ServerCaps::nullable_column_presence_ops`]).
    pub nullable: bool,
    /// Minimum server version floor for this field, compared against
    /// `ServerCaps.version` in `validate_leaf`. `None` = present since the
    /// 0.61 rules-via-REST floor. The field set is NOT stable across
    /// versions: `codec`/`missing`/`samplerate`/`rg*` arrived in 0.62.0 and
    /// the `replaygain_*` aliases in 0.63.0 — offering them to an older
    /// server saves the rule then permanently-empties the playlist (D5), so
    /// they carry a floor.
    pub min_server: Option<(u32, u32, u32)>,
}

/// Resolve a server-registered field alias to its canonical name so the
/// class matches the server (`model/criteria/fields.go`). `albumtype` is the
/// `releasetype` tag; `recordingdate` is the scalar `date` column. Any other
/// name passes through unchanged.
pub(crate) fn resolve_field_alias(key: &str) -> &str {
    match key {
        "albumtype" => "releasetype",
        "recordingdate" => "date",
        other => other,
    }
}

const fn col(name: &'static str, label: &'static str, class: FieldClass) -> FieldDef {
    FieldDef {
        name,
        label,
        class,
        nullable: false,
        min_server: None,
    }
}

const fn col_null(name: &'static str, label: &'static str, class: FieldClass) -> FieldDef {
    FieldDef {
        name,
        label,
        class,
        nullable: true,
        min_server: None,
    }
}

/// A non-nullable column with a minimum-server-version floor — the field
/// does not exist on older servers, which would persist the rule and then
/// evaluate it to zero matches (the D5 permanently-empty class).
const fn col_since(
    name: &'static str,
    label: &'static str,
    class: FieldClass,
    min: (u32, u32, u32),
) -> FieldDef {
    FieldDef {
        name,
        label,
        class,
        nullable: false,
        min_server: Some(min),
    }
}

/// A nullable column with a minimum-server-version floor.
const fn col_null_since(
    name: &'static str,
    label: &'static str,
    class: FieldClass,
    min: (u32, u32, u32),
) -> FieldDef {
    FieldDef {
        name,
        label,
        class,
        nullable: true,
        min_server: Some(min),
    }
}

/// Tier 1: the static column fields, pinned to fields.go. `random` is a
/// sort-only pseudo-field and deliberately NOT here (it is accepted as a
/// sort key with a warning instead).
pub const STATIC_FIELDS: &[FieldDef] = &[
    col("title", "Title", FieldClass::Text),
    col_null("album", "Album", FieldClass::Text),
    col("hascoverart", "Has cover art", FieldClass::Bool),
    col("tracknumber", "Track number", FieldClass::Number),
    col("discnumber", "Disc number", FieldClass::Number),
    col("year", "Year", FieldClass::Number),
    col("date", "Recording date", FieldClass::Date),
    col("originalyear", "Original year", FieldClass::Number),
    col("originaldate", "Original date", FieldClass::Date),
    col("releaseyear", "Release year", FieldClass::Number),
    col("releasedate", "Release date", FieldClass::Date),
    col("size", "File size", FieldClass::Number),
    col("compilation", "Compilation", FieldClass::Bool),
    col_since("missing", "Missing file", FieldClass::Bool, (0, 62, 0)),
    col_null("explicitstatus", "Explicit status", FieldClass::Text),
    col("dateadded", "Date added", FieldClass::Date),
    col("datemodified", "Date modified", FieldClass::Date),
    col_null("discsubtitle", "Disc subtitle", FieldClass::Text),
    col_null("comment", "Comment", FieldClass::Text),
    col_null("lyrics", "Lyrics", FieldClass::Text),
    col_null("sorttitle", "Sort title", FieldClass::Text),
    col_null("sortalbum", "Sort album", FieldClass::Text),
    col_null("sortartist", "Sort artist", FieldClass::Text),
    col_null("sortalbumartist", "Sort album artist", FieldClass::Text),
    col_null("albumcomment", "Album comment", FieldClass::Text),
    col_null("catalognumber", "Catalog number", FieldClass::Text),
    col("filepath", "File path", FieldClass::Text),
    col("filetype", "File type", FieldClass::Text),
    col_since("codec", "Codec", FieldClass::Text, (0, 62, 0)),
    col("duration", "Duration (seconds)", FieldClass::Number),
    col("bitrate", "Bitrate", FieldClass::Number),
    col_null("bitdepth", "Bit depth", FieldClass::Number),
    col_since("samplerate", "Sample rate", FieldClass::Number, (0, 62, 0)),
    col_null("bpm", "BPM", FieldClass::Number),
    col("channels", "Channels", FieldClass::Number),
    col("loved", "Loved", FieldClass::Bool),
    col("dateloved", "Date loved", FieldClass::Date),
    col("lastplayed", "Last played", FieldClass::Date),
    col("daterated", "Date rated", FieldClass::Date),
    col("playcount", "Play count", FieldClass::Number),
    col("rating", "Rating", FieldClass::Number),
    col("averagerating", "Average rating", FieldClass::Number),
    col("albumrating", "Album rating", FieldClass::Number),
    col("albumloved", "Album loved", FieldClass::Bool),
    col("albumplaycount", "Album play count", FieldClass::Number),
    col("albumlastplayed", "Album last played", FieldClass::Date),
    col("albumdateloved", "Album date loved", FieldClass::Date),
    col("albumdaterated", "Album date rated", FieldClass::Date),
    col("artistrating", "Artist rating", FieldClass::Number),
    col("artistloved", "Artist loved", FieldClass::Bool),
    col("artistplaycount", "Artist play count", FieldClass::Number),
    col("artistlastplayed", "Artist last played", FieldClass::Date),
    col("artistdateloved", "Artist date loved", FieldClass::Date),
    col("artistdaterated", "Artist date rated", FieldClass::Date),
    col_null("mbz_album_id", "MusicBrainz album id", FieldClass::Text),
    col_null(
        "mbz_album_artist_id",
        "MusicBrainz album-artist id",
        FieldClass::Text,
    ),
    col_null("mbz_artist_id", "MusicBrainz artist id", FieldClass::Text),
    col_null(
        "mbz_recording_id",
        "MusicBrainz recording id",
        FieldClass::Text,
    ),
    col_null(
        "mbz_release_track_id",
        "MusicBrainz release-track id",
        FieldClass::Text,
    ),
    col_null(
        "mbz_release_group_id",
        "MusicBrainz release-group id",
        FieldClass::Text,
    ),
    col_null_since(
        "rgalbumgain",
        "ReplayGain album gain",
        FieldClass::Number,
        (0, 62, 0),
    ),
    col_null_since(
        "rgalbumpeak",
        "ReplayGain album peak",
        FieldClass::Number,
        (0, 62, 0),
    ),
    col_null_since(
        "rgtrackgain",
        "ReplayGain track gain",
        FieldClass::Number,
        (0, 62, 0),
    ),
    col_null_since(
        "rgtrackpeak",
        "ReplayGain track peak",
        FieldClass::Number,
        (0, 62, 0),
    ),
    col("library_id", "Library id", FieldClass::Number),
    // Backward-compat aliases the server registers explicitly. `albumtype`
    // is deliberately NOT here — it aliases to the `releasetype` TAG
    // (`resolve_field_alias`); classifying it as a column mislabels it
    // (false Save block on presence, D5 on range). The `replaygain_*` long
    // forms were added by #5585 in 0.63.0 (the `rg*` short forms above are
    // 0.62.0) — floored so an older server doesn't save-then-empty on them.
    col_null_since(
        "replaygain_album_gain",
        "ReplayGain album gain (tag alias)",
        FieldClass::Number,
        (0, 63, 0),
    ),
    col_null_since(
        "replaygain_album_peak",
        "ReplayGain album peak (tag alias)",
        FieldClass::Number,
        (0, 63, 0),
    ),
    col_null_since(
        "replaygain_track_gain",
        "ReplayGain track gain (tag alias)",
        FieldClass::Number,
        (0, 63, 0),
    ),
    col_null_since(
        "replaygain_track_peak",
        "ReplayGain track peak (tag alias)",
        FieldClass::Number,
        (0, 63, 0),
    ),
];

/// Tier 2: the 14 artist roles, version-pinned to `model/participants.go`
/// `AllRoles`. Registered at server startup via `criteria.AddRoles`, so
/// they are always valid rule fields. `artist`/`albumartist` sit in the
/// typed picker's quick rows; the rest reach through "More fields…".
pub const ROLE_FIELDS: [&str; 14] = [
    "artist",
    "albumartist",
    "composer",
    "conductor",
    "lyricist",
    "arranger",
    "producer",
    "director",
    "engineer",
    "mixer",
    "remixer",
    "djmixer",
    "performer",
    "maincredit",
];

/// The evidence-ranked ~18 typed-picker quick rows (owner-corpus frequency
/// first, then the competitive-audit ranking). Every other whitelisted
/// field stays reachable via "More fields…" and raw JSON.
pub const PICKER_FIELDS: &[&str] = &[
    "rating",
    "playcount",
    "lastplayed",
    "genre",
    "loved",
    "dateadded",
    "daterated",
    "dateloved",
    "duration",
    "albumrating",
    "albumloved",
    "albumlastplayed",
    "title",
    "album",
    "artist",
    "albumartist",
    "year",
    "releasetype",
];

/// How a field name resolved against the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A static column (tier 1).
    Column(&'static FieldDef),
    /// One of the 14 artist roles (tier 2) — multi-valued, text-class.
    Role,
    /// A runtime-discovered tag (tier 3) — multi-valued, text-class.
    Tag,
}

impl FieldKind {
    /// The class driving value shapes: roles/tags behave as text.
    pub fn class(self) -> FieldClass {
        match self {
            FieldKind::Column(def) => def.class,
            FieldKind::Role | FieldKind::Tag => FieldClass::Text,
        }
    }

    /// Multi-valued JSON fields (tags/roles) — `inTheRange` is rejected on
    /// these at refresh (`criteria_sql.go` `rangeExpr`).
    pub fn is_multi_valued(self) -> bool {
        matches!(self, FieldKind::Role | FieldKind::Tag)
    }
}

/// The three-tier field whitelist: static columns + roles (compile-time,
/// version-pinned) + tags discovered live via `GET /api/tag`.
#[derive(Debug, Clone, Default)]
pub struct FieldRegistry {
    /// Distinct `tagName`s from tag discovery, lowercased. Empty when
    /// discovery hasn't run or failed (validation hedges its copy then).
    pub discovered_tags: Vec<String>,
}

impl FieldRegistry {
    /// Registry with the tags every Navidrome maps by default — the
    /// pre-discovery baseline so `genre`/`mood`/`releasetype` validate even
    /// before (or without) a successful `GET /api/tag`.
    pub fn with_default_tags() -> Self {
        Self {
            // `recordingdate` is deliberately absent — it aliases to the
            // scalar `date` COLUMN (`resolve_field_alias`), not a tag;
            // listing it here mislabels it multi-valued (false "multi-value"
            // Save block on ranges, D5 on presence).
            discovered_tags: vec![
                "genre".to_owned(),
                "mood".to_owned(),
                "grouping".to_owned(),
                "releasetype".to_owned(),
                "recordlabel".to_owned(),
                "media".to_owned(),
            ],
        }
    }

    /// Merge live-discovered tag names (lowercased, deduped).
    pub fn merge_discovered_tags<I: IntoIterator<Item = String>>(&mut self, tags: I) {
        for tag in tags {
            let tag = tag.to_lowercase();
            if !self.discovered_tags.contains(&tag) {
                self.discovered_tags.push(tag);
            }
        }
    }

    /// Resolve a field name (server-style: lowercased) against the tiers.
    /// Server-registered aliases resolve to their canonical field FIRST, so
    /// the CLASS matches the server (`fields.go`): `albumtype` is the
    /// `releasetype` TAG (multi-valued — presence-ok, range-rejected), and
    /// `recordingdate` is the scalar `date` COLUMN (range-ok, presence
    /// rejected). Classifying them by their surface name mislabels both and
    /// produces false Save blocks + D5 false-negatives.
    pub fn lookup(&self, name: &str) -> Option<FieldKind> {
        let lowered = name.to_lowercase();
        let key = resolve_field_alias(&lowered);
        if let Some(def) = STATIC_FIELDS.iter().find(|d| d.name == key) {
            return Some(FieldKind::Column(def));
        }
        if ROLE_FIELDS.contains(&key) {
            return Some(FieldKind::Role);
        }
        if self.discovered_tags.iter().any(|t| t.as_str() == key) {
            return Some(FieldKind::Tag);
        }
        None
    }

    /// If `field` resolves to a TAG (directly or via an alias like
    /// `albumtype`→`releasetype`), its canonical tag name — the key into
    /// `TagDiscovery.values_by_tag`. `None` for columns and roles, whose
    /// values are free text, not a bounded library set.
    pub fn resolved_tag_name(&self, field: &str) -> Option<String> {
        let lowered = field.to_lowercase();
        match self.lookup(&lowered) {
            Some(FieldKind::Tag) => Some(resolve_field_alias(&lowered).to_owned()),
            Some(FieldKind::Column(_) | FieldKind::Role) | None => None,
        }
    }

    /// Whether presence ops (`isMissing`/`isPresent`) are valid on `name`
    /// under `caps`: tags/roles everywhere; nullable columns only on ≥0.63
    /// (`criteria_sql.go` `missingExpr` — non-nullable columns are rejected
    /// by its default arm on EVERY version, and would save fine then
    /// hard-fail refresh: the D5 permanently-empty-playlist class).
    pub fn presence_ops_valid(&self, name: &str, caps: &ServerCaps) -> bool {
        match self.lookup(name) {
            Some(FieldKind::Role | FieldKind::Tag) => true,
            Some(FieldKind::Column(def)) => def.nullable && caps.nullable_column_presence_ops,
            None => false,
        }
    }
}

// =========================================================================
// Server capabilities
// =========================================================================

/// Version-derived capability flags. ALL false until a version is fetched
/// or when it is unparseable — conservative: feature-HIDDEN, never
/// feature-enabled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerCaps {
    pub version: Option<(u32, u32, u32)>,
    /// Rules read/write via native REST. ≥0.61.0.
    pub rules_via_rest: bool,
    /// PUT preserves unsent columns (sent-cols diff, #5541/#5542). ≥0.62.0.
    /// 0.61 full-replaces rules on any PUT (the read-merge guard in
    /// `update_playlist` covers renames there).
    pub rules_put_preserves_unsent: bool,
    /// `PUT {"sync": false}` detaches a file-backed playlist. ≥0.62.0 —
    /// verified ABSENT in v0.61.0 (no sent-cols machinery at all).
    pub sync_via_put: bool,
    /// isMissing/isPresent on Nullable COLUMNS. ≥0.63.0 (the Nullable
    /// metadata + missingExpr arm landed there; tags/roles work on every
    /// version).
    pub nullable_column_presence_ops: bool,
    /// PUT with changed rules nils EvaluatedAt (rest_adapter.go:138).
    /// UNRELEASED as of 2026-07-18 — commit 85132240 is in NO tag
    /// (verified absent from v0.62.0 AND v0.63.2; only the CREATE-path nil
    /// is released). Pinned to a ≥0.64 floor; FALSE for every released
    /// version including the owner's 0.63.2.
    pub put_nils_evaluated_at: bool,
    /// Per-playlist refreshDelay — same unreleased commit, same ≥0.64 pin.
    /// Substrate-preserved only until then.
    pub per_playlist_refresh_delay: bool,
}

impl ServerCaps {
    /// Parse a Navidrome `serverVersion` string ("0.63.2", "0.63.2 (hash)",
    /// …). Garbage ⇒ `version: None` ⇒ every capability false.
    pub fn from_version_str(version: &str) -> Self {
        let Some(v) = parse_semver_prefix(version) else {
            return Self::default();
        };
        Self {
            version: Some(v),
            rules_via_rest: v >= (0, 61, 0),
            rules_put_preserves_unsent: v >= (0, 62, 0),
            sync_via_put: v >= (0, 62, 0),
            nullable_column_presence_ops: v >= (0, 63, 0),
            put_nils_evaluated_at: v >= (0, 64, 0),
            per_playlist_refresh_delay: v >= (0, 64, 0),
        }
    }
}

/// Extract a leading `major.minor.patch` triple from a version string,
/// tolerating a leading `v` and trailing junk (" (hash)", "-SNAPSHOT").
fn parse_semver_prefix(version: &str) -> Option<(u32, u32, u32)> {
    let trimmed = version.trim().trim_start_matches('v');
    let numeric: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = numeric.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

// =========================================================================
// The rules model
// =========================================================================

/// Top-level conjunction polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conjunction {
    All,
    Any,
}

impl Conjunction {
    pub fn wire_key(self) -> &'static str {
        match self {
            Conjunction::All => "all",
            Conjunction::Any => "any",
        }
    }
}

/// One rule leaf: `{ "<op>": { "<field>": <value> } }`.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleLeaf {
    pub operator: RuleOperator,
    /// The operator key AS WRITTEN in the source JSON (the server matches
    /// case-insensitively) — re-emitted verbatim for round-trip fidelity.
    /// Freshly built leaves use the canonical `wire_key()`.
    pub original_key: String,
    pub field: String,
    pub value: Value,
}

impl RuleLeaf {
    pub fn new(operator: RuleOperator, field: impl Into<String>, value: Value) -> Self {
        Self {
            operator,
            original_key: operator.wire_key().to_owned(),
            field: field.into(),
            value,
        }
    }
}

/// A node in the criteria tree.
#[derive(Debug, Clone, PartialEq)]
pub enum CriteriaNode {
    Leaf(RuleLeaf),
    /// Nested conjunction — unlimited depth in the MODEL (the flat-plus-one
    /// cap is a FORM limitation, not a model one).
    Group(CriteriaGroup),
    /// Unrecognized operator key (or a malformed item) — preserved
    /// verbatim, rendered read-only, never dropped.
    Unknown(Value),
}

/// A conjunction group with its child nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct CriteriaGroup {
    pub conjunction: Conjunction,
    /// The conjunction key as written ("all"/"Any"/…) — Go's encoding/json
    /// matches struct keys case-insensitively at the top level, so parse
    /// tolerates any casing and re-emits it verbatim.
    pub original_key: String,
    pub nodes: Vec<CriteriaNode>,
}

impl CriteriaGroup {
    pub fn new(conjunction: Conjunction) -> Self {
        Self {
            conjunction,
            original_key: conjunction.wire_key().to_owned(),
            nodes: Vec::new(),
        }
    }
}

/// One sort key in the effective (typed) view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    pub field: String,
    pub descending: bool,
}

/// The sort clause. Navidrome's real wire grammar is ONE comma-separated
/// string with optional `+`/`-` prefixes (`model/criteria/sort.go`), plus
/// a legacy top-level `order` key that inverts EVERY key when `"desc"`.
///
/// `raw` preserves the source string byte-faithfully until the user edits
/// the sort ([`SmartRules::edit_sort`] — the only canonicalization point).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SortSpec {
    /// The source `sort` string, verbatim. `None` after an edit (canonical
    /// serialization from `keys` takes over) or when absent from source.
    pub raw: Option<String>,
    /// The typed keys. For an UNEDITED spec these mirror `raw` WITHOUT the
    /// legacy-order fold (use [`SmartRules::effective_sort_keys`] for the
    /// folded view the form renders).
    pub keys: Vec<SortKey>,
}

/// Parse a sort string into keys (sort.go `OrderByFields`, minus the
/// registry drop — validation warns on unknown keys instead of dropping).
fn parse_sort_string(sort: &str) -> Vec<SortKey> {
    sort.split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let (descending, field) = match part.strip_prefix('-') {
                Some(rest) => (true, rest.trim()),
                None => (false, part.strip_prefix('+').unwrap_or(part).trim()),
            };
            Some(SortKey {
                field: field.to_lowercase(),
                descending,
            })
        })
        .collect()
}

/// Serialize keys to the canonical order-free comma string (`-` prefixes
/// only; ascending keys go bare).
fn serialize_sort_keys(keys: &[SortKey]) -> String {
    keys.iter()
        .map(|k| {
            if k.descending {
                format!("-{}", k.field)
            } else {
                k.field.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The typed parse of one `rules` JSON object. See the module docs for the
/// round-trip contract.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartRules {
    /// The root conjunction, or `None` when the source had neither `all`
    /// nor `any` (kept absent on re-serialize until a rule is added).
    pub root: Option<CriteriaGroup>,
    /// Both `all` AND `any` present at top level — the server rejects this
    /// at decode; only reachable via raw JSON. The second group is
    /// preserved in `extra`.
    pub both_conjunctions: bool,
    pub sort: SortSpec,
    /// The legacy top-level `order` key, typed — NEVER left to the
    /// substrate. `"desc"` inverts every sort key server-side; preserved
    /// verbatim on round-trip, folded per-key only on the first sort edit.
    pub legacy_order: Option<String>,
    pub limit: Option<u64>,
    pub limit_percent: Option<u64>,
    pub offset: Option<u64>,
    /// Preserved, not typed-edited in v1 (per-playlist refreshDelay is
    /// unreleased server-side anyway).
    pub refresh_delay: Option<String>,
    /// Every unrecognized top-level key, byte-preserved.
    pub extra: Map<String, Value>,
}

impl Default for SmartRules {
    fn default() -> Self {
        Self::new_empty()
    }
}

impl SmartRules {
    /// A fresh, empty rule set (root `all` group with no nodes — validation
    /// blocks Preview/Save until a rule lands, and `to_value` is never
    /// called on it by any network path).
    pub fn new_empty() -> Self {
        Self {
            root: Some(CriteriaGroup::new(Conjunction::All)),
            both_conjunctions: false,
            sort: SortSpec::default(),
            legacy_order: None,
            limit: None,
            limit_percent: None,
            offset: None,
            refresh_delay: None,
            extra: Map::new(),
        }
    }

    /// Parse a raw `rules` value. Infallible by design: whatever the typed
    /// model doesn't understand is preserved (`Unknown` nodes, `extra`
    /// keys) so nothing is ever dropped; validation reports the problems.
    pub fn parse(value: &Value) -> Self {
        let Some(obj) = value.as_object() else {
            // Not an object at all — preserve it wholesale under a
            // sentinel key so to_value can reproduce it.
            let mut rules = Self::new_empty();
            rules.root = None;
            if !value.is_null() {
                rules
                    .extra
                    .insert(NON_OBJECT_SENTINEL.to_owned(), value.clone());
            }
            return rules;
        };

        let mut rules = Self {
            root: None,
            both_conjunctions: false,
            sort: SortSpec::default(),
            legacy_order: None,
            limit: None,
            limit_percent: None,
            offset: None,
            refresh_delay: None,
            extra: Map::new(),
        };

        for (key, val) in obj {
            match key.to_lowercase().as_str() {
                "all" | "any" if rules.root.is_none() => {
                    rules.root = Some(parse_group(key, val));
                }
                "all" | "any" => {
                    // Second top-level conjunction — server rejects at
                    // decode. Preserve + flag for the Error diagnostic.
                    rules.both_conjunctions = true;
                    rules.extra.insert(key.clone(), val.clone());
                }
                "sort" => match val.as_str() {
                    Some(s) => {
                        rules.sort = SortSpec {
                            raw: Some(s.to_owned()),
                            keys: parse_sort_string(s),
                        };
                    }
                    None => {
                        rules.extra.insert(key.clone(), val.clone());
                    }
                },
                "order" => match val.as_str() {
                    Some(s) => rules.legacy_order = Some(s.to_owned()),
                    None => {
                        rules.extra.insert(key.clone(), val.clone());
                    }
                },
                "limit" => match val.as_u64() {
                    Some(n) => rules.limit = Some(n),
                    None => {
                        rules.extra.insert(key.clone(), val.clone());
                    }
                },
                "limitpercent" => match val.as_u64() {
                    Some(n) => rules.limit_percent = Some(n),
                    None => {
                        rules.extra.insert(key.clone(), val.clone());
                    }
                },
                "offset" => match val.as_u64() {
                    Some(n) => rules.offset = Some(n),
                    None => {
                        rules.extra.insert(key.clone(), val.clone());
                    }
                },
                "refreshdelay" => match val.as_str() {
                    Some(s) => rules.refresh_delay = Some(s.to_owned()),
                    None => {
                        rules.extra.insert(key.clone(), val.clone());
                    }
                },
                _ => {
                    rules.extra.insert(key.clone(), val.clone());
                }
            }
        }
        rules
    }

    /// Serialize back to the wire `Value`. A parse → no-edit → to_value
    /// cycle reproduces the input Value exactly (test-pinned, including
    /// `order:"desc"` un-folded and original key spellings).
    pub fn to_value(&self) -> Value {
        let mut obj = Map::new();
        if let Some(root) = &self.root
            && (!root.nodes.is_empty() || !self.extra.contains_key(NON_OBJECT_SENTINEL))
        {
            obj.insert(root.original_key.clone(), serialize_nodes(&root.nodes));
        }
        if let Some(raw) = &self.sort.raw {
            obj.insert("sort".to_owned(), Value::String(raw.clone()));
        } else if !self.sort.keys.is_empty() {
            obj.insert(
                "sort".to_owned(),
                Value::String(serialize_sort_keys(&self.sort.keys)),
            );
        }
        if let Some(order) = &self.legacy_order {
            obj.insert("order".to_owned(), Value::String(order.clone()));
        }
        if let Some(limit) = self.limit {
            obj.insert("limit".to_owned(), Value::from(limit));
        }
        if let Some(pct) = self.limit_percent {
            obj.insert("limitPercent".to_owned(), Value::from(pct));
        }
        if let Some(offset) = self.offset {
            obj.insert("offset".to_owned(), Value::from(offset));
        }
        if let Some(delay) = &self.refresh_delay {
            obj.insert("refreshDelay".to_owned(), Value::String(delay.clone()));
        }
        for (k, v) in &self.extra {
            if k == NON_OBJECT_SENTINEL {
                return v.clone();
            }
            // Preserve the source's key spelling for limitPercent /
            // refreshDelay-style keys that landed in extra unparsed.
            obj.insert(k.clone(), v.clone());
        }
        Value::Object(obj)
    }

    /// The sort keys the FORM renders: the typed keys with the legacy
    /// `order:"desc"` fold applied (every key inverted, per sort.go).
    pub fn effective_sort_keys(&self) -> Vec<SortKey> {
        let invert = self
            .legacy_order
            .as_deref()
            .is_some_and(|o| o.trim().eq_ignore_ascii_case("desc"));
        self.sort
            .keys
            .iter()
            .map(|k| SortKey {
                field: k.field.clone(),
                descending: k.descending != invert,
            })
            .collect()
    }

    /// Replace the sort with edited keys — THE canonicalization point: the
    /// raw string is dropped, the legacy `order` is folded away (the keys
    /// passed in are the effective view), and serialization goes canonical.
    pub fn edit_sort(&mut self, keys: Vec<SortKey>) {
        self.sort = SortSpec { raw: None, keys };
        self.legacy_order = None;
    }

    /// All `inPlaylist`/`notInPlaylist` ids referenced anywhere in the
    /// tree (raw-JSON nesting included).
    pub fn referenced_playlist_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if let Some(root) = &self.root {
            collect_playlist_refs(&root.nodes, &mut ids);
        }
        ids
    }

    /// Maximum nesting depth of the tree (0 = leaves only at root). The
    /// typed form renders depth ≤1 editable; deeper trees lock to
    /// read-only + edit-as-JSON.
    pub fn max_depth(&self) -> usize {
        fn depth(nodes: &[CriteriaNode]) -> usize {
            nodes
                .iter()
                .map(|n| match n {
                    CriteriaNode::Group(g) => 1 + depth(&g.nodes),
                    CriteriaNode::Leaf(_) | CriteriaNode::Unknown(_) => 0,
                })
                .max()
                .unwrap_or(0)
        }
        self.root.as_ref().map_or(0, |r| depth(&r.nodes))
    }
}

/// Sentinel `extra` key holding a non-object source value verbatim.
const NON_OBJECT_SENTINEL: &str = "\u{0}nokkvi-non-object-rules";

fn parse_group(original_key: &str, value: &Value) -> CriteriaGroup {
    let conjunction = if original_key.to_lowercase() == "any" {
        Conjunction::Any
    } else {
        Conjunction::All
    };
    let mut group = CriteriaGroup {
        conjunction,
        original_key: original_key.to_owned(),
        nodes: Vec::new(),
    };
    let Some(items) = value.as_array() else {
        // Conjunction value isn't an array — preserve wholesale.
        group.nodes.push(CriteriaNode::Unknown(value.clone()));
        return group;
    };
    for item in items {
        group.nodes.extend(parse_item(item));
    }
    group
}

/// Parse one conjunction item. The server iterates ALL keys of each item
/// object (json.go `unmarshalConjunctionType`), so a multi-key item yields
/// multiple nodes here too.
fn parse_item(item: &Value) -> Vec<CriteriaNode> {
    let Some(obj) = item.as_object() else {
        return vec![CriteriaNode::Unknown(item.clone())];
    };
    if obj.is_empty() {
        return vec![CriteriaNode::Unknown(item.clone())];
    }
    let mut nodes = Vec::new();
    for (key, val) in obj {
        let lower = key.to_lowercase();
        if lower == "all" || lower == "any" {
            nodes.push(CriteriaNode::Group(parse_group(key, val)));
            continue;
        }
        match RuleOperator::from_wire_key(key) {
            Some(op) => {
                // The operand must be a one-field object; anything else is
                // preserved as Unknown (the server would reject it).
                let leaf = val.as_object().and_then(|fields| {
                    if fields.len() == 1 {
                        fields.iter().next().map(|(field, value)| RuleLeaf {
                            operator: op,
                            original_key: key.clone(),
                            field: field.clone(),
                            value: value.clone(),
                        })
                    } else {
                        None
                    }
                });
                match leaf {
                    Some(leaf) => nodes.push(CriteriaNode::Leaf(leaf)),
                    None => {
                        let mut single = Map::new();
                        single.insert(key.clone(), val.clone());
                        nodes.push(CriteriaNode::Unknown(Value::Object(single)));
                    }
                }
            }
            None => {
                let mut single = Map::new();
                single.insert(key.clone(), val.clone());
                nodes.push(CriteriaNode::Unknown(Value::Object(single)));
            }
        }
    }
    nodes
}

fn serialize_nodes(nodes: &[CriteriaNode]) -> Value {
    Value::Array(
        nodes
            .iter()
            .map(|node| match node {
                CriteriaNode::Leaf(leaf) => {
                    let mut inner = Map::new();
                    inner.insert(leaf.field.clone(), leaf.value.clone());
                    let mut outer = Map::new();
                    outer.insert(leaf.original_key.clone(), Value::Object(inner));
                    Value::Object(outer)
                }
                CriteriaNode::Group(group) => {
                    let mut outer = Map::new();
                    outer.insert(group.original_key.clone(), serialize_nodes(&group.nodes));
                    Value::Object(outer)
                }
                CriteriaNode::Unknown(value) => value.clone(),
            })
            .collect(),
    )
}

fn collect_playlist_refs(nodes: &[CriteriaNode], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            CriteriaNode::Leaf(leaf)
                if matches!(
                    leaf.operator,
                    RuleOperator::InPlaylist | RuleOperator::NotInPlaylist
                ) =>
            {
                // The operand for playlist refs is `{"id": "<playlist-id>"}`
                // (field == "id", value == the id string).
                if leaf.field == "id"
                    && let Some(id) = leaf.value.as_str()
                {
                    out.push(id.to_owned());
                }
            }
            CriteriaNode::Group(group) => collect_playlist_refs(&group.nodes, out),
            CriteriaNode::Leaf(_) | CriteriaNode::Unknown(_) => {}
        }
    }
}

// =========================================================================
// Validation
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Blocks Preview AND Save.
    Error,
    /// Dimmed, never blocks.
    Warning,
}

/// Where a diagnostic anchors in the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticLocation {
    /// The whole rule set (empty root, both-conjunctions, …).
    Root,
    /// A rule node, addressed by its index path from the root group.
    Rule(Vec<usize>),
    /// A sort key (index into the effective keys).
    Sort(usize),
    Limit,
    Offset,
    /// The session's playlist-name input.
    Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub location: DiagnosticLocation,
    pub message: String,
}

impl Diagnostic {
    fn error(location: DiagnosticLocation, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            location,
            message: message.into(),
        }
    }

    fn warning(location: DiagnosticLocation, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            location,
            message: message.into(),
        }
    }
}

/// Everything validation needs beyond the rules themselves.
#[derive(Debug, Clone, Default)]
pub struct ValidationContext<'a> {
    pub caps: ServerCaps,
    /// The loaded playlists list as `(id, name)` — the `inPlaylist`
    /// resolution + duplicate-name source.
    pub playlists: &'a [(String, String)],
    /// FALSE until the session-open list fetch resolves. While false, the
    /// dangling-ref and duplicate-name diagnostics are SUPPRESSED — an
    /// unloaded list is empty, not stale, and "references a playlist that
    /// no longer exists" against it would be a false alarm.
    pub playlists_loaded: bool,
    /// The session target's playlist id (edit mode) — self-reference
    /// detection. A self-referencing smart playlist sends the server's
    /// child-refresh into unbounded recursion (refreshChildPlaylists has no
    /// cycle guard — verified), so this is an ERROR.
    pub session_target_id: Option<&'a str>,
    /// Known library ids for `library_id` value checks.
    pub known_library_ids: &'a [i32],
    /// Tag discovery failed (or never ran with live data) — hedge the
    /// unknown-field copy.
    pub discovery_failed: bool,
    /// The session's playlist-name input (create AND edit) — empty blocks
    /// Save; duplicates warn.
    pub name: &'a str,
}

/// Whether a presence-op operand coerces to bool the way the server's
/// `ToBool` (`model/criteria/json.go`) does: a real bool, a
/// `strconv.ParseBool`-parseable string, or a JSON number exactly 0 or 1.
/// Anything else fails the server's `value.(bool)` assertion at refresh.
fn is_bool_coercible(v: &Value) -> bool {
    if v.is_boolean() {
        return true;
    }
    if let Some(s) = v.as_str() {
        // strconv.ParseBool's exact accepted set.
        return matches!(
            s,
            "1" | "t"
                | "T"
                | "TRUE"
                | "true"
                | "True"
                | "0"
                | "f"
                | "F"
                | "FALSE"
                | "false"
                | "False"
        );
    }
    if let Some(n) = v.as_i64() {
        return n == 0 || n == 1;
    }
    false
}

/// `YYYY-MM-DD` with real calendar bounds-checking (months 1-12, days
/// 1-31 with per-month caps; leap years honored). The server does NO date
/// parsing for before/after — the literal lands in a SQL string compare —
/// so a malformed date saves fine then matches garbage: the D5 class this
/// deterministic client check exists to stop. (No relative date forms
/// exist server-side — verified: only inTheLast/notInTheLast take day
/// counts.)
pub fn is_valid_date_literal(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> { s.get(r)?.parse().ok() };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    if !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => unreachable!(),
    };
    day <= max_day
}

/// Validate a rule set. Errors block Preview/Save; warnings render dimmed.
pub fn validate(
    rules: &SmartRules,
    registry: &FieldRegistry,
    ctx: &ValidationContext<'_>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    // --- Root shape ------------------------------------------------------
    if rules.both_conjunctions {
        out.push(Diagnostic::error(
            DiagnosticLocation::Root,
            "'all' and 'any' cannot both be used at the top level — nest one inside the other",
        ));
    }
    match &rules.root {
        None => out.push(Diagnostic::error(
            DiagnosticLocation::Root,
            "No rules — add at least one",
        )),
        Some(root) if root.nodes.is_empty() => out.push(Diagnostic::error(
            DiagnosticLocation::Root,
            "No rules — add at least one",
        )),
        Some(root) => validate_nodes(&root.nodes, &mut Vec::new(), registry, ctx, &mut out),
    }

    // --- Sort -------------------------------------------------------------
    for (i, key) in rules.sort.keys.iter().enumerate() {
        if key.field == "random" {
            out.push(Diagnostic::warning(
                DiagnosticLocation::Sort(i),
                "Random sort is known to spam server logs on SQLite — consider a limit and a different sort",
            ));
        } else if registry.lookup(&key.field).is_none() {
            out.push(Diagnostic::warning(
                DiagnosticLocation::Sort(i),
                format!(
                    "'{}' is not a sortable field on this server — the server will fall back to title",
                    key.field
                ),
            ));
        }
    }

    // --- Limit / offset ---------------------------------------------------
    if let Some(pct) = rules.limit_percent
        && !(1..=100).contains(&pct)
    {
        out.push(Diagnostic::error(
            DiagnosticLocation::Limit,
            "Limit percent must be between 1 and 100",
        ));
    }
    if rules.offset.is_some() && rules.limit.is_none() && rules.limit_percent.is_none() {
        out.push(Diagnostic::warning(
            DiagnosticLocation::Offset,
            "Offset only applies together with a limit — the server ignores it otherwise",
        ));
    }

    // --- Name -------------------------------------------------------------
    if ctx.name.trim().is_empty() {
        out.push(Diagnostic::error(
            DiagnosticLocation::Name,
            "Name cannot be empty",
        ));
    } else if ctx.playlists_loaded {
        let existing = ctx
            .playlists
            .iter()
            .filter(|(id, _)| Some(id.as_str()) != ctx.session_target_id)
            .map(|(_, name)| name.as_str());
        if crate::services::api::playlists::duplicate_playlist_name(ctx.name, existing).is_some() {
            out.push(Diagnostic::warning(
                DiagnosticLocation::Name,
                format!("A playlist named \"{}\" already exists", ctx.name.trim()),
            ));
        }
    }

    out
}

fn validate_nodes(
    nodes: &[CriteriaNode],
    path: &mut Vec<usize>,
    registry: &FieldRegistry,
    ctx: &ValidationContext<'_>,
    out: &mut Vec<Diagnostic>,
) {
    for (i, node) in nodes.iter().enumerate() {
        path.push(i);
        match node {
            CriteriaNode::Group(group) => {
                if group.nodes.is_empty() {
                    out.push(Diagnostic::error(
                        DiagnosticLocation::Rule(path.clone()),
                        "Empty group — add a rule or remove it",
                    ));
                } else {
                    validate_nodes(&group.nodes, path, registry, ctx, out);
                }
            }
            CriteriaNode::Leaf(leaf) => validate_leaf(leaf, path, registry, ctx, out),
            // Unknown operators from a NEWER server round-trip untouched;
            // the server that emitted them accepts them back.
            CriteriaNode::Unknown(_) => {}
        }
        path.pop();
    }
}

fn validate_leaf(
    leaf: &RuleLeaf,
    path: &[usize],
    registry: &FieldRegistry,
    ctx: &ValidationContext<'_>,
    out: &mut Vec<Diagnostic>,
) {
    let location = || DiagnosticLocation::Rule(path.to_vec());
    let kind = registry.lookup(&leaf.field);

    // Playlist refs don't name a library field — their operand is
    // {"id": "<playlist-id>"}.
    if matches!(
        leaf.operator,
        RuleOperator::InPlaylist | RuleOperator::NotInPlaylist
    ) {
        if leaf.field != "id" || !leaf.value.is_string() {
            out.push(Diagnostic::error(
                location(),
                "Playlist rules need a playlist id (\"id\": \"…\")",
            ));
            return;
        }
        let id = leaf.value.as_str().unwrap_or_default();
        if Some(id) == ctx.session_target_id {
            // Verified: refreshChildPlaylists recursively refreshes
            // referenced playlists with NO cycle guard — a self-reference
            // is unbounded server-side recursion.
            //
            // KNOWN GAP (finding 3, unfixed): an INDIRECT cycle (A→B, B→A)
            // slips past this direct check and crashes the server the same
            // way. The client can't reliably catch it — it only holds
            // (id, name) pairs, not other playlists' rules, and a back-ref
            // can be added after A is saved (TOCTOU). The durable fix is a
            // visited-set in Navidrome's refreshChildPlaylists; any client
            // is exposed until then.
            out.push(Diagnostic::error(
                location(),
                "This rule references the playlist itself — the server cannot evaluate a self-referencing playlist",
            ));
        } else if ctx.playlists_loaded && !ctx.playlists.iter().any(|(pid, _)| pid == id) {
            out.push(Diagnostic::warning(
                location(),
                "References a playlist that no longer exists — matches nothing",
            ));
        }
        return;
    }

    // Unknown field: the server persists it and the playlist sits
    // permanently empty with only a server-side log (D5).
    let Some(kind) = kind else {
        let message = if ctx.discovery_failed || ctx.caps.version.is_none() {
            format!(
                "'{}' may not be a field on this server — the playlist will save but may match nothing",
                leaf.field
            )
        } else {
            format!(
                "'{}' is not a known field on this server — the playlist will save but may match nothing",
                leaf.field
            )
        };
        out.push(Diagnostic::warning(location(), message));
        return;
    };

    // Field exists in nokkvi's registry but was added to Navidrome AFTER
    // this server's version — the server persists the rule and then matches
    // nothing (D5). Only fires when the version is KNOWN and below the floor;
    // an unknown version stays quiet (the unknown-field arm above hedges).
    if let FieldKind::Column(def) = kind
        && let (Some(floor), Some(v)) = (def.min_server, ctx.caps.version)
        && v < floor
    {
        out.push(Diagnostic::error(
            location(),
            format!(
                "'{}' needs Navidrome {}.{}.{}+ — this server ({}.{}.{}) would save the rule but match nothing",
                leaf.field, floor.0, floor.1, floor.2, v.0, v.1, v.2
            ),
        ));
        return;
    }

    match leaf.operator {
        RuleOperator::InTheRange => {
            if kind.is_multi_valued() {
                // rangeExpr rejects tag/role ranges at REFRESH — saves
                // fine, then permanently-empty (D5).
                out.push(Diagnostic::error(
                    location(),
                    format!(
                        "'{}' is a multi-value field — ranges aren't supported on it",
                        leaf.field
                    ),
                ));
                return;
            }
            let pair_ok = leaf.value.as_array().is_some_and(|arr| arr.len() == 2);
            if !pair_ok {
                out.push(Diagnostic::error(
                    location(),
                    "Range needs exactly a [min, max] pair",
                ));
                return;
            }
            if kind.class() == FieldClass::Date
                && let Some(arr) = leaf.value.as_array()
            {
                for bound in arr {
                    if !bound.as_str().is_some_and(is_valid_date_literal) {
                        out.push(Diagnostic::error(location(), "Dates must be YYYY-MM-DD"));
                        break;
                    }
                }
            }
        }
        RuleOperator::Before | RuleOperator::After => {
            if !leaf.value.as_str().is_some_and(is_valid_date_literal) {
                out.push(Diagnostic::error(location(), "Dates must be YYYY-MM-DD"));
            }
        }
        RuleOperator::InTheLast | RuleOperator::NotInTheLast => {
            // The server ParseInts the value at refresh — a non-integer
            // saves fine then hard-fails evaluation (D5).
            let integer_ok = leaf.value.as_i64().is_some()
                || leaf
                    .value
                    .as_str()
                    .is_some_and(|s| s.trim().parse::<i64>().is_ok());
            if !integer_ok {
                out.push(Diagnostic::error(
                    location(),
                    "Needs a whole number of days",
                ));
            }
        }
        RuleOperator::IsMissing | RuleOperator::IsPresent => {
            if !registry.presence_ops_valid(&leaf.field, &ctx.caps) {
                let message = match kind {
                    FieldKind::Column(def) if def.nullable => {
                        // Valid on ≥0.63 only — the caps said no.
                        format!(
                            "'{}' supports presence checks only on Navidrome 0.63+ — this server would fail every refresh",
                            leaf.field
                        )
                    }
                    _ => format!(
                        "'{}' doesn't support presence checks — the playlist would save, then every refresh fails and it stays empty",
                        leaf.field
                    ),
                };
                out.push(Diagnostic::error(location(), message));
            } else if !is_bool_coercible(&leaf.value) {
                // The GUI always writes a bool here, but raw-JSON / .nsp
                // import can carry a non-bool operand. The server's
                // `missingExpr` type-asserts `value.(bool)` after failing to
                // coerce it, aborting every refresh (D5).
                out.push(Diagnostic::error(
                    location(),
                    format!("'{}' presence check needs true or false", leaf.field),
                ));
            }
        }
        RuleOperator::Is | RuleOperator::IsNot => {
            if leaf.field == "library_id"
                && ctx.playlists_loaded
                && let Some(id) = leaf.value.as_i64()
                && !ctx.known_library_ids.is_empty()
                && !ctx.known_library_ids.contains(&(id as i32))
            {
                out.push(Diagnostic::warning(
                    location(),
                    "References a library that no longer exists — matches nothing",
                ));
            }
        }
        RuleOperator::Gt
        | RuleOperator::Lt
        | RuleOperator::Contains
        | RuleOperator::NotContains
        | RuleOperator::StartsWith
        | RuleOperator::EndsWith
        | RuleOperator::InPlaylist
        | RuleOperator::NotInPlaylist => {}
    }
}

// =========================================================================
// Seed presets
// =========================================================================

/// One seeded preset — a named `SmartRules` constructor. Purely seeds:
/// fully editable after insertion, never a ceiling.
pub struct PresetDef {
    pub name: &'static str,
    pub description: &'static str,
    pub build: fn() -> SmartRules,
}

fn leaf(op: RuleOperator, field: &str, value: Value) -> CriteriaNode {
    CriteriaNode::Leaf(RuleLeaf::new(op, field, value))
}

fn preset(
    conjunction: Conjunction,
    nodes: Vec<CriteriaNode>,
    sort: &str,
    descending: bool,
    limit: u64,
) -> SmartRules {
    let mut rules = SmartRules::new_empty();
    let mut root = CriteriaGroup::new(conjunction);
    root.nodes = nodes;
    rules.root = Some(root);
    rules.sort = SortSpec {
        raw: None,
        keys: vec![SortKey {
            field: sort.to_owned(),
            descending,
        }],
    };
    rules.limit = Some(limit);
    rules
}

/// The 5 seeded preset trees, mirroring the owner's proven recipes.
/// Comeback Queue is the tiered any-of-alls — it exercises the
/// flat-plus-one group render.
pub const SEED_PRESETS: &[PresetDef] = &[
    PresetDef {
        name: "Never Played",
        description: "Everything with zero plays, oldest additions first",
        build: || {
            preset(
                Conjunction::All,
                vec![leaf(RuleOperator::Is, "playcount", Value::from(0))],
                "dateadded",
                false,
                500,
            )
        },
    },
    PresetDef {
        name: "Heavy Rotation",
        description: "Your most-played tracks",
        build: || {
            preset(
                Conjunction::All,
                vec![leaf(RuleOperator::Gt, "playcount", Value::from(5))],
                "playcount",
                true,
                200,
            )
        },
    },
    PresetDef {
        name: "Forgotten Loves",
        description: "Loved tracks you haven't played in 90 days",
        build: || {
            preset(
                Conjunction::All,
                vec![
                    leaf(RuleOperator::Is, "loved", Value::Bool(true)),
                    leaf(RuleOperator::NotInTheLast, "lastplayed", Value::from(90)),
                ],
                "dateloved",
                true,
                100,
            )
        },
    },
    PresetDef {
        name: "Comeback Queue",
        description: "High-rated tracks on a per-tier cooldown — higher ratings recur sooner",
        build: || {
            let tier = |rating: i64, days: i64| {
                let mut group = CriteriaGroup::new(Conjunction::All);
                group.nodes = vec![
                    leaf(RuleOperator::Is, "rating", Value::from(rating)),
                    leaf(RuleOperator::NotInTheLast, "lastplayed", Value::from(days)),
                ];
                CriteriaNode::Group(group)
            };
            let loved_tier = {
                let mut group = CriteriaGroup::new(Conjunction::All);
                group.nodes = vec![
                    leaf(RuleOperator::Is, "loved", Value::Bool(true)),
                    leaf(RuleOperator::NotInTheLast, "lastplayed", Value::from(10)),
                ];
                CriteriaNode::Group(group)
            };
            preset(
                Conjunction::Any,
                vec![tier(3, 90), tier(4, 30), tier(5, 15), loved_tier],
                "lastplayed",
                false,
                1000,
            )
        },
    },
    PresetDef {
        name: "Recently Added",
        description: "The newest additions to the library",
        build: || {
            preset(
                Conjunction::All,
                vec![leaf(RuleOperator::InTheLast, "dateadded", Value::from(30))],
                "dateadded",
                true,
                500,
            )
        },
    },
];

// =========================================================================
// .nsp file envelope (import)
// =========================================================================

/// Navidrome's .nsp size cap, mirrored (`parse_nsp.go`: 100 KB LimitReader).
pub const NSP_MAX_BYTES: usize = 100 * 1024;

/// A parsed .nsp file: playlist metadata + the criteria object (the file's
/// top level IS the criteria, with name/comment/public riding alongside —
/// `parse_nsp.go` `nspFile` embeds `criteria.Criteria`).
#[derive(Debug, Clone, PartialEq)]
pub struct NspEnvelope {
    pub name: Option<String>,
    pub comment: Option<String>,
    pub public: Option<bool>,
    /// The criteria value (the source object minus name/comment/public) —
    /// wire-identical to the REST `rules` body.
    pub criteria: Value,
}

/// Parse .nsp bytes the way the server does: 100 KB cap, JSON comments
/// stripped (`jsoncommentstrip`), then the flat envelope. Error strings are
/// user-facing (the import failure toast).
pub fn parse_nsp_envelope(bytes: &[u8]) -> Result<NspEnvelope, String> {
    if bytes.len() > NSP_MAX_BYTES {
        return Err("file exceeds 100 KB".to_owned());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "couldn't parse JSON".to_owned())?;
    let stripped = strip_json_comments(text);
    let value: Value =
        serde_json::from_str(&stripped).map_err(|_| "couldn't parse JSON".to_owned())?;
    let Some(obj) = value.as_object() else {
        return Err("no rules object found".to_owned());
    };
    let has_rules = obj
        .keys()
        .any(|k| matches!(k.to_lowercase().as_str(), "all" | "any"));
    if !has_rules {
        return Err("no rules object found".to_owned());
    }
    let mut criteria = obj.clone();
    let name = criteria
        .remove("name")
        .and_then(|v| v.as_str().map(str::to_string));
    let comment = criteria
        .remove("comment")
        .and_then(|v| v.as_str().map(str::to_string));
    let public = criteria.remove("public").and_then(|v| v.as_bool());
    Ok(NspEnvelope {
        name,
        comment,
        public,
        criteria: Value::Object(criteria),
    })
}

/// Strip `//` line and `/* */` block comments OUTSIDE string literals —
/// the `jsoncommentstrip` behavior the server applies before parsing.
///
/// Operates on RAW BYTES and copies non-comment bytes verbatim: every
/// comment/string marker (`/ * " \` and newline) is ASCII, and multi-byte
/// UTF-8 continuation bytes (0x80–0xBF) never collide with them, so the
/// scan never splits a code point. Copying whole bytes (not `byte as char`)
/// keeps non-ASCII playlist names and comments intact on import.
fn strip_json_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                out.push(b);
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    // `out` is `input` minus whole ASCII comment runs, so it stays valid
    // UTF-8; the fallback keeps the fn total rather than risk a panic.
    String::from_utf8(out).unwrap_or_default()
}

// =========================================================================
// Tag discovery projection
// =========================================================================

/// Distinct tag names + per-name value lists (ordered by song count desc)
/// projected from `GET /api/tag` rows — the registry-merge + autocomplete
/// source.
#[derive(Debug, Clone, Default)]
pub struct TagDiscovery {
    pub tag_names: Vec<String>,
    pub values_by_tag: HashMap<String, Vec<String>>,
}

impl TagDiscovery {
    /// Build from raw `(tag_name, tag_value, song_count)` rows.
    pub fn from_rows<I: IntoIterator<Item = (String, String, u64)>>(rows: I) -> Self {
        let mut values: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        for (name, value, song_count) in rows {
            let name = name.to_lowercase();
            values.entry(name).or_default().push((value, song_count));
        }
        let mut tag_names: Vec<String> = values.keys().cloned().collect();
        tag_names.sort();
        let values_by_tag = values
            .into_iter()
            .map(|(name, mut vals)| {
                vals.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                (name, vals.into_iter().map(|(v, _)| v).collect())
            })
            .collect();
        Self {
            tag_names,
            values_by_tag,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Strip the .nsp envelope's name/comment for the criteria-only value
    /// (they are playlist metadata, not rules — but note the full-envelope
    /// test below shows they'd survive via `extra` anyway).
    fn criteria_of(mut v: Value) -> Value {
        if let Some(obj) = v.as_object_mut() {
            obj.remove("name");
            obj.remove("comment");
        }
        v
    }

    fn roundtrip(v: &Value) -> Value {
        SmartRules::parse(v).to_value()
    }

    // --- Round-trip fixtures from the owner's LIVE corpus -----------------

    /// comeback_queue.nsp — the tiered any-of-alls.
    #[test]
    fn roundtrip_comeback_queue_fixture() {
        let v = criteria_of(json!({
            "name": "Comeback Queue",
            "comment": "smart playlist - High-rated tracks on a per-tier cooldown; higher rating recurs sooner.",
            "any": [
                { "all": [ { "is": { "rating": 3 } }, { "notInTheLast": { "lastplayed": 90 } } ] },
                { "all": [ { "is": { "rating": 4 } }, { "notInTheLast": { "lastplayed": 30 } } ] },
                { "all": [ { "is": { "rating": 5 } }, { "notInTheLast": { "lastplayed": 15 } } ] },
                { "all": [ { "is": { "loved": true } }, { "notInTheLast": { "lastplayed": 10 } } ] }
            ],
            "sort": "lastplayed",
            "order": "asc",
            "limit": 1000
        }));
        assert_eq!(roundtrip(&v), v);

        let rules = SmartRules::parse(&v);
        let root = rules.root.as_ref().expect("root parses");
        assert_eq!(root.conjunction, Conjunction::Any);
        assert_eq!(root.nodes.len(), 4);
        assert!(
            root.nodes
                .iter()
                .all(|n| matches!(n, CriteriaNode::Group(_)))
        );
        assert_eq!(rules.max_depth(), 1, "any-of-alls = flat-plus-one");
        assert_eq!(rules.limit, Some(1000));
        assert_eq!(rules.legacy_order.as_deref(), Some("asc"));
    }

    /// movie_tv_soundtracks.nsp — multi-key sort + isNot + nested any.
    #[test]
    fn roundtrip_movie_tv_soundtracks_fixture() {
        let v = criteria_of(json!({
            "all": [
                { "any": [
                    { "is": { "genre": "Soundtrack" } },
                    { "is": { "genre": "Score" } },
                    { "contains": { "releasetype": "soundtrack" } }
                ] },
                { "isNot": { "genre": "Video Game Music" } },
                { "isNot": { "album": "ODDSAC" } },
                { "isNot": { "album": "Tangerine Reef" } }
            ],
            "sort": "album,discnumber,tracknumber",
            "order": "asc",
            "limit": 1000
        }));
        assert_eq!(roundtrip(&v), v);

        let rules = SmartRules::parse(&v);
        assert_eq!(
            rules.sort.keys,
            vec![
                SortKey {
                    field: "album".into(),
                    descending: false
                },
                SortKey {
                    field: "discnumber".into(),
                    descending: false
                },
                SortKey {
                    field: "tracknumber".into(),
                    descending: false
                },
            ],
            "the multi-key comma string parses into ordered keys"
        );
    }

    /// forgotten_loves.nsp — the order:"desc" legacy form, preserved
    /// UN-folded on a no-edit round-trip.
    #[test]
    fn roundtrip_preserves_order_desc_unfolded() {
        let v = criteria_of(json!({
            "all": [
                { "is": { "loved": true } },
                { "notInTheLast": { "lastplayed": 90 } }
            ],
            "sort": "dateloved",
            "order": "desc",
            "limit": 100
        }));
        assert_eq!(roundtrip(&v), v);

        let rules = SmartRules::parse(&v);
        assert_eq!(rules.legacy_order.as_deref(), Some("desc"));
        assert_eq!(
            rules.sort.keys,
            vec![SortKey {
                field: "dateloved".into(),
                descending: false
            }],
            "the typed keys mirror the raw string WITHOUT the fold"
        );
        assert_eq!(
            rules.effective_sort_keys(),
            vec![SortKey {
                field: "dateloved".into(),
                descending: true
            }],
            "the FORM view applies the order:desc inversion"
        );
    }

    /// A synthetic with refreshDelay + offset + an unknown top-level key +
    /// an unknown operator — everything preserved.
    #[test]
    fn roundtrip_preserves_exotica() {
        let v = json!({
            "all": [
                { "is": { "rating": 5 } },
                { "futureOp": { "novelty": 1 } },
                { "startsWith": { "title": "A" } }
            ],
            "sort": "+rating,-year",
            "limit": 42,
            "limitPercent": 10,
            "offset": 5,
            "refreshDelay": "8h",
            "someFutureKey": { "nested": [1, 2, 3] }
        });
        assert_eq!(roundtrip(&v), v);

        let rules = SmartRules::parse(&v);
        assert_eq!(rules.offset, Some(5));
        assert_eq!(rules.limit_percent, Some(10));
        assert_eq!(rules.refresh_delay.as_deref(), Some("8h"));
        assert!(rules.extra.contains_key("someFutureKey"));
        let root = rules.root.expect("root");
        assert!(
            matches!(&root.nodes[1], CriteriaNode::Unknown(u) if u.get("futureOp").is_some()),
            "unknown operator preserved verbatim as an Unknown node"
        );
        assert_eq!(
            rules.sort.keys,
            vec![
                SortKey {
                    field: "rating".into(),
                    descending: false
                },
                SortKey {
                    field: "year".into(),
                    descending: true
                },
            ],
            "+/- prefixes parse"
        );
    }

    /// Feeding the FULL .nsp envelope (name/comment included) also
    /// round-trips — the unknown keys ride `extra`.
    #[test]
    fn roundtrip_full_nsp_envelope_survives_via_extra() {
        let v = json!({
            "name": "Forgotten Loves",
            "comment": "smart playlist - Loved tracks you haven't played in 90 days.",
            "all": [ { "is": { "loved": true } } ],
            "sort": "dateloved",
            "order": "desc",
            "limit": 100
        });
        assert_eq!(roundtrip(&v), v);
    }

    /// Sort EDIT is the one canonicalization point: raw dropped, legacy
    /// order folded away, canonical `-`-prefixed comma string emitted.
    #[test]
    fn edit_sort_folds_legacy_order_canonically() {
        let v = criteria_of(json!({
            "all": [ { "is": { "loved": true } } ],
            "sort": "dateloved,playcount",
            "order": "desc",
            "limit": 100
        }));
        let mut rules = SmartRules::parse(&v);
        let mut effective = rules.effective_sort_keys();
        assert_eq!(
            effective,
            vec![
                SortKey {
                    field: "dateloved".into(),
                    descending: true
                },
                SortKey {
                    field: "playcount".into(),
                    descending: true
                },
            ]
        );
        // User flips the second key to ascending.
        effective[1].descending = false;
        rules.edit_sort(effective);

        let out = rules.to_value();
        assert_eq!(out["sort"], "-dateloved,playcount");
        assert!(
            out.get("order").is_none(),
            "the legacy order key is folded away on edit"
        );
    }

    // --- Operator enum ----------------------------------------------------

    /// All 17 wire keys round-trip through the enum; the case-insensitive
    /// parse matches the server's ToLower; unknown keys yield None.
    #[test]
    fn operator_wire_keys_roundtrip() {
        assert_eq!(RuleOperator::ALL.len(), 17);
        for op in RuleOperator::ALL {
            assert_eq!(RuleOperator::from_wire_key(op.wire_key()), Some(op));
            assert_eq!(
                RuleOperator::from_wire_key(&op.wire_key().to_uppercase()),
                Some(op)
            );
        }
        assert_eq!(RuleOperator::from_wire_key("notInTheRange"), None);
        assert_eq!(RuleOperator::from_wire_key("regex"), None);
    }

    // --- Validation matrix ------------------------------------------------

    fn ctx_with<'a>(caps: ServerCaps) -> ValidationContext<'a> {
        ValidationContext {
            caps,
            playlists: &[],
            playlists_loaded: false,
            session_target_id: None,
            known_library_ids: &[],
            discovery_failed: false,
            name: "Valid Name",
        }
    }

    fn caps_063() -> ServerCaps {
        ServerCaps::from_version_str("0.63.2")
    }

    fn caps_062() -> ServerCaps {
        ServerCaps::from_version_str("0.62.0")
    }

    fn errors(d: &[Diagnostic]) -> Vec<&str> {
        d.iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect()
    }

    fn warnings(d: &[Diagnostic]) -> Vec<&str> {
        d.iter()
            .filter(|d| d.severity == Severity::Warning)
            .map(|d| d.message.as_str())
            .collect()
    }

    #[test]
    fn validation_empty_root_errors() {
        let registry = FieldRegistry::with_default_tags();
        let rules = SmartRules::new_empty();
        let diags = validate(&rules, &registry, &ctx_with(caps_063()));
        assert!(
            errors(&diags).iter().any(|m| m.contains("No rules")),
            "the blank-create gate: an empty root blocks Preview AND Save"
        );
    }

    #[test]
    fn validation_both_conjunctions_errors() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({
            "all": [ { "is": { "loved": true } } ],
            "any": [ { "is": { "rating": 5 } } ]
        });
        let rules = SmartRules::parse(&v);
        let diags = validate(&rules, &registry, &ctx_with(caps_063()));
        assert!(errors(&diags).iter().any(|m| m.contains("both")));
        // And it still round-trips (the second conjunction rides extra).
        assert_eq!(rules.to_value(), v);
    }

    /// Fields added after 0.61 carry a version floor — offering them to an
    /// older server would save the rule then permanently-empty the playlist.
    #[test]
    fn field_version_floor_blocks_newer_field() {
        let registry = FieldRegistry::with_default_tags();

        // `missing` arrived in 0.62.0 → error on 0.61, clean on 0.62.
        let rules = SmartRules::parse(&json!({ "all": [ { "is": { "missing": true } } ] }));
        let diags = validate(
            &rules,
            &registry,
            &ctx_with(ServerCaps::from_version_str("0.61.0")),
        );
        assert!(
            errors(&diags)
                .iter()
                .any(|m| m.contains("needs Navidrome 0.62")),
            "a 0.62 field is blocked on 0.61"
        );
        assert!(
            errors(&validate(&rules, &registry, &ctx_with(caps_062()))).is_empty(),
            "the field exists on 0.62"
        );

        // `replaygain_album_gain` alias arrived in 0.63.0 → error on 0.62.
        let rules =
            SmartRules::parse(&json!({ "all": [ { "gt": { "replaygain_album_gain": -5 } } ] }));
        assert!(
            errors(&validate(&rules, &registry, &ctx_with(caps_062())))
                .iter()
                .any(|m| m.contains("needs Navidrome 0.63")),
            "a 0.63 alias is blocked on 0.62"
        );
    }

    /// `albumtype`/`recordingdate` resolve to their server-canonical field so
    /// classification (tag vs scalar column) matches the server.
    #[test]
    fn alias_fields_classified_like_the_server() {
        let registry = FieldRegistry::with_default_tags();

        // albumtype = releasetype TAG (multi-valued): presence OK everywhere.
        assert!(matches!(registry.lookup("albumtype"), Some(FieldKind::Tag)));
        assert!(
            registry.presence_ops_valid("albumtype", &ServerCaps::from_version_str("0.61.0")),
            "tag presence works on every version (server allows it)"
        );

        // recordingdate = scalar date COLUMN: presence rejected, range OK.
        assert!(
            matches!(registry.lookup("recordingdate"), Some(FieldKind::Column(d)) if d.name == "date"),
            "recordingdate resolves to the date column"
        );
        assert!(
            !registry.presence_ops_valid("recordingdate", &caps_063()),
            "presence on the scalar date column is rejected (matches the server)"
        );
        // A range on recordingdate must NOT be flagged multi-value (the false
        // Save block this fix removes).
        let rules = SmartRules::parse(
            &json!({ "all": [ { "inTheRange": { "recordingdate": ["2000-01-01", "2010-12-31"] } } ] }),
        );
        assert!(
            !errors(&validate(&rules, &registry, &ctx_with(caps_063())))
                .iter()
                .any(|m| m.contains("multi-value")),
            "a date range is valid — not a multi-value field"
        );
    }

    /// The server type-asserts `value.(bool)` on presence ops — a non-bool
    /// operand (reachable via raw JSON / .nsp import) must be blocked.
    #[test]
    fn presence_operand_must_be_bool() {
        let registry = FieldRegistry::with_default_tags();

        let bad = SmartRules::parse(&json!({ "all": [ { "isMissing": { "genre": 2 } } ] }));
        assert!(
            errors(&validate(&bad, &registry, &ctx_with(caps_063())))
                .iter()
                .any(|m| m.contains("true or false")),
            "a non-bool presence operand errors"
        );

        let ok = SmartRules::parse(&json!({ "all": [ { "isMissing": { "genre": true } } ] }));
        assert!(
            errors(&validate(&ok, &registry, &ctx_with(caps_063()))).is_empty(),
            "a real bool operand is fine"
        );

        // strconv.ParseBool-able strings coerce server-side → clean.
        let coerce = SmartRules::parse(&json!({ "all": [ { "isPresent": { "genre": "true" } } ] }));
        assert!(
            errors(&validate(&coerce, &registry, &ctx_with(caps_063()))).is_empty(),
            "a ParseBool-able string coerces"
        );
    }

    #[test]
    fn validation_unknown_field_warns_hedged_on_unknown_server() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({ "all": [ { "is": { "grooviness": 11 } } ] });
        let rules = SmartRules::parse(&v);

        let diags = validate(&rules, &registry, &ctx_with(caps_063()));
        assert!(
            warnings(&diags)
                .iter()
                .any(|m| m.contains("not a known field")),
            "unknown field warns (saves fine, matches nothing — D5)"
        );
        assert!(errors(&diags).is_empty(), "warn-only, never blocks");

        let diags = validate(&rules, &registry, &ctx_with(ServerCaps::default()));
        assert!(
            warnings(&diags).iter().any(|m| m.contains("may not be")),
            "hedged copy when the server version is unknown"
        );
    }

    #[test]
    fn validation_date_literals() {
        let registry = FieldRegistry::with_default_tags();
        let ctx = ctx_with(caps_063());
        for (value, ok) in [
            ("2024-03-01", true),
            ("2024-02-29", true),  // leap year
            ("2023-02-29", false), // not a leap year
            ("2024-13-45", false),
            ("last tuesday", false),
            ("2024-3-1", false),
        ] {
            let v = json!({ "all": [ { "before": { "lastplayed": value } } ] });
            let rules = SmartRules::parse(&v);
            let diags = validate(&rules, &registry, &ctx);
            assert_eq!(
                errors(&diags).is_empty(),
                ok,
                "date literal {value:?} should be ok={ok}"
            );
        }
    }

    #[test]
    fn validation_presence_ops_version_matrix() {
        let registry = FieldRegistry::with_default_tags();
        // Non-nullable column: rejected on EVERY version (missingExpr's
        // default arm — hard-fails refresh, permanently-empty playlist).
        for caps in [caps_062(), caps_063()] {
            let v = json!({ "all": [ { "isMissing": { "title": true } } ] });
            let diags = validate(&SmartRules::parse(&v), &registry, &ctx_with(caps));
            assert!(
                !errors(&diags).is_empty(),
                "isMissing on a non-nullable column must error on every caps"
            );
        }
        // Nullable column: ok on ≥0.63, error on ≤0.62.
        let v = json!({ "all": [ { "isMissing": { "lyrics": true } } ] });
        let d62 = validate(&SmartRules::parse(&v), &registry, &ctx_with(caps_062()));
        assert!(
            errors(&d62).iter().any(|m| m.contains("0.63+")),
            "nullable column presence op errors on 0.62 with version copy"
        );
        let d63 = validate(&SmartRules::parse(&v), &registry, &ctx_with(caps_063()));
        assert!(errors(&d63).is_empty(), "…and passes on 0.63");
        // Tags/roles: everywhere.
        for field in ["genre", "composer"] {
            let v = json!({ "all": [ { "isMissing": { field: true } } ] });
            for caps in [caps_062(), caps_063()] {
                let diags = validate(&SmartRules::parse(&v), &registry, &ctx_with(caps));
                assert!(
                    errors(&diags).is_empty(),
                    "tag/role presence op must pass everywhere ({field})"
                );
            }
        }
    }

    #[test]
    fn validation_range_on_tag_errors() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({ "all": [ { "inTheRange": { "genre": ["A", "Z"] } } ] });
        let diags = validate(&SmartRules::parse(&v), &registry, &ctx_with(caps_063()));
        assert!(
            errors(&diags).iter().any(|m| m.contains("multi-value")),
            "rangeExpr rejects tag/role ranges at refresh — must error"
        );
    }

    #[test]
    fn validation_days_must_be_integer() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({ "all": [ { "inTheLast": { "lastplayed": "ninety" } } ] });
        let diags = validate(&SmartRules::parse(&v), &registry, &ctx_with(caps_063()));
        assert!(errors(&diags).iter().any(|m| m.contains("days")));
    }

    #[test]
    fn validation_dangling_ref_suppressed_until_list_loads() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({ "all": [ { "inPlaylist": { "id": "gone-1" } } ] });
        let rules = SmartRules::parse(&v);

        // Unloaded list: SUPPRESSED (never false-fire against emptiness).
        let ctx = ctx_with(caps_063());
        let diags = validate(&rules, &registry, &ctx);
        assert!(
            warnings(&diags).is_empty(),
            "dangling-ref must stay silent while playlists_loaded is false"
        );

        // Loaded list without the id: fires.
        let playlists = vec![("p1".to_owned(), "Road Trip".to_owned())];
        let ctx = ValidationContext {
            playlists: &playlists,
            playlists_loaded: true,
            ..ctx_with(caps_063())
        };
        let diags = validate(&rules, &registry, &ctx);
        assert!(
            warnings(&diags)
                .iter()
                .any(|m| m.contains("no longer exists")),
            "dangling ref warns once the list is genuinely loaded"
        );
    }

    #[test]
    fn validation_self_reference_errors() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({ "all": [ { "inPlaylist": { "id": "me" } } ] });
        let rules = SmartRules::parse(&v);
        let ctx = ValidationContext {
            session_target_id: Some("me"),
            ..ctx_with(caps_063())
        };
        let diags = validate(&rules, &registry, &ctx);
        assert!(
            errors(&diags).iter().any(|m| m.contains("itself")),
            "self-reference = unbounded server recursion — Error"
        );
    }

    #[test]
    fn validation_sort_and_limit_lanes() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({
            "all": [ { "is": { "loved": true } } ],
            "sort": "random,notafield",
            "offset": 10
        });
        let rules = SmartRules::parse(&v);
        let diags = validate(&rules, &registry, &ctx_with(caps_063()));
        let warns = warnings(&diags);
        assert!(warns.iter().any(|m| m.contains("Random sort")));
        assert!(warns.iter().any(|m| m.contains("fall back to title")));
        assert!(warns.iter().any(|m| m.contains("Offset")));

        let v = json!({ "all": [ { "is": { "loved": true } } ], "limitPercent": 250 });
        let diags = validate(&SmartRules::parse(&v), &registry, &ctx_with(caps_063()));
        assert!(errors(&diags).iter().any(|m| m.contains("1 and 100")));
    }

    #[test]
    fn validation_name_lanes() {
        let registry = FieldRegistry::with_default_tags();
        let v = json!({ "all": [ { "is": { "loved": true } } ] });
        let rules = SmartRules::parse(&v);

        let ctx = ValidationContext {
            name: "  ",
            ..ctx_with(caps_063())
        };
        let diags = validate(&rules, &registry, &ctx);
        assert!(errors(&diags).iter().any(|m| m.contains("empty")));

        // Duplicate name warns (loaded list), excluding the session target.
        let playlists = vec![
            ("p1".to_owned(), "Road Trip".to_owned()),
            ("me".to_owned(), "My Own".to_owned()),
        ];
        let ctx = ValidationContext {
            name: "road trip",
            playlists: &playlists,
            playlists_loaded: true,
            session_target_id: Some("me"),
            ..ctx_with(caps_063())
        };
        let diags = validate(&rules, &registry, &ctx);
        assert!(
            warnings(&diags)
                .iter()
                .any(|m| m.contains("already exists"))
        );

        // The session target's own name never self-warns.
        let ctx = ValidationContext {
            name: "My Own",
            playlists: &playlists,
            playlists_loaded: true,
            session_target_id: Some("me"),
            ..ctx_with(caps_063())
        };
        let diags = validate(&rules, &registry, &ctx);
        assert!(warnings(&diags).is_empty());
    }

    // --- ServerCaps -------------------------------------------------------

    #[test]
    fn server_caps_version_matrix() {
        let c = ServerCaps::from_version_str("0.62.0");
        assert_eq!(c.version, Some((0, 62, 0)));
        assert!(c.rules_via_rest);
        assert!(c.rules_put_preserves_unsent);
        assert!(c.sync_via_put);
        assert!(!c.nullable_column_presence_ops);
        assert!(!c.put_nils_evaluated_at);

        let c = ServerCaps::from_version_str("0.63.2 (49c5cc98)");
        assert_eq!(c.version, Some((0, 63, 2)));
        assert!(c.nullable_column_presence_ops);
        // Pin-documented: the PUT-path EvaluatedAt nil is UNRELEASED — must
        // stay false for every 0.63.x including the owner's live server.
        assert!(!c.put_nils_evaluated_at);
        assert!(!c.per_playlist_refresh_delay);

        let c = ServerCaps::from_version_str("0.60.3");
        assert!(!c.rules_via_rest);

        let c = ServerCaps::from_version_str("garbage");
        assert_eq!(c.version, None);
        assert!(
            !c.rules_via_rest,
            "unparseable ⇒ all-false (feature-hidden)"
        );
    }

    // --- Registry ---------------------------------------------------------

    #[test]
    fn registry_three_tiers_resolve() {
        let registry = FieldRegistry::with_default_tags();
        assert!(matches!(
            registry.lookup("playcount"),
            Some(FieldKind::Column(d)) if d.class == FieldClass::Number
        ));
        assert!(matches!(
            registry.lookup("LastPlayed"),
            Some(FieldKind::Column(d)) if d.class == FieldClass::Date
        ));
        assert!(matches!(registry.lookup("composer"), Some(FieldKind::Role)));
        assert!(matches!(registry.lookup("genre"), Some(FieldKind::Tag)));
        assert_eq!(registry.lookup("random"), None, "sort-only pseudo-field");
        assert_eq!(registry.lookup("grooviness"), None);
        assert_eq!(ROLE_FIELDS.len(), 14);

        // Every picker quick-row resolves against the registry.
        for name in PICKER_FIELDS {
            assert!(
                registry.lookup(name).is_some(),
                "picker field {name} must resolve"
            );
        }
    }

    // --- Presets ----------------------------------------------------------

    /// Every seeded preset builds, validates clean on the owner's caps, and
    /// round-trips through serialization.
    #[test]
    fn seed_presets_build_clean() {
        let registry = FieldRegistry::with_default_tags();
        let ctx = ctx_with(caps_063());
        assert_eq!(SEED_PRESETS.len(), 5);
        for preset in SEED_PRESETS {
            let rules = (preset.build)();
            let diags = validate(&rules, &registry, &ctx);
            assert!(
                errors(&diags).is_empty(),
                "preset {:?} must validate clean, got {:?}",
                preset.name,
                diags
            );
            let v = rules.to_value();
            assert_eq!(
                SmartRules::parse(&v).to_value(),
                v,
                "preset {:?} must round-trip",
                preset.name
            );
        }
        let comeback = SEED_PRESETS
            .iter()
            .find(|p| p.name == "Comeback Queue")
            .expect("present");
        assert_eq!(
            (comeback.build)().max_depth(),
            1,
            "the tiered preset exercises the flat-plus-one render"
        );
    }

    // --- .nsp envelope ----------------------------------------------------

    /// The envelope split (name/comment/public out, criteria kept) against
    /// the owner's live movie_tv_soundtracks.nsp shape, with comments and
    /// the cap exercised.
    #[test]
    fn nsp_envelope_parse_matrix() {
        let fixture = br#"{
            // film & tv soundtracks
            "name": "Movie & TV Soundtracks",
            "comment": "smart playlist - Film & TV soundtracks/scores",
            "all": [
                { "any": [ { "is": { "genre": "Soundtrack" } } ] },
                { "isNot": { "genre": "Video Game Music" } }
            ],
            "sort": "album,discnumber,tracknumber", /* multi-key */
            "order": "asc",
            "limit": 1000
        }"#;
        let env = parse_nsp_envelope(fixture).expect("fixture parses");
        assert_eq!(env.name.as_deref(), Some("Movie & TV Soundtracks"));
        assert!(env.comment.as_deref().is_some_and(|c| c.contains("scores")));
        assert_eq!(env.public, None);
        assert!(env.criteria.get("name").is_none(), "metadata split out");
        assert!(env.criteria.get("all").is_some(), "criteria kept");
        assert_eq!(env.criteria["sort"], "album,discnumber,tracknumber");
        // The criteria round-trips through the typed model.
        let rules = SmartRules::parse(&env.criteria);
        assert_eq!(rules.to_value(), env.criteria);

        // Failure lanes carry the user-facing reasons.
        assert_eq!(
            parse_nsp_envelope(b"not json").unwrap_err(),
            "couldn't parse JSON"
        );
        assert_eq!(
            parse_nsp_envelope(b"{ \"name\": \"x\" }").unwrap_err(),
            "no rules object found"
        );
        let big = vec![b' '; NSP_MAX_BYTES + 1];
        assert_eq!(parse_nsp_envelope(&big).unwrap_err(), "file exceeds 100 KB");

        // Comment markers INSIDE strings survive the strip.
        let tricky =
            br#"{ "all": [ { "contains": { "comment": "http://x // not a comment" } } ] }"#;
        let env = parse_nsp_envelope(tricky).expect("string-embedded slashes survive");
        assert!(
            env.criteria
                .to_string()
                .contains("http://x // not a comment"),
            "the in-string content must be untouched"
        );

        // Non-ASCII content survives byte-for-byte (the `byte as char`
        // regression mojibaked multi-byte UTF-8).
        let unicode = "{ \"name\": \"Café Motörhead 音楽 🎸\", \
             \"all\": [ { \"is\": { \"genre\": \"Métal\" } } ] }";
        let env = parse_nsp_envelope(unicode.as_bytes()).expect("utf-8 parses");
        assert_eq!(env.name.as_deref(), Some("Café Motörhead 音楽 🎸"));
        assert_eq!(env.criteria["all"][0]["is"]["genre"], "Métal");
    }

    // --- TagDiscovery -----------------------------------------------------

    #[test]
    fn tag_discovery_orders_values_by_song_count() {
        let d = TagDiscovery::from_rows(vec![
            ("genre".to_owned(), "Rock".to_owned(), 10),
            ("genre".to_owned(), "Black Metal".to_owned(), 400),
            ("Mood".to_owned(), "calm".to_owned(), 3),
        ]);
        assert_eq!(d.tag_names, vec!["genre", "mood"]);
        assert_eq!(
            d.values_by_tag["genre"],
            vec!["Black Metal".to_owned(), "Rock".to_owned()]
        );
    }

    // --- Property: serialize→parse fixpoint -------------------------------

    mod prop {
        // Disambiguate `prop`: the glob above AND `super::*` (whose parent also
        // globs proptest's prelude) both re-export proptest's `prop` module, so
        // `#[deny(ambiguous_glob_imports)]` — a future-incompat lint that is a
        // hard error on newer rustc (CI), only a warning on 1.95.0 — rejects it.
        // An explicit import wins over the globs and resolves the ambiguity.
        use proptest::prelude::{prop, *};

        use super::*;

        fn arb_leaf() -> impl Strategy<Value = CriteriaNode> {
            let ops = prop::sample::select(RuleOperator::ALL.to_vec());
            let fields = prop::sample::select(vec![
                "title",
                "rating",
                "playcount",
                "lastplayed",
                "genre",
                "loved",
            ]);
            (ops, fields, 0i64..1000).prop_map(|(op, field, n)| {
                CriteriaNode::Leaf(RuleLeaf::new(op, field, serde_json::json!(n)))
            })
        }

        fn arb_rules() -> impl Strategy<Value = SmartRules> {
            (
                prop::collection::vec(arb_leaf(), 1..5),
                prop::collection::vec(arb_leaf(), 0..3),
                prop::bool::ANY,
                prop::option::of(1u64..5000),
                prop::option::of(1u64..100),
            )
                .prop_map(|(leaves, group_leaves, any, limit, pct)| {
                    let mut rules = SmartRules::new_empty();
                    let mut root = CriteriaGroup::new(if any {
                        Conjunction::Any
                    } else {
                        Conjunction::All
                    });
                    root.nodes = leaves;
                    if !group_leaves.is_empty() {
                        let mut group = CriteriaGroup::new(Conjunction::All);
                        group.nodes = group_leaves;
                        root.nodes.push(CriteriaNode::Group(group));
                    }
                    rules.root = Some(root);
                    rules.limit = limit;
                    rules.limit_percent = pct;
                    rules.sort = SortSpec {
                        raw: None,
                        keys: vec![SortKey {
                            field: "playcount".into(),
                            descending: true,
                        }],
                    };
                    rules
                })
        }

        proptest! {
            /// to_value → parse → to_value is a fixpoint for arbitrary
            /// well-formed rule trees.
            #[test]
            fn serialize_parse_fixpoint(rules in arb_rules()) {
                let v1 = rules.to_value();
                let v2 = SmartRules::parse(&v1).to_value();
                prop_assert_eq!(v1, v2);
            }
        }
    }
}
