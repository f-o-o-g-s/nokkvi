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

use super::snapshotted_lru::SnapshottedLru;

/// Maximum entries in the large artwork LRU cache.
/// Each 500px image handle is ~80KB, so 200 entries ≈ 16MB cap.
const LARGE_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(200).expect("capacity must be > 0");
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

/// What [`PanelArtGate::step`] decided for a centred row.
#[derive(Debug, PartialEq, Eq)]
pub enum PanelArtStep {
    /// Fetch now: nothing is on its way, or the cover is already cached.
    Now(PanelArt),
    /// A cover is on its way: this row now waits as the newest, fetched when
    /// that cover lands, or after [`PANEL_ART_OVERDUE`]
    /// (`ArtworkMessage::PanelArtOverdue`) when it is slow.
    Wait(PanelArt),
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
/// Only steps in the list the user is steering reach the gate
/// (`Nokkvi::panel_art_step`); one waiting slot is enough because a step
/// from any other list loads directly. Reset on logout via
/// `ArtworkState::default()`, so a cover landing or an overdue timer from an
/// ended session finds nothing waiting.
#[derive(Debug, Clone, Default)]
pub struct PanelArtGate {
    /// The cover on its way, recorded once its loader claimed the fetch.
    in_flight: Option<PanelArt>,
    /// The newest row a step centred while that cover was on its way, with
    /// the list it was centred in.
    waiting: Option<(Option<crate::View>, PanelArt)>,
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
        if !cached && self.in_flight.is_some() {
            self.waiting = Some((source, target.clone()));
            PanelArtStep::Wait(target)
        } else {
            self.waiting = None;
            PanelArtStep::Now(target)
        }
    }

    /// Whether a row from a list other than `source` is waiting. In the split
    /// view both panes steer the panel, and a step in one must not replace or
    /// clear the row the other has waiting (its cover would never be sent):
    /// that step loads directly and leaves the gate alone.
    pub fn another_list_waits(&self, source: Option<crate::View>) -> bool {
        self.waiting
            .as_ref()
            .is_some_and(|(owner, _)| *owner != source)
    }

    /// A fetch for `target` went out: it is the cover on its way.
    pub fn sent(&mut self, target: PanelArt) {
        self.in_flight = Some(target);
    }

    /// A full-size cover for `id` landed, on any outcome. When it was the
    /// cover on its way, the gate opens and hands back the newest row that
    /// waited for it.
    pub fn landed(&mut self, id: &str) -> Option<(Option<crate::View>, PanelArt)> {
        if self
            .in_flight
            .as_ref()
            .is_some_and(|cover| cover.id() == id)
        {
            self.in_flight = None;
            self.waiting.take()
        } else {
            None
        }
    }

    /// The overdue timer of a waiting `target` fired: `true` when it is
    /// still the newest waiting row, which stops waiting (the caller fetches
    /// it past the slow cover).
    pub fn overdue(&mut self, target: &PanelArt) -> bool {
        if self.waiting.as_ref().is_some_and(|(_, row)| row == target) {
            self.waiting = None;
            true
        } else {
            false
        }
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
    /// cover on the next cold load.
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
    /// Large artwork cache for detail views (LRU-bounded).
    pub large_artwork: SnapshottedLru<String, image::Handle>,
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

impl Default for ArtworkState {
    fn default() -> Self {
        Self {
            album_art: SnapshottedLru::new(MINI_ARTWORK_CACHE_CAPACITY),
            album_art_versions: HashMap::new(),
            album_art_pending: HashSet::new(),
            failed_art: HashMap::new(),
            placeholders: SnapshottedLru::new(PLACEHOLDER_CACHE_CAPACITY),
            large_artwork: SnapshottedLru::new(LARGE_ARTWORK_CACHE_CAPACITY),
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
    use super::{InFlight, PanelArt, PanelArtGate, PanelArtStep};
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
