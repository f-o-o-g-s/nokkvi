# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- New `nokkvi <verb>` CLI for scripting and WM hotkeys — 16 verbs covering transport, volume, queue, view-switching, and `love`/`rate` on the currently-playing track.

### Changed

### Fixed

- Lock, heart, and star outline icons in slot-list rows now darken in lockstep with the row's text when the row is selected (centered, multi-selected, or currently playing) — previously they kept their muted light tint against the light selected-row fill and were hard to read. Most visible on private playlists in the Playlists view, but the heart and star outlines had the same issue under multi-selection (ctrl-click) on every slot-list view.

### Removed

## v0.4.2 — 2026-05-19

### Changed

- Roulette spin no longer plays automatically — the wheel cruises until you press Enter, then decelerates onto the picked song. Escape during the spin still cancels without auto-play. The cruise rate and decel walk are also tuned so each visible click advances a small number of items instead of skipping hundreds per click on libraries above a few thousand songs.

### Fixed

- Setting `queue_show_genre = true` or `songs_show_genre = true` directly in `config.toml` now actually shows the Genre column in the Queue or Songs view on next launch. Both keys shipped in the on-disk TOML format with full serde wiring, but the apply-side path had no handler for them, so the value was silently snapped back to off on every load regardless of what the file said. Toggling the column from the settings UI worked normally; only direct file edits and pre-existing config files with these keys set were affected.
- EQ modal's preset dropdown now shows the correct saved custom preset as active after restarting nokkvi — previously the dropdown fell back to "Custom" even though your saved preset was still loaded and applied.
- Settings → Hotkeys row now updates to the new key right after you rebind it — previously the row kept showing the old key even though the new binding was already saved and working.
- Reordering the queue while a song is playing (drag-and-drop, Shift+↑/↓, right-click "Remove from queue" or "Play Next", sort, or clear) no longer leaves the player bar's title and the queue view's highlight out of sync with what's actually playing. The desync was most visible with shuffle and crossfade both on — every reorder now resets nokkvi's pre-buffered "next song" choice so the displayed track follows the actual audio.
- Logging out now clears the same session-bound state as session-expiry — the hamburger menu closes, library data resets, and pending find-and-expand chains, in-progress roulette spins, and similar-songs results no longer carry over into the next login.
- Album artwork no longer briefly flickers after every track. Navidrome's `library-changed` SSE event fires on every play-count bump, and the previous handler treated each one as a signal to re-fetch and re-upload the album's large artwork. SSE no longer auto-refreshes artwork; cover-art replacements still surface on the next library reload or via the right-click "Refresh Artwork" action.
- Queue's vertical artwork no longer sits flush against the playlist context bar or playlist edit bar when one of those bars is showing — there's now a small bottom gap matching the artwork's other insets in Always-Vertical Native / Stretched and Auto's portrait-fallback modes.
- Play, "Center on Playing", and multi-row drag-reorder in the queue now reliably target the clicked/dragged row even with duplicates of the same song in the queue, or when a second drag is started before the first has fully settled. v0.4.1 closed this gap for right-click and Delete; the same per-row addressing now covers Play, Focus, and batch Move as well.

## v0.4.1 — 2026-05-17

### Fixed

- Right-clicking one of two duplicate songs in the queue (or removing one via multi-select Delete / Ctrl+D) now removes only the clicked row instead of dropping both copies. Each queue row now carries a runtime per-entry id so duplicates of the same song are individually addressable; existing on-disk queue snapshots remain compatible.
- Dragging songs from the library pane into the queue pane (split-view) now drops at the slot under the cursor across all artwork-column modes (Always-Vertical Native/Stretched, Auto with portrait fallback), nav layouts (Top/Side/None), playlist edit mode, and at scrolled queue positions — previously the drag could wrong-track the source row or land on the wrong queue slot in those combinations. The drag also fails-safe (cancels rather than dropping at a stale index) if the queue mutates mid-drag.
- Eliminated a silent worker-thread panic during radio→library track transitions.

## v0.4.0 — 2026-05-16

### Fixed

- Volume changes made by releasing the slider, scrolling the wheel, or sending an external D-Bus command (MPRIS, playerctl, headset buttons, hardware media keys) now reliably persist past the 500ms throttle. All three were previously routed through the drag-intermediate path, so a release that landed inside the throttle window or a discrete wheel notch / external command inside it could be silently dropped on the next launch.
- Two rapid mouse-wheel notches now produce two volume steps. The wheel handler previously based the second event off a render-time snapshot, so a second notch arriving before the next render computed from the stale pre-first base and overwrote the first.
- Ctrl-clicking an expanded child row (e.g. a track inside an Artists album expansion) now adds it to the multi-selection in legacy click-to-play mode. The child-row handler previously dispatched the play action on modifier-click and defeated multi-select.
- Playlist create, save, and delete writes now use a leading-slash path so reverse-proxied Navidrome deployments behind a URL prefix accept them. Read paths already had the leading slash.
- Songs view and per-genre Songs view now load complete libraries instead of stopping at 50_000 rows. The underlying Navidrome request had a hardcoded `_end=50000` sentinel that silently truncated libraries above that size; the load now paginates internally in 5_000-row chunks until the server returns a short page or the cumulative count matches `X-Total-Count`.
- Visualizer no longer flickers blank or freezes its peak bars during a crossfade between tracks of different sample rates (e.g. a 44.1 kHz FLAC into a 48 kHz one). Two concurrent streams were sharing the visualizer's stored sample rate and flipping it ~86 times per second, forcing the spectrum engine to reinitialize on every flip and clearing the FFT input buffer faster than it could refill. The visualizer now follows whichever stream is currently dominant in the fade, handing off at the equal-power midpoint, so the spectrum keeps animating continuously through the whole crossfade window.
- Seeking near the end of a track no longer causes a multi-second silent fade-in on the next track. The seek silently disarmed the pending gapless crossfade so the engine's position-based trigger never fired, and the crossfade fell through to its EOF-fallback path — firing the configured crossfade against an already-drained outgoing stream so the new track faded in from silence rather than overlapping with the previous one. Within-track seeks now preserve the armed crossfade state and the position trigger fires normally one crossfade-duration before track end.
- Opus-encoded tracks served from your Navidrome library now play, as do Opus Icecast radio streams. Symphonia ships no native Opus decoder, so any Opus file previously surfaced as "Failed to start playback: Failed to create decoder"; the decoder is now provided by a bundled libopus build, so there's no new runtime system dependency — though building nokkvi from source now needs `cmake` to compile the bundled decoder.

## Older releases

- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./CHANGELOG-0.3.md)
