---
paths:
  - "src/views/radios.rs"
  - "src/update/{radios,radio_artwork,playlist_artwork,artwork}.rs"
  - "src/update/components/custom_artwork.rs"
  - "src/state/artwork.rs"
  - "src/services/mpris_art_writer.rs"
  - "src/update/tests/{radio_artwork,playlist_artwork}.rs"
---

# Station & Playlist Artwork

## Radio Station Artwork (Radios)

Station rows render artwork instead of the generic tower glyph. Fetch/persistence precedence (uploaded logo via getCoverArt vs remembered ICY art in `RadioArtStore`): see backend-services.md "RadioArtStore"; hydration runs once per session (`artwork.radio_art_hydrated` gate). Caches live on `Nokkvi.artwork`: `radio_art` / `radio_large_art` (`SnapshottedLru<String, Handle>` keyed by `station_id`) + `radio_icy_captured`. The large panel LOCKS to the playing station while a radio plays and follows the centered station otherwise, with the over-cover visualizer + boat drawn via the same shared panel helper as the Queue cover. Right-click → Refresh Artwork (`RadioContextEntry::RefreshArtwork`) clears memory + disk and refetches. Right-click → Set Custom Artwork… / Reset Artwork upload/DELETE a server-side logo via `POST|DELETE /api/radio/{id}/image` (rfd portal picker → one `shell_task`; on success SET clears via `clear_radio_station_art_handles` — KEEPING the ICY dedup record so the tick can't re-capture stream art over the new logo — while Reset/Refresh use the full `clear_radio_station_artwork_caches`; the station-list reload then refreshes the play-time `active_playback` station snapshot and re-warms row + panel from the fresh `coverArt` token). Handlers in `update/radio_artwork.rs`; `ArtworkMessage` carries the `RadioArt*` / `LoadRadioLarge` / `RadioIcyArtLoaded` / `RadioCustomArtwork{Set,Reset}` variants. MPRIS follows the same identity rule: a playing station publishes its logo, else its http(s) stream art, else the app icon (`art_candidates` in `services/mpris_art_writer.rs`). The playlist twin (custom cover beats the collage, `pl-<id>` fetches into `playlist_custom_art` / `playlist_custom_large_art`, viewport prefetch gated by the shared `should_refetch` + a pending set + a `playlist_id -> updated_at` versions map) lives in `update/playlist_artwork.rs`.
