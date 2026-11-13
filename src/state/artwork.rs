//! Artwork caches (mini, large, collages) and loading state.
//!
//! Pre-redesign also held dominant-color extractions for the artwork-elevated
//! header — the redesign moved every active-state visual onto `accent_bright()`
//! fills, so the dominant-color path was removed (along with `color-thief`).

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    time::Duration,
};

use iced::widget::image;
use nokkvi_data::types::player_settings::LyricsBackdropBlur;
// The gate's clock: the system's in the app, one a test can hold still.
use tokio::time::Instant;

use super::snapshotted_lru::SnapshottedLru;

/// Maximum entries in the large artwork LRU cache. The entry count bounds
/// small covers; [`LARGE_ARTWORK_BYTE_BUDGET`] bounds the cache's memory.
const LARGE_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(200).expect("capacity must be > 0");
/// Bytes of full-size covers the panel's cache holds at most
/// ([`ArtworkState::cache_large_cover`]). A byte budget beside the entry
/// count, because a cover's size follows the Artwork Resolution setting and
/// the server's encoder: Navidrome answers the square covers nokkvi asks for
/// as PNG, about 1.3 MB each at 1000 px (measured 2026-11-13), so 200 of
/// them held 255 MB. 64 MB keeps about 50 of those for stepping back.
pub(crate) const LARGE_ARTWORK_BYTE_BUDGET: usize = 64 * 1024 * 1024;
/// Capacity for the mini-artwork (`album_art`) LRU. Sized roughly 6× a typical
/// 80px slot list viewport so recently-visited slot regions stay warm but
/// memory stays bounded as the user scrolls a large library. Doubled when the
/// playlist 2×2 quads landed: a playlists viewport claims up to 4 ids per row
/// instead of 1, and the quads share this cache with every other 80px surface
/// — at ~3-6KB per 80px handle the cap stays a few MB.
const MINI_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(1024).expect("capacity must be > 0");
/// Capacity for the per-target collage mini LRU (genre or playlist).
const COLLAGE_MINI_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("capacity must be > 0");
/// Capacity for the per-target collage tile LRU (genre or playlist).
const COLLAGE_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("capacity must be > 0");
/// Capacity for the mini radio-station artwork LRU (`station_id -> Handle`).
/// Radio station lists are small (a few dozen), so a modest cap holds every
/// station's logo / remembered stream-art thumbnail with room to spare.
const RADIO_ART_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(256).expect("capacity must be > 0");
/// Capacity for the mini custom playlist artwork LRU (`playlist_id -> Handle`).
/// Mirrors the collage-mini cap: custom covers are one image per playlist and
/// only playlists whose `uploaded_image` is set ever fetch, so 100 covers a
/// full viewport plus generous scrollback.
const PLAYLIST_CUSTOM_ART_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("capacity must be > 0");
/// Capacity for the large custom playlist artwork LRU (`playlist_id -> Handle`).
/// The panel shows one at a time; a small cap keeps back-navigation instant
/// without holding many resolution-sized images.
const PLAYLIST_CUSTOM_LARGE_ART_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(32).expect("capacity must be > 0");
/// Capacity for the large radio-station artwork LRU (`station_id -> Handle`).
/// The panel shows one station's large art at a time, but the cache accumulates
/// every logo station ever centered (each scroll/nav step dispatches a
/// `LoadRadioLarge`), and the playing station's entry is `put` once and never
/// recency-bumped (the view reads the snapshot without touching the LRU). Sized
/// to comfortably exceed any realistic station list so scrolling can't evict the
/// now-playing station's large art out from under the locked panel / MiniPlayer.
const RADIO_LARGE_ART_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(512).expect("capacity must be > 0");
/// Capacity for the cover placeholder LRU (`id -> Placeholder`). Each is a
/// picture of at most 32 x 32 (4 KB), decoded only for a cover on its way, so
/// half the mini cache's cap covers every slot a long scroll leaves loading.
const PLACEHOLDER_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(512).expect("capacity must be > 0");

/// In-flight artwork fetches for one cache: id -> the version the request
/// carries (`()` for the thumbnail gates, which hold any fetch of an id).
///
/// The artwork paths re-dispatch on every scroll step, re-entry or centering,
/// and a held arrow key re-centers one row ~25x/s, so without a gate a slow
/// server stacks one request per pass (gotchas/artwork.md). Every gated path
/// does the same four things:
/// - gate: [`Self::holds`] (an exact repeat sends nothing; a changed version
///   goes out, so the gate never pins a replaced cover) or [`Self::contains`];
/// - [`Self::claim`] once the fetch task is built, after the `app_service`
///   check (artwork state resets at logout, not login);
/// - [`Self::release`] in the loaded handler on every outcome, so a failed
///   fetch is re-attempted by the next pass;
/// - [`Self::forget`] wherever the cover's cache is cleared for a re-warm.
#[derive(Debug, Clone)]
pub struct InFlight<V> {
    claims: HashMap<String, V>,
}

impl<V> Default for InFlight<V> {
    fn default() -> Self {
        Self {
            claims: HashMap::new(),
        }
    }
}

impl<V> InFlight<V> {
    /// Whether any fetch of `id` is on its way, whatever its version.
    pub fn contains(&self, id: &str) -> bool {
        self.claims.contains_key(id)
    }

    /// Whether a fetch of `id` at exactly `version` is on its way.
    pub fn holds(&self, id: &str, version: &V) -> bool
    where
        V: PartialEq,
    {
        self.claims.get(id) == Some(version)
    }

    /// Record the fetch the caller has just built, replacing any older claim
    /// for `id`.
    pub fn claim(&mut self, id: String, version: V) {
        self.claims.insert(id, version);
    }

    /// The loaded handler's release, on every outcome.
    pub fn release(&mut self, id: &str) {
        self.claims.remove(id);
    }

    /// Drop the claim where `id`'s cached cover is cleared: the fetch still in
    /// flight is for the old cover, and the re-warm must go out rather than
    /// wait on it (a Refresh keeps the same version, so [`Self::holds`] alone
    /// would hold it back). Its completion still lands and releases.
    pub fn forget(&mut self, id: &str) {
        self.claims.remove(id);
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }
}

/// How long the newest waiting row trusts the cover on its way to land
/// before it is fetched anyway: past this the request is slow or stuck (the
/// cover client waits up to 8 s to connect and 20 s for a read).
pub(crate) const PANEL_ART_OVERDUE: Duration = Duration::from_secs(1);

/// The full-size cover a centred row shows in the big panel, by the loader
/// it comes from: an album's, or an artist's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelArt {
    Album(String),
    Artist(String),
}

impl PanelArt {
    /// The album or artist id, the key of `large_artwork`.
    pub fn id(&self) -> &str {
        match self {
            Self::Album(id) | Self::Artist(id) => id,
        }
    }
}

/// Steps closer together than this are a held key's repeats (every 40 ms at
/// the default rate) or a wheel flick. Taps and single notches come slower
/// and are never paced ([`PanelArtGate`]).
pub(crate) const PACED_RUN_MAX_GAP: Duration = Duration::from_millis(60);

/// A cover that lands faster than this is on screen a few frames after its
/// step, so a server that fast is never paced.
pub(crate) const PACED_RUN_MIN_LANDING: Duration = Duration::from_millis(25);

/// What [`PanelArtGate::step`] decided for a centred row.
#[derive(Debug, PartialEq, Eq)]
pub enum PanelArtStep {
    /// Fetch now: nothing is on its way, or the cover is already cached.
    Now(PanelArt),
    /// A cover is on its way: this row now waits as the newest, fetched when
    /// that cover lands, or after [`PANEL_ART_OVERDUE`]
    /// (`ArtworkMessage::PanelArtOverdue`) when it is slow.
    Wait(PanelArt),
    /// The row is one of a run of steps that come faster than a cover
    /// lands: it waits as the newest, and is fetched `after` this long if no
    /// step follows it (`ArtworkMessage::PanelArtOverdue`), which is the run
    /// stopping on it.
    Paced { target: PanelArt, after: Duration },
}

/// The newest row waiting at the gate.
#[derive(Debug, Clone)]
struct WaitingRow {
    /// The list it was centred in.
    source: Option<crate::View>,
    target: PanelArt,
    /// It waits for its run of steps to stop, not for the cover on its way.
    paced: bool,
}

/// Keeps one full-size cover for the big panel on its way at a time. A step
/// while one is on its way waits as the newest row, and replaces any older
/// waiting row, which is never fetched; when the cover lands, the newest
/// waiting row goes out. A fast server lands each cover before the next
/// step, so every step fetches at once and the panel turns sharp as it
/// always did. On a slow one a scroll fetches one cover per landing instead
/// of one per row (about 1 MB each at 1000 px), and none of them queue ahead
/// of the thumbnails of the rows the scroll stopped on. The panel shows the
/// thumbnail or the blurred placeholder (`views::panel_cover`) meanwhile.
///
/// A held key or a wheel flick steps faster than a cover lands on any
/// server that is not on this machine (a key repeats every 40 ms; a 1000 px
/// cover took about 45 ms from a server on the LAN). Each of those rows has
/// left the centre before its cover could show, yet one was fetched per
/// landing: a 40 s held scroll asked for 897 covers, 1.1 GB, and drew none
/// of them. Such a run is paced: while steps come closer together than
/// [`PACED_RUN_MAX_GAP`] and than the last cover took to land, a row waits
/// for the run to stop instead, a quarter of a step longer than the steps
/// are apart, and is fetched then, past any cover still on its way. The
/// first step of a run, a tap, a single wheel notch and every step on a
/// server that lands a cover between two steps load at once as before: the
/// gate measures the landing, it never assumes one. Releasing the key ends
/// the run at once ([`Self::run_stopped`]); the timer is for a run with no
/// release to go by.
///
/// Only steps in the list the user is steering reach the gate
/// (`Nokkvi::panel_art_step`); one waiting slot is enough because a step
/// from any other list loads directly. Reset on logout via
/// `ArtworkState::default()`, so a cover landing or an overdue timer from an
/// ended session finds nothing waiting.
#[derive(Debug, Clone, Default)]
pub struct PanelArtGate {
    /// The cover on its way and when it went out, recorded once its loader
    /// claimed the fetch.
    in_flight: Option<(PanelArt, Instant)>,
    /// The newest row a step centred while that cover was on its way, or in
    /// a paced run.
    waiting: Option<WaitingRow>,
    /// When the steered list last stepped.
    last_step: Option<Instant>,
    /// How long the last cover on its way took to land.
    landing: Option<Duration>,
}

impl PanelArtGate {
    /// The steered list (`source`) centred `target`. `cached`: its cover is
    /// already in the cache, so it shows at once and leaves nothing waiting.
    pub fn step(
        &mut self,
        source: Option<crate::View>,
        target: PanelArt,
        cached: bool,
    ) -> PanelArtStep {
        self.step_at(source, target, cached, Instant::now())
    }

    /// [`Self::step`] at `now`.
    fn step_at(
        &mut self,
        source: Option<crate::View>,
        target: PanelArt,
        cached: bool,
        now: Instant,
    ) -> PanelArtStep {
        let gap = self
            .last_step
            .map(|last| now.saturating_duration_since(last));
        self.last_step = Some(now);
        if cached {
            self.waiting = None;
            return PanelArtStep::Now(target);
        }
        if let Some(after) = self.paced_wait(gap) {
            self.waiting = Some(WaitingRow {
                source,
                target: target.clone(),
                paced: true,
            });
            return PanelArtStep::Paced { target, after };
        }
        if self.in_flight.is_some() {
            self.waiting = Some(WaitingRow {
                source,
                target: target.clone(),
                paced: false,
            });
            return PanelArtStep::Wait(target);
        }
        self.waiting = None;
        PanelArtStep::Now(target)
    }

    /// How long a step that came `gap` after the one before it waits for
    /// its run to stop, or `None` when it is not one of a paced run: the
    /// first step, a step slower than a held key, or a server whose covers
    /// land between two such steps.
    fn paced_wait(&self, gap: Option<Duration>) -> Option<Duration> {
        let (gap, landing) = (gap?, self.landing?);
        (gap < PACED_RUN_MAX_GAP && landing >= PACED_RUN_MIN_LANDING && gap < landing)
            .then(|| gap + gap / 4)
    }

    /// Whether a row from a list other than `source` is waiting. In the split
    /// view both panes steer the panel, and a step in one must not replace or
    /// clear the row the other has waiting (its cover would never be sent):
    /// that step loads directly and leaves the gate alone.
    pub fn another_list_waits(&self, source: Option<crate::View>) -> bool {
        self.waiting
            .as_ref()
            .is_some_and(|row| row.source != source)
    }

    /// A fetch for `target` went out: it is the cover on its way.
    pub fn sent(&mut self, target: PanelArt) {
        self.sent_at(target, Instant::now());
    }

    /// [`Self::sent`] at `now`.
    fn sent_at(&mut self, target: PanelArt, now: Instant) {
        self.in_flight = Some((target, now));
    }

    /// A full-size cover for `id` landed, on any outcome. When it was the
    /// cover on its way, the gate opens and hands back the newest row that
    /// waited for it. A paced row stays: its own timer sends it.
    pub fn landed(&mut self, id: &str) -> Option<(Option<crate::View>, PanelArt)> {
        self.landed_at(id, Instant::now())
    }

    /// [`Self::landed`] at `now`.
    fn landed_at(&mut self, id: &str, now: Instant) -> Option<(Option<crate::View>, PanelArt)> {
        let (_, sent) = self.in_flight.take_if(|(cover, _)| cover.id() == id)?;
        self.landing = Some(now.saturating_duration_since(sent));
        self.waiting
            .take_if(|row| !row.paced)
            .map(|row| (row.source, row.target))
    }

    /// The timer of a waiting `target` fired (a cover that is overdue, or a
    /// paced run that stopped): `true` when it is still the newest waiting
    /// row, which stops waiting (the caller fetches it).
    pub fn overdue(&mut self, target: &PanelArt) -> bool {
        self.waiting.take_if(|row| row.target == *target).is_some()
    }

    /// The run of steps a paced row is the last of has stopped (the held key
    /// was released): the row stops waiting and is handed back to be
    /// fetched. A row waiting for the cover on its way stays.
    pub fn run_stopped(&mut self) -> Option<(Option<crate::View>, PanelArt)> {
        self.waiting
            .take_if(|row| row.paced)
            .map(|row| (row.source, row.target))
    }

    /// As if the last cover on its way had taken `landing` to land.
    #[cfg(test)]
    pub(crate) fn set_landing_for_test(&mut self, landing: Duration) {
        self.landing = Some(landing);
    }
}

/// A cover's blurred stand-in: the ThumbHash (Navidrome 0.64+) it was
/// decoded from, and the picture. See [`ArtworkState::placeholders`].
#[derive(Debug, Clone)]
pub struct Placeholder {
    pub thumb_hash: String,
    pub handle: image::Handle,
}

/// Per-target collage artwork cache (genre or playlist).
///
/// Each LRU is wrapped in `SnapshottedLru` so the view-layer-borrowable
/// `HashMap` snapshot stays in sync automatically — callers can't forget the
/// pairing.
#[derive(Debug, Clone)]
pub struct CollageArtworkCache {
    /// Mini artwork LRU + snapshot (item_id -> first album's cover Handle).
    pub mini: SnapshottedLru<String, image::Handle>,
    /// Collage artwork LRU + snapshot (item_id -> up-to-9 tile Handles).
    pub collage: SnapshottedLru<String, Vec<image::Handle>>,
    /// IDs with pending artwork loads (prevents duplicate in-flight requests).
    pub pending: HashSet<String>,
}

impl CollageArtworkCache {
    pub fn new() -> Self {
        Self {
            mini: SnapshottedLru::new(COLLAGE_MINI_CACHE_CAPACITY),
            collage: SnapshottedLru::new(COLLAGE_ARTWORK_CACHE_CAPACITY),
            pending: HashSet::new(),
        }
    }
}

/// Artwork caches and loading state.
///
/// All three primary caches use `SnapshottedLru` so view-layer borrowing
/// against the read-only `HashMap` mirror stays consistent with the LRU
/// after every mutation — no manual `refresh_*_snapshot()` discipline
/// required.
#[derive(Clone)]
pub struct ArtworkState {
    /// Mini artwork cache (album_id -> Handle), bounded LRU.
    /// Without a persistent disk cache, this is the only thing keeping recently-
    /// rendered thumbnails warm. Capacity must stay above the typical viewport
    /// + scrollback or slot lists thrash.
    pub album_art: SnapshottedLru<String, image::Handle>,
    /// Sibling map recording the artwork version that warmed each `album_art`
    /// slot: the image hash on Navidrome 0.64+ (`artwork_version`), else the
    /// `updated_at` cache-buster. Kept in lockstep with `album_art` on every
    /// put: when the server-side cover changes, the version changes, and a
    /// later prefetch tick sees `album_art_versions[id] != new_version` and
    /// treats the slot as a genuine miss — re-fetching the changed cover on the
    /// album-coherent surfaces (Albums view, Artists/Genres expansion, the
    /// Artists view's `ar-` minis) that pass it, without threading a full
    /// `(album_id, version)` key through the ~15 view read sites (N17).
    /// The passive surfaces (queue, song-mini, similar, playlist editor) carry
    /// only a per-song `updated_at`, which would oscillate this album_id-keyed
    /// map, so they feed a constant `None` (id-only dedup,
    /// `update::components::passive_artwork_version`) and re-warm the current
    /// cover on the next cold load. That `None` offers no version: it
    /// never counts as a mismatch and never replaces a recorded one
    /// (`should_refetch`, `record_mini_art`).
    ///
    /// Reset to empty by `Default`, so logout (`ArtworkState::default()`) drops
    /// it for free. `album_art` evicts silently at capacity, so a version entry
    /// can outlive its handle — `should_refetch` guards against that by checking
    /// `album_art` membership, not just this map.
    pub album_art_versions: HashMap<String, Option<String>>,
    /// Album/artist ids with an in-flight 80px fetch, from every surface that
    /// fills [`Self::album_art`]: the viewport prefetches (albums, songs,
    /// queue, editor, Similar, the Artists `ar-` minis), the expansion and
    /// Harbour fan-outs, the quad tiles and the now-playing warm. Every one
    /// re-runs on a scroll step or a re-entry; gated on arrived thumbnails
    /// alone, a cold viewport on a slow server re-requested every thumbnail
    /// still on its way per step (one was fetched 29 times in a demo-server
    /// session), and the repeats queued ahead of the large covers and the
    /// audio. Inserted when a fetch task is built, removed by
    /// `handle_artwork_loaded` / `handle_song_artwork_loaded` on success AND
    /// failure (the loaded message always arrives), so a transient failure
    /// is re-attempted by the next pass as before.
    pub album_art_pending: HashSet<String>,
    /// Negative cache: album/artist ids whose 80px cover fetch returned NO image
    /// — a stale/deleted id that resolves to Navidrome's code-70 "Artwork not
    /// found". An album that merely lacks a cover gets a placeholder *image* and
    /// caches normally via `album_art`, so it never lands here; only genuinely
    /// unresolvable ids do. Maps `id -> the updated_at that failed`, mirroring
    /// `album_art_versions`: the membership-based prefetch gates (`should_refetch`,
    /// the artist gate, the quad gate) consult it to stop re-queuing a known-dead
    /// id on every scroll/resize/view-switch. A CHANGED `updated_at` (server cover
    /// added) bypasses the entry and re-attempts; it is cleared on any later
    /// success in the loaded-handlers and on a user "Refresh Artwork", and dropped
    /// wholesale by `ArtworkState::default()` on logout/session reset (so server-A
    /// failures never suppress server-B art). Unbounded like `album_art_versions`,
    /// but populated only by the narrow stale-id case and reset on logout.
    pub failed_art: HashMap<String, Option<String>>,
    /// Blurred stand-ins for covers on their way, in [`Self::album_art`]'s id
    /// space (album or artist id). Navidrome 0.64+ sends each album's and
    /// artist's `thumbHash`; the 80px claim (`Nokkvi::claim_minis`) decodes
    /// it, so a row waiting on its mini, and the big panel waiting on both
    /// covers, draw the blur instead of an empty square
    /// (`views::mini_or_placeholder`, `views::panel_cover`). A cover whose
    /// ThumbHash changed is decoded again at its next claim. Reset on logout
    /// via `Default`.
    pub placeholders: SnapshottedLru<String, Placeholder>,
    /// Large artwork cache for detail views (LRU-bounded). A cover enters it
    /// once it is on the GPU ([`Self::large_staging`]), so a panel that
    /// reads it never draws a cover the renderer is still decoding.
    pub large_artwork: SnapshottedLru<String, image::Handle>,
    /// Full-size covers whose bytes arrived and which are being decoded
    /// onto the GPU (`image::allocate`), by album or artist id. The renderer
    /// decodes a new image on a worker thread and draws nothing for it
    /// meanwhile: handed to the panel at once, a cover left it empty for a
    /// frame or more between the thumbnail and the cover. Staged, it stays
    /// out of [`Self::large_artwork`], so the panel keeps the thumbnail
    /// (`views::panel_cover`) until `Nokkvi::handle_large_staged` shows it.
    pub large_staging: HashMap<String, image::Handle>,
    /// The allocations of the covers shown most recently, which keep them
    /// on the GPU: stepping back onto one draws it at once. Held within
    /// [`READY_COVER_PIXEL_BUDGET`]; an older cover is decoded again when it
    /// is next shown.
    pub(crate) large_on_gpu: Budgeted<image::Allocation>,
    /// The Navidrome 0.64 image hash each large-art request carried
    /// (album/artist id -> hash), recorded when the load is dispatched and
    /// removed when a load carries no hash. A mini that later arrives at the
    /// rows' current, different hash marks the cached large art stale
    /// (`handle_artwork_loaded`). Its own map because the passive surfaces
    /// write `None` into `album_art_versions`. Unbounded like the version
    /// map; reset on logout via `Default`.
    pub large_artwork_hashes: HashMap<String, String>,
    /// The big panel's gate ([`PanelArtGate`]): one full-size cover on its
    /// way at a time, the newest waiting row next.
    pub panel_gate: PanelArtGate,
    /// In-flight large-art fetches ([`InFlight`]): album/artist id (the id
    /// space of [`Self::large_artwork`]) -> the version the request carries
    /// (image hash on Navidrome 0.64+, else `updated_at`). Every centering
    /// re-dispatches the load, and a held arrow key at the end of a list
    /// re-centers the same row on every repeat. Released by
    /// `handle_large_artwork_loaded` (albums and artists).
    pub large_artwork_pending: InFlight<Option<String>>,
    /// Mini radio-station artwork (`station_id -> Handle`) for the Radios slot
    /// list. Holds either an admin-uploaded station logo (fetched via
    /// `getCoverArt?id=ra-…` when [`RadioStation::logo_cover_art`] is present)
    /// or, for logo-less stations, the remembered last-played stream (ICY) art
    /// hydrated from the on-disk [`RadioArtStore`]. Reset on logout via
    /// `Default`; the disk store re-hydrates it next launch.
    ///
    /// [`RadioStation::logo_cover_art`]: nokkvi_data::types::radio_station::RadioStation::logo_cover_art
    /// [`RadioArtStore`]: nokkvi_data::services::radio_art_store::RadioArtStore
    pub radio_art: SnapshottedLru<String, image::Handle>,
    /// Station ids with an in-flight 80px logo fetch ([`InFlight`]),
    /// released by `handle_radio_art_loaded`.
    pub radio_art_pending: InFlight<()>,
    /// Large radio-station artwork (`station_id -> Handle`) for the artwork
    /// panel: a resolution-sized station logo (logo stations) or the
    /// remembered/now-playing stream (ICY) image (logo-less stations). A logo
    /// station's live track art is NOT stored here — its logo is its stable
    /// in-app identity. Falls back to [`Self::radio_art`] then the radio-tower
    /// glyph at the view layer.
    pub radio_large_art: SnapshottedLru<String, image::Handle>,
    /// In-flight large-logo fetches ([`InFlight`]): station id -> the logo
    /// token it requests, released by `RadioLargeLoaded`.
    pub radio_large_art_pending: InFlight<String>,
    /// Per-station dedup of the last ICY `StreamUrl` we fetched now-playing art
    /// from (`station_id -> url`). The playback tick re-parses ICY metadata
    /// every ~100ms, so this gates the external fetch to fire only when the
    /// URL actually changes (a new track). Seeded from the on-disk
    /// `RadioArtStore` on launch so a restart doesn't re-fetch unchanged art.
    pub radio_icy_captured: HashMap<String, String>,
    /// Whether remembered radio art has been hydrated from disk this session.
    /// A one-shot gate — probing `radio_icy_captured` emptiness conflates
    /// capture-state with hydration-state and re-reads disk on an empty store.
    /// Reset with the rest of `ArtworkState` on logout.
    pub radio_art_hydrated: bool,
    /// Genre artwork cache.
    pub genre: CollageArtworkCache,
    /// Playlist artwork cache.
    pub playlist: CollageArtworkCache,
    /// Mini CUSTOM playlist artwork (`playlist_id -> Handle`): the
    /// user-uploaded cover fetched via `getCoverArt?id=pl-<id>` at 80px for
    /// the slot-row thumbnail. Populated only for playlists whose
    /// `uploaded_image` is set; takes display precedence over the 2×2 quad /
    /// collage mini. Mirrors [`Self::radio_art`]'s shape.
    pub playlist_custom_art: SnapshottedLru<String, image::Handle>,
    /// Large CUSTOM playlist artwork (`playlist_id -> Handle`) at the user's
    /// `artwork_resolution` for the artwork panel. Falls back to
    /// [`Self::playlist_custom_art`] at the view layer while loading.
    /// Mirrors [`Self::radio_large_art`]'s shape.
    pub playlist_custom_large_art: SnapshottedLru<String, image::Handle>,
    /// In-flight large custom-cover fetches ([`InFlight`]): playlist id -> the
    /// cache-buster version it carries, released by
    /// `PlaylistCustomLargeLoaded`.
    pub playlist_custom_large_art_pending: InFlight<String>,
    /// Playlist ids with an in-flight custom-cover mini fetch ([`InFlight`]):
    /// the viewport prefetch re-dispatches on every scroll step. Released by
    /// `handle_playlist_custom_mini_loaded`.
    pub playlist_custom_art_pending: InFlight<()>,
    /// `playlist_id -> the version that warmed its custom mini` (the image
    /// hash on Navidrome 0.64+, else the `updated_at` cache-buster). A cover
    /// replaced in the web UI changes it, so the version-aware prefetch gate
    /// re-fetches it this
    /// session instead of serving the stale handle forever. Mirrors
    /// [`Self::album_art_versions`]; reset wholesale on logout via `Default`.
    pub playlist_custom_art_versions: HashMap<String, Option<String>>,
    /// Negative cache for custom-cover minis: `playlist_id -> the updated_at
    /// that failed` (code-70 "Artwork not found" — a stale `uploaded_image`
    /// flag whose image is gone server-side). Stops a known-dead id from
    /// re-firing on every scroll step; a CHANGED updated_at bypasses the
    /// entry. Mirrors [`Self::failed_art`].
    pub playlist_custom_art_failed: HashMap<String, Option<String>>,
    /// The most recent large-art request (album or artist id): what the
    /// centered panel is waiting on. Not a dedup gate; that is
    /// [`Self::large_artwork_pending`].
    pub loading_large_artwork: Option<String>,
    /// The one blurred variant of the now-playing cover for the lyrics
    /// backdrop (single slot, not an LRU: only the CURRENT track's cover is
    /// ever frosted, and the sharp source stays in [`Self::large_artwork`]).
    /// `handle: None` records a decode failure for this exact source so the
    /// tick doesn't re-dispatch a doomed blur every 100 ms. Keyed by the
    /// SOURCE handle's unique id — a server-side cover refresh mints a new
    /// handle id and invalidates this slot automatically.
    pub lyrics_blurred: Option<LyricsBlurredCover>,
    /// In-flight blur guard `(album_id, level)` — the blur runs on a blocking
    /// thread; without this the 100 ms tick would pile up duplicate jobs.
    pub lyrics_blur_pending: Option<(String, LyricsBackdropBlur)>,
}

/// The cached blurred cover behind the lyrics overlay. See
/// [`ArtworkState::lyrics_blurred`].
#[derive(Debug, Clone)]
pub struct LyricsBlurredCover {
    pub album_id: String,
    /// Identity of the sharp source handle this blur was computed from.
    pub source_id: iced::advanced::image::Id,
    pub level: LyricsBackdropBlur,
    /// `None` = the source failed to decode; negative-cached.
    pub handle: Option<image::Handle>,
    /// Whether the renderer has answered the request to keep `handle` on the
    /// GPU (`hold_lyrics_covers`). The view swaps to the frosted cover only
    /// from then on, so it draws on the very next frame: iced 0.15 decodes and
    /// uploads a picture off-thread the first time it is drawn and draws
    /// nothing until then, which blanked the cover for a frame at each swap.
    /// Also set when no window could take the request, so the frost still
    /// shows.
    pub frosted_ready: bool,
    /// What the renderer granted for the frosted cover and its sharp source:
    /// held, they stay on the GPU while this entry lives (iced frees a
    /// picture that is neither drawn nor allocated), so swapping between them
    /// either way never waits on a fresh decode.
    pub allocations: Vec<image::Allocation>,
}

/// How many pixels of full-size covers are held on the GPU after they leave
/// the screen ([`ArtworkState::large_on_gpu`]): 16 MP is 64 MB of RGBA,
/// sixteen 1000 px covers or four 2000 px ones. A pixel budget rather than a
/// count, because the Artwork Resolution setting goes up to the original.
pub(crate) const READY_COVER_PIXEL_BUDGET: u64 = 16_000_000;

/// Values held newest first within a size budget. The newest is always
/// held, whatever its size.
#[derive(Clone)]
pub(crate) struct Budgeted<T> {
    /// `(id, size, value)`, newest first.
    held: std::collections::VecDeque<(String, u64, T)>,
    budget: u64,
}

impl<T> Budgeted<T> {
    pub(crate) fn new(budget: u64) -> Self {
        Self {
            held: std::collections::VecDeque::new(),
            budget,
        }
    }

    /// Hold `value` for `id` as the newest, in place of any older value for
    /// that id, then let go of the oldest while the total is over budget.
    pub(crate) fn hold(&mut self, id: String, size: u64, value: T) {
        self.release(&id);
        self.held.push_front((id, size, value));
        while self.held.len() > 1 && self.total() > self.budget {
            self.held.pop_back();
        }
    }

    /// Mark `id` as the newest. Returns whether it is held.
    pub(crate) fn touch(&mut self, id: &str) -> bool {
        match self.held.iter().position(|(held, _, _)| held == id) {
            Some(at) => {
                if let Some(entry) = self.held.remove(at) {
                    self.held.push_front(entry);
                }
                true
            }
            None => false,
        }
    }

    /// Let go of `id`'s value.
    pub(crate) fn release(&mut self, id: &str) {
        self.held.retain(|(held, _, _)| held != id);
    }

    #[cfg(test)]
    pub(crate) fn holds(&self, id: &str) -> bool {
        self.held.iter().any(|(held, _, _)| held == id)
    }

    pub(crate) fn len(&self) -> usize {
        self.held.len()
    }

    fn total(&self) -> u64 {
        self.held.iter().map(|(_, size, _)| size).sum()
    }
}

/// The bytes a cover's handle keeps in memory.
fn cover_bytes(cover: &image::Handle) -> usize {
    match cover {
        image::Handle::Bytes(_, bytes) => bytes.len(),
        image::Handle::Rgba { pixels, .. } => pixels.len(),
        image::Handle::Path(..) => 0,
    }
}

impl ArtworkState {
    /// Give the panel's cache the full-size cover of `id` as its newest:
    /// one that just reached the GPU, or one shown again (which marks it as
    /// used). The least recently used covers then leave while the cache is
    /// over [`LARGE_ARTWORK_BYTE_BUDGET`]. Two covers stay whatever the
    /// total: the one just given (the panel is about to draw it), and
    /// `playing`'s, the playing song's, which the now-playing surfaces read
    /// the whole song long with no centring to mark it as used. A cover
    /// that left is fetched again when its row is next centred.
    pub(crate) fn cache_large_cover(
        &mut self,
        id: String,
        cover: image::Handle,
        playing: Option<&str>,
    ) {
        self.large_artwork.put(id.clone(), cover);
        let mut total: usize = self
            .large_artwork
            .iter()
            .map(|(_, cover)| cover_bytes(cover))
            .sum();
        if total <= LARGE_ARTWORK_BYTE_BUDGET {
            return;
        }
        // Oldest first.
        let mut leaving = Vec::new();
        for (key, cover) in self.large_artwork.iter().rev() {
            if total <= LARGE_ARTWORK_BYTE_BUDGET {
                break;
            }
            if *key == id || playing == Some(key.as_str()) {
                continue;
            }
            total -= cover_bytes(cover);
            leaving.push(key.clone());
        }
        for key in leaving {
            self.large_artwork.pop(&key);
        }
    }

    /// Forget `id`'s full-size cover wherever it is: shown, on its way onto
    /// the GPU, or held there. For a cover that changed on the server or is
    /// being refreshed; an allocation answer for the dropped cover finds
    /// nothing staged and changes nothing.
    pub(crate) fn drop_large_cover(&mut self, id: &str) {
        self.large_artwork.pop(&id.to_string());
        self.large_staging.remove(id);
        self.large_on_gpu.release(id);
    }
}

impl Default for ArtworkState {
    fn default() -> Self {
        Self {
            album_art: SnapshottedLru::new(MINI_ARTWORK_CACHE_CAPACITY),
            album_art_versions: HashMap::new(),
            album_art_pending: HashSet::new(),
            failed_art: HashMap::new(),
            placeholders: SnapshottedLru::new(PLACEHOLDER_CACHE_CAPACITY),
            large_artwork: SnapshottedLru::new(LARGE_ARTWORK_CACHE_CAPACITY),
            large_staging: HashMap::new(),
            large_on_gpu: Budgeted::new(READY_COVER_PIXEL_BUDGET),
            large_artwork_hashes: HashMap::new(),
            large_artwork_pending: InFlight::default(),
            panel_gate: PanelArtGate::default(),
            radio_art: SnapshottedLru::new(RADIO_ART_CACHE_CAPACITY),
            radio_art_pending: InFlight::default(),
            radio_large_art: SnapshottedLru::new(RADIO_LARGE_ART_CACHE_CAPACITY),
            radio_large_art_pending: InFlight::default(),
            radio_icy_captured: HashMap::new(),
            radio_art_hydrated: false,
            genre: CollageArtworkCache::new(),
            playlist: CollageArtworkCache::new(),
            playlist_custom_art: SnapshottedLru::new(PLAYLIST_CUSTOM_ART_CACHE_CAPACITY),
            playlist_custom_large_art: SnapshottedLru::new(
                PLAYLIST_CUSTOM_LARGE_ART_CACHE_CAPACITY,
            ),
            playlist_custom_art_pending: InFlight::default(),
            playlist_custom_large_art_pending: InFlight::default(),
            playlist_custom_art_versions: HashMap::new(),
            playlist_custom_art_failed: HashMap::new(),
            loading_large_artwork: None,
            lyrics_blurred: None,
            lyrics_blur_pending: None,
        }
    }
}

impl std::fmt::Debug for ArtworkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtworkState")
            .field("album_art", &self.album_art)
            .field("album_art_versions", &self.album_art_versions)
            .field("large_artwork", &self.large_artwork)
            .field("large_staging", &self.large_staging.len())
            .field("large_on_gpu", &self.large_on_gpu.len())
            .field("radio_art", &self.radio_art)
            .field("radio_large_art", &self.radio_large_art)
            .field("genre", &self.genre)
            .field("playlist", &self.playlist)
            .field("playlist_custom_art", &self.playlist_custom_art)
            .field("playlist_custom_large_art", &self.playlist_custom_large_art)
            .field("loading_large_artwork", &self.loading_large_artwork)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{InFlight, Instant, PanelArt, PanelArtGate, PanelArtStep};
    use crate::View;

    fn album(id: &str) -> PanelArt {
        PanelArt::Album(id.to_string())
    }

    const ALBUMS: Option<View> = Some(View::Albums);

    #[test]
    fn one_cover_on_its_way_and_the_newest_row_waits_for_it() {
        let mut gate = PanelArtGate::default();
        assert_eq!(
            gate.step(ALBUMS, album("a"), false),
            PanelArtStep::Now(album("a"))
        );
        gate.sent(album("a"));

        // A held key while "a" is on its way: each row replaces the last.
        assert_eq!(
            gate.step(ALBUMS, album("b"), false),
            PanelArtStep::Wait(album("b"))
        );
        assert_eq!(
            gate.step(ALBUMS, album("c"), false),
            PanelArtStep::Wait(album("c"))
        );

        assert_eq!(
            gate.landed("x"),
            None,
            "another cover landing opens nothing"
        );
        assert_eq!(
            gate.landed("a"),
            Some((ALBUMS, album("c"))),
            "only the newest goes"
        );
        assert_eq!(gate.landed("a"), None, "and only once");

        // Nothing on its way: the next step goes at once.
        assert_eq!(
            gate.step(ALBUMS, album("d"), false),
            PanelArtStep::Now(album("d"))
        );
    }

    #[test]
    fn a_cached_cover_goes_now_and_leaves_nothing_waiting() {
        let mut gate = PanelArtGate::default();
        gate.sent(album("a"));
        assert_eq!(
            gate.step(ALBUMS, album("b"), false),
            PanelArtStep::Wait(album("b"))
        );
        assert_eq!(
            gate.step(ALBUMS, album("c"), true),
            PanelArtStep::Now(album("c"))
        );
        assert_eq!(gate.landed("a"), None, "the passed row is not fetched");
    }

    #[test]
    fn an_overdue_timer_lets_only_the_newest_waiting_row_through() {
        let mut gate = PanelArtGate::default();
        gate.sent(album("a"));
        gate.step(ALBUMS, album("b"), false);
        gate.step(ALBUMS, album("c"), false);

        assert!(
            !gate.overdue(&album("b")),
            "a replaced row's timer does nothing"
        );
        assert!(gate.overdue(&album("c")));
        assert!(!gate.overdue(&album("c")), "once");
        assert_eq!(
            gate.landed("a"),
            None,
            "the slow cover then lands to nothing waiting"
        );
    }

    /// A gate whose last cover took `landing_ms` to land, and the moment
    /// that was measured.
    fn gate_with_landing(landing_ms: u64) -> (PanelArtGate, Instant) {
        let mut gate = PanelArtGate::default();
        let start = Instant::now();
        gate.sent_at(album("first"), start);
        let landed = start + Duration::from_millis(landing_ms);
        assert_eq!(gate.landed_at("first", landed), None);
        (gate, landed)
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// A held key repeats every 40 ms; a cover that takes 45 ms to land
    /// could not show before its row has left the centre. Those rows fetch
    /// nothing: each waits a quarter of a step longer than the steps are
    /// apart, and only the one the run stops on is fetched.
    #[test]
    fn a_run_of_steps_faster_than_a_cover_lands_fetches_only_where_it_stops() {
        let (mut gate, t) = gate_with_landing(45);

        // The first step after a rest loads at once, as a tap does.
        assert_eq!(
            gate.step_at(ALBUMS, album("a"), false, t + ms(1000)),
            PanelArtStep::Now(album("a"))
        );
        gate.sent_at(album("a"), t + ms(1000));

        let paced = |target: PanelArt| PanelArtStep::Paced {
            target,
            after: ms(50),
        };
        assert_eq!(
            gate.step_at(ALBUMS, album("b"), false, t + ms(1040)),
            paced(album("b"))
        );
        // The first step's cover lands mid-run: the paced row is not its to
        // send (it would land after the row left, like every one before).
        assert_eq!(gate.landed_at("a", t + ms(1045)), None);
        assert_eq!(
            gate.step_at(ALBUMS, album("c"), false, t + ms(1080)),
            paced(album("c")),
            "paced with nothing on its way too"
        );

        assert!(
            !gate.overdue(&album("b")),
            "a passed row's timer does nothing"
        );
        assert!(gate.overdue(&album("c")), "the run stopped on c");
    }

    /// Letting go of the key is the run stopping: the row it stopped on is
    /// handed back at once, and its timer then finds nothing waiting. A row
    /// that waits for the cover on its way is not a paced run's and stays.
    #[test]
    fn a_key_release_ends_a_paced_run_at_once() {
        let (mut gate, t) = gate_with_landing(45);
        gate.step_at(ALBUMS, album("a"), false, t + ms(1000));
        gate.step_at(ALBUMS, album("b"), false, t + ms(1040));
        assert_eq!(gate.run_stopped(), Some((ALBUMS, album("b"))));
        assert_eq!(gate.run_stopped(), None, "once");
        assert!(!gate.overdue(&album("b")), "its timer does nothing after");

        gate.sent_at(album("b"), t + ms(1050));
        assert_eq!(
            gate.step_at(ALBUMS, album("c"), false, t + ms(1200)),
            PanelArtStep::Wait(album("c"))
        );
        assert_eq!(gate.run_stopped(), None, "c waits for b to land");
        assert_eq!(
            gate.landed_at("b", t + ms(1300)),
            Some((ALBUMS, album("c")))
        );
    }

    /// Taps and wheel notches come slower than a held key: each one loads
    /// at once, or waits for the cover on its way and follows its landing,
    /// exactly as before the gate paced anything.
    #[test]
    fn steps_slower_than_a_held_key_are_never_paced() {
        let (mut gate, t) = gate_with_landing(300);

        assert_eq!(
            gate.step_at(ALBUMS, album("a"), false, t + ms(1000)),
            PanelArtStep::Now(album("a"))
        );
        gate.sent_at(album("a"), t + ms(1000));
        assert_eq!(
            gate.step_at(ALBUMS, album("b"), false, t + ms(1060)),
            PanelArtStep::Wait(album("b")),
            "60 ms apart is a tap: it waits for the cover, not for a timer"
        );
        assert_eq!(
            gate.landed_at("a", t + ms(1300)),
            Some((ALBUMS, album("b"))),
            "and follows its landing"
        );
        assert_eq!(
            gate.step_at(ALBUMS, album("c"), false, t + ms(1400)),
            PanelArtStep::Now(album("c"))
        );
    }

    /// A server that lands a cover between two repeats of a held key (one
    /// on this machine) shows every row's cover as it passes: nothing waits.
    #[test]
    fn a_server_that_lands_covers_between_steps_is_never_paced() {
        let (mut gate, t) = gate_with_landing(30);
        for (n, at) in [1000, 1040, 1080].into_iter().enumerate() {
            let row = album(&format!("a{n}"));
            assert_eq!(
                gate.step_at(ALBUMS, row.clone(), false, t + ms(at)),
                PanelArtStep::Now(row.clone())
            );
            gate.sent_at(row, t + ms(at));
            assert_eq!(gate.landed_at(&format!("a{n}"), t + ms(at + 30)), None);
        }

        // Nor is anything paced before a landing was ever measured, or on a
        // landing too fast to tell from one frame.
        let mut fresh = PanelArtGate::default();
        let (mut fast, _) = gate_with_landing(10);
        for gate in [&mut fresh, &mut fast] {
            assert_eq!(
                gate.step_at(ALBUMS, album("a"), false, t + ms(2000)),
                PanelArtStep::Now(album("a"))
            );
            assert_eq!(
                gate.step_at(ALBUMS, album("b"), false, t + ms(2005)),
                PanelArtStep::Now(album("b"))
            );
        }
    }

    /// A cached cover shows at once in the middle of a paced run, and the
    /// row that was waiting is dropped with it.
    #[test]
    fn a_cached_cover_goes_now_in_a_paced_run() {
        let (mut gate, t) = gate_with_landing(45);
        gate.step_at(ALBUMS, album("a"), false, t + ms(1000));
        assert!(matches!(
            gate.step_at(ALBUMS, album("b"), false, t + ms(1040)),
            PanelArtStep::Paced { .. }
        ));
        assert_eq!(
            gate.step_at(ALBUMS, album("c"), true, t + ms(1080)),
            PanelArtStep::Now(album("c"))
        );
        assert!(!gate.overdue(&album("b")));
    }

    #[test]
    fn in_flight_holds_only_the_claimed_version() {
        let mut gate = InFlight::<String>::default();
        gate.claim("a".into(), "v1".into());
        assert!(gate.holds("a", &"v1".to_string()), "an exact repeat waits");
        assert!(
            !gate.holds("a", &"v2".to_string()),
            "a replaced cover goes out, so the gate never pins the old one"
        );
        assert!(gate.contains("a"));

        gate.claim("a".into(), "v2".into());
        assert!(gate.holds("a", &"v2".to_string()), "the newer claim wins");

        gate.release("a");
        assert!(!gate.contains("a"));
        gate.claim("b".into(), "v1".into());
        gate.forget("b");
        assert!(gate.is_empty());
    }
}

#[cfg(test)]
mod budgeted_tests {
    use super::Budgeted;

    fn ids(held: &Budgeted<()>) -> Vec<&str> {
        held.held.iter().map(|(id, _, _)| id.as_str()).collect()
    }

    #[test]
    fn the_newest_values_are_held_within_the_budget() {
        let mut held = Budgeted::new(10);
        held.hold("a".into(), 4, ());
        held.hold("b".into(), 4, ());
        assert_eq!(ids(&held), ["b", "a"]);
        held.hold("c".into(), 4, ());
        assert_eq!(ids(&held), ["c", "b"], "the oldest gives way");
        assert!(held.holds("b") && !held.holds("a"));
    }

    /// The cover on screen is never let go of, however large it is.
    #[test]
    fn the_newest_is_held_even_when_it_alone_is_over_budget() {
        let mut held = Budgeted::new(10);
        held.hold("a".into(), 4, ());
        held.hold("huge".into(), 50, ());
        assert_eq!(ids(&held), ["huge"]);
    }

    #[test]
    fn a_touched_value_becomes_the_newest() {
        let mut held = Budgeted::new(10);
        held.hold("a".into(), 4, ());
        held.hold("b".into(), 4, ());
        assert!(held.touch("a"));
        held.hold("c".into(), 4, ());
        assert_eq!(ids(&held), ["c", "a"], "b was the oldest after the touch");
        assert!(!held.touch("gone"));
    }

    #[test]
    fn holding_an_id_again_replaces_its_value() {
        let mut held = Budgeted::new(10);
        held.hold("a".into(), 4, ());
        held.hold("b".into(), 4, ());
        held.hold("a".into(), 6, ());
        assert_eq!(ids(&held), ["a", "b"]);
        assert_eq!(held.len(), 2);
        held.release("a");
        assert_eq!(ids(&held), ["b"]);
    }
}

#[cfg(test)]
mod large_cover_cache_tests {
    use iced::widget::image;

    use super::ArtworkState;

    /// A cover of `mib` MiB. Its bytes are never read, so the test pays for
    /// none of them.
    fn cover(mib: usize) -> image::Handle {
        image::Handle::from_bytes(vec![0u8; mib * 1024 * 1024])
    }

    /// The cached ids, newest first.
    fn cached(state: &ArtworkState) -> Vec<&str> {
        state
            .large_artwork
            .iter()
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// Three 20 MiB covers fit the 64 MiB budget; a fourth does not.
    #[test]
    fn the_oldest_covers_give_way_over_the_byte_budget() {
        let mut state = ArtworkState::default();
        for id in ["a", "b", "c"] {
            state.cache_large_cover(id.into(), cover(20), None);
        }
        assert_eq!(cached(&state), ["c", "b", "a"]);

        state.cache_large_cover("d".into(), cover(20), None);
        assert_eq!(cached(&state), ["d", "c", "b"], "the oldest gives way");
        assert!(!state.large_artwork.snapshot.contains_key("a"));
    }

    /// The cover the panel is about to draw is kept, however large it is.
    #[test]
    fn the_cover_just_cached_is_kept_even_when_it_alone_is_over_budget() {
        let mut state = ArtworkState::default();
        state.cache_large_cover("a".into(), cover(20), None);
        state.cache_large_cover("huge".into(), cover(70), None);
        assert_eq!(cached(&state), ["huge"]);
    }

    /// Centring a cached cover caches it again: it is the newest then, and
    /// the next one over budget takes an older cover's place.
    #[test]
    fn a_cover_cached_again_becomes_the_newest() {
        let mut state = ArtworkState::default();
        let first = cover(20);
        state.cache_large_cover("a".into(), first.clone(), None);
        state.cache_large_cover("b".into(), cover(20), None);
        state.cache_large_cover("c".into(), cover(20), None);
        state.cache_large_cover("a".into(), first, None);

        state.cache_large_cover("d".into(), cover(20), None);
        assert_eq!(cached(&state), ["d", "a", "c"], "b was the oldest");
    }

    /// The now-playing surfaces (Queue panel, Theater, the lyrics backdrop)
    /// read the playing song's cover for as long as it plays, and nothing
    /// marks it as used meanwhile: browsing past it must not drop it.
    #[test]
    fn the_playing_songs_cover_is_never_let_go_of() {
        let mut state = ArtworkState::default();
        for id in ["playing", "b", "c", "d", "e"] {
            state.cache_large_cover(id.into(), cover(20), Some("playing"));
        }
        assert_eq!(cached(&state), ["e", "d", "playing"]);
    }

    /// Small covers (a low Artwork Resolution, a JPEG-serving server) never
    /// reach the byte budget: the entry count bounds them as before.
    #[test]
    fn small_covers_fill_the_whole_entry_capacity() {
        let mut state = ArtworkState::default();
        for n in 0..250 {
            let small = image::Handle::from_bytes(vec![0u8; 16]);
            state.cache_large_cover(format!("a{n}"), small, None);
        }
        assert_eq!(state.large_artwork.len(), 200);
    }
}
