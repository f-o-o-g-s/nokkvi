# Changelog

## v0.0.3 — 2026-03-19

### Fixes
- **install.sh copies binary** — `install.sh` now copies `target/release/nokkvi`
  to `~/.local/bin/`, so the desktop entry's `Exec=nokkvi` works without manual
  `$PATH` setup. Exits with a helpful message if the binary hasn't been built yet.

---

## v0.0.2 — 2026-03-19

### Features
- **Click track metadata to navigate** — clicking the title, artist, or album
  text in the track info strip (top bar, player bar, or side nav) navigates to
  the queue view. Codec and bitrate fields remain non-clickable.

### Fixes
- **Network playback stuttering** — audio no longer cuts in and out when the
  Navidrome server is on a different machine. Root cause was ring buffer
  starvation: the cpal audio callback consumed samples faster than the decoder
  could supply them over the network, producing silence on underruns.
  - Ring buffer increased from 2s to 5s (192K → 480K samples), giving more
    runway to absorb network latency spikes.
  - HTTP connection pooling re-enabled — previously every 256KB chunk fetch
    opened a new TCP connection, paying TLS handshake and TCP slow start on
    each request.
  - HTTP chunk cache doubled (8 → 16 chunks, ~4MB) to reduce re-fetches.
  - Sequential prefetch added — the HTTP reader now speculatively fetches the
    next chunk after each read, keeping the cache ahead of the decoder.
  - Pre-buffering increased: 5 → 15 chunks at playback start, 3 → 10 after
    seek, ensuring the ring buffer is well-filled before audio begins.

### Internal
- Agent rules and workflows synced with current codebase.

---

## v0.0.1 — 2026-03-19

Initial release.
