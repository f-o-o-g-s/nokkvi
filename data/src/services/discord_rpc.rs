//! Discord Rich Presence over the local IPC socket.
//!
//! Discord's desktop client listens on a unix socket (`discord-ipc-0..9`).
//! A client sends a handshake carrying the user's application id, then
//! `SET_ACTIVITY` frames. The whole protocol is length-prefixed JSON, so this
//! talks to the socket directly instead of pulling in a crate.
//!
//! [`DiscordRpc`] owns one worker thread. The UI hands it the desired
//! [`Presence`] (or `None` to clear it); the worker connects when Discord is
//! running, re-sends the latest activity after a reconnect, and keeps clear of
//! Discord's activity rate limit. Dropping the handle ends the thread.
//!
//! Cover art goes out only as a URL [`usable_cover_url`] accepts: Navidrome's
//! `getCoverArt` links carry the session credential, and an activity image
//! URL is visible to everyone, so only the server's public `share/img` links
//! (from `getAlbumInfo2`) qualify.

use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use tracing::{debug, trace, warn};

/// Handshake / activity frame opcodes of the IPC protocol.
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;

/// Discord rejects `details` / `state` outside 2..=128 characters.
const TEXT_MIN_CHARS: usize = 2;
const TEXT_MAX_CHARS: usize = 128;

/// Discord allows five activity updates per 20 s; stay well under it.
const MIN_SEND_INTERVAL: Duration = Duration::from_secs(4);
/// How soon a missing Discord is looked for again.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(15);
/// Bound on a single socket read or write, so a wedged Discord cannot hang the worker.
const IO_TIMEOUT: Duration = Duration::from_secs(3);
/// Largest reply frame read; Discord's answers are a few hundred bytes.
const MAX_REPLY_BYTES: u32 = 64 * 1024;

/// What is playing, as Discord should show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `None` for a live stream (radio): no progress bar.
    pub duration_secs: Option<u32>,
    pub position_secs: u32,
    pub playing: bool,
    /// A public image link for the cover ([`usable_cover_url`] has vetted it).
    pub cover_url: Option<String>,
}

/// The identity of a presence for change detection: everything that changes
/// what Discord shows, with the elapsed time folded into the start stamp so a
/// seek registers while ordinary playback does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceKey {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: Option<u32>,
    pub playing: bool,
    pub cover_url: Option<String>,
    /// Unix second the track would have started at (`now - position`), `None` while paused.
    pub start_secs: Option<i64>,
}

/// A started stamp that drifted this far from the sent one is a seek.
const START_DRIFT_SECS: i64 = 3;

impl PresenceKey {
    pub fn of(presence: &Presence, now_unix: i64) -> Self {
        Self {
            title: presence.title.clone(),
            artist: presence.artist.clone(),
            album: presence.album.clone(),
            duration_secs: presence.duration_secs,
            playing: presence.playing,
            cover_url: presence.cover_url.clone(),
            start_secs: presence
                .playing
                .then(|| now_unix - i64::from(presence.position_secs)),
        }
    }

    /// Whether `self` (just computed) needs sending after `sent` went out.
    pub fn differs_from(&self, sent: &Self) -> bool {
        let start_moved = match (self.start_secs, sent.start_secs) {
            (Some(a), Some(b)) => (a - b).abs() > START_DRIFT_SECS,
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
        };
        start_moved
            || self.title != sent.title
            || self.artist != sent.artist
            || self.album != sent.album
            || self.duration_secs != sent.duration_secs
            || self.playing != sent.playing
            || self.cover_url != sent.cover_url
    }
}

/// Whether `now` must be sent given what was `sent` last: `None` for `sent`
/// means nothing went out yet; an inner `None` is a cleared presence.
pub fn presence_changed(sent: Option<&Option<PresenceKey>>, now: &Option<PresenceKey>) -> bool {
    match (sent, now) {
        (Some(None), None) => false,
        (Some(Some(sent)), Some(now)) => now.differs_from(sent),
        // Nothing went out yet / a clear followed by a track / a track followed by a clear.
        (None, _) | (Some(None), Some(_)) | (Some(Some(_)), None) => true,
    }
}

/// Query keys that carry a credential or session on Navidrome / Subsonic URLs.
const CREDENTIAL_QUERY_KEYS: [&str; 10] = [
    "u", "t", "s", "p", "apikey", "api_key", "token", "jwt", "password", "salt",
];

/// `url` when Discord can fetch it and showing it leaks nothing: http(s), a
/// public host (not localhost, `.local` or a private / loopback address that
/// Discord's servers cannot reach), and none of [`CREDENTIAL_QUERY_KEYS`] in
/// the query.
pub fn usable_cover_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let reachable = match parsed.host()? {
        url::Host::Domain(name) => {
            let name = name.to_ascii_lowercase();
            name != "localhost" && !name.ends_with(".local") && !name.ends_with(".localhost")
        }
        url::Host::Ipv4(ip) => {
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                // Carrier-grade NAT / Tailscale (100.64.0.0/10).
                || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1])))
        }
        url::Host::Ipv6(ip) => !(ip.is_loopback() || ip.is_unspecified()),
    };
    let carries_credential = parsed
        .query_pairs()
        .any(|(key, _)| CREDENTIAL_QUERY_KEYS.contains(&key.to_ascii_lowercase().as_str()));
    (reachable
        && !carries_credential
        && parsed.username().is_empty()
        && parsed.password().is_none())
    .then(|| parsed.into())
}

/// Fit `text` to Discord's 2..=128 character window.
fn fit_text(text: &str) -> String {
    let trimmed = text.trim();
    let mut out: String = trimmed.chars().take(TEXT_MAX_CHARS).collect();
    // Discord drops an activity whose line is a single character. U+2800 is a
    // blank that survives its trimming.
    while out.chars().count() < TEXT_MIN_CHARS {
        out.push('\u{2800}');
    }
    out
}

/// The `activity` object of a `SET_ACTIVITY` command.
pub fn activity_json(presence: &Presence, now_unix: i64) -> Value {
    // Type 2 is "Listening to": the progress bar reads as a track's.
    let mut activity = json!({
        "type": 2,
        "details": fit_text(&presence.title),
    });
    let artist = if presence.playing {
        presence.artist.clone()
    } else if presence.artist.trim().is_empty() {
        "Paused".to_owned()
    } else {
        format!("Paused · {}", presence.artist.trim())
    };
    if !artist.trim().is_empty() {
        activity["state"] = json!(fit_text(&artist));
    }
    let mut assets = serde_json::Map::new();
    if !presence.album.trim().is_empty() {
        assets.insert("large_text".into(), json!(fit_text(&presence.album)));
    }
    if let Some(url) = &presence.cover_url {
        assets.insert("large_image".into(), json!(url));
    }
    if !assets.is_empty() {
        activity["assets"] = Value::Object(assets);
    }
    if presence.playing {
        // Unix MILLISECONDS, as Feishin sends them (Discord's documented unit).
        let start = (now_unix - i64::from(presence.position_secs)) * 1000;
        let mut timestamps = json!({ "start": start });
        if let Some(duration) = presence.duration_secs.filter(|d| *d > 0) {
            timestamps["end"] = json!(start + i64::from(duration) * 1000);
        }
        activity["timestamps"] = timestamps;
    }
    activity
}

/// One IPC frame: opcode, length, JSON payload (little-endian header).
fn encode_frame(opcode: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string();
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(body.as_bytes());
    frame
}

/// `SET_ACTIVITY` payload; a `None` activity clears the presence.
fn set_activity_payload(activity: Option<Value>, pid: u32, nonce: u64) -> Value {
    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce.to_string(),
    })
}

/// Directories Discord may put its socket in, most specific first.
fn socket_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            dirs.push(PathBuf::from(value));
        }
    }
    dirs.push(PathBuf::from("/tmp"));
    // Sandboxed Discord builds keep the socket one level down.
    let sandboxed: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|d| {
            [
                d.join("app/com.discordapp.Discord"),
                d.join("app/com.discordapp.DiscordCanary"),
                d.join("snap.discord"),
            ]
        })
        .collect();
    dirs.extend(sandboxed);
    dirs.dedup();
    dirs
}

#[cfg(unix)]
mod conn {
    use std::os::unix::net::UnixStream;

    use super::*;

    pub struct Connection {
        stream: UnixStream,
    }

    impl Connection {
        /// Open the first live `discord-ipc-N` socket and complete the handshake.
        pub fn open(app_id: &str) -> Option<Self> {
            for dir in socket_dirs() {
                for n in 0..10 {
                    let path = dir.join(format!("discord-ipc-{n}"));
                    let Ok(stream) = UnixStream::connect(&path) else {
                        continue;
                    };
                    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
                        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
                    {
                        continue;
                    }
                    let mut conn = Self { stream };
                    let hello = json!({ "v": 1, "client_id": app_id });
                    if conn.write(OP_HANDSHAKE, &hello).is_err() {
                        continue;
                    }
                    match conn.read_reply() {
                        Ok(reply) if reply.get("evt").and_then(Value::as_str) == Some("READY") => {
                            debug!("Discord RPC connected via {}", path.display());
                            return Some(conn);
                        }
                        Ok(reply) => {
                            // An unknown application id answers with an error event.
                            warn!("Discord RPC handshake refused: {reply}");
                        }
                        Err(e) => trace!("Discord RPC handshake read failed: {e}"),
                    }
                }
            }
            None
        }

        pub fn write(&mut self, opcode: u32, payload: &Value) -> std::io::Result<()> {
            self.stream.write_all(&encode_frame(opcode, payload))
        }

        /// Read one reply frame (and discard what is past the size bound).
        pub fn read_reply(&mut self) -> std::io::Result<Value> {
            let mut header = [0u8; 8];
            self.stream.read_exact(&mut header)?;
            let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            if len > MAX_REPLY_BYTES {
                return Err(std::io::Error::other("oversized Discord reply"));
            }
            let mut body = vec![0u8; len as usize];
            self.stream.read_exact(&mut body)?;
            Ok(serde_json::from_slice(&body).unwrap_or(Value::Null))
        }
    }
}

#[cfg(not(unix))]
mod conn {
    use super::*;

    pub struct Connection;

    impl Connection {
        pub fn open(_app_id: &str) -> Option<Self> {
            None
        }

        pub fn write(&mut self, _opcode: u32, _payload: &Value) -> std::io::Result<()> {
            Ok(())
        }

        pub fn read_reply(&mut self) -> std::io::Result<Value> {
            Ok(Value::Null)
        }
    }
}

use conn::Connection;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Handle to the worker thread; the thread ends when this is dropped.
pub struct DiscordRpc {
    tx: mpsc::Sender<Option<Presence>>,
}

impl DiscordRpc {
    pub fn start(app_id: String) -> Self {
        let (tx, rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || run_worker(&app_id, &rx));
        if let Err(e) = spawned {
            warn!("Discord RPC thread failed to start: {e}");
        }
        Self { tx }
    }

    /// Show `presence`, or clear the activity with `None`.
    pub fn set(&self, presence: Option<Presence>) {
        // A closed channel means the worker is gone; there is nothing to do about it.
        if self.tx.send(presence).is_err() {
            debug!("Discord RPC worker is gone; presence dropped");
        }
    }
}

/// What the worker last wanted on screen, and what Discord was last told.
struct Worker<'a> {
    app_id: &'a str,
    conn: Option<Connection>,
    wanted: Option<Presence>,
    /// `true` once `wanted` is what Discord shows.
    delivered: bool,
    nonce: u64,
    last_send: Option<Instant>,
    last_connect_try: Option<Instant>,
}

impl Worker<'_> {
    fn flush(&mut self) {
        if self.delivered {
            return;
        }
        if self
            .last_send
            .is_some_and(|at| at.elapsed() < MIN_SEND_INTERVAL)
        {
            return;
        }
        if self.conn.is_none() {
            if self
                .last_connect_try
                .is_some_and(|at| at.elapsed() < RECONNECT_INTERVAL)
            {
                return;
            }
            self.last_connect_try = Some(Instant::now());
            self.conn = Connection::open(self.app_id);
        }
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        self.nonce += 1;
        let activity = self.wanted.as_ref().map(|p| activity_json(p, unix_now()));
        let payload = set_activity_payload(activity, std::process::id(), self.nonce);
        let sent = conn
            .write(OP_FRAME, &payload)
            .and_then(|()| conn.read_reply());
        self.last_send = Some(Instant::now());
        match sent {
            Ok(_) => self.delivered = true,
            Err(e) => {
                debug!("Discord RPC send failed, will reconnect: {e}");
                self.conn = None;
            }
        }
    }
}

fn run_worker(app_id: &str, rx: &mpsc::Receiver<Option<Presence>>) {
    let mut worker = Worker {
        app_id,
        conn: None,
        wanted: None,
        delivered: true,
        nonce: 0,
        last_send: None,
        last_connect_try: None,
    };
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(presence) => {
                worker.wanted = presence;
                worker.delivered = false;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        worker.flush();
    }
    // Leave no stale activity behind.
    if let Some(conn) = worker.conn.as_mut() {
        let clear = set_activity_payload(None, std::process::id(), worker.nonce + 1);
        if let Err(e) = conn.write(OP_FRAME, &clear) {
            debug!("Discord RPC clear on shutdown failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presence() -> Presence {
        Presence {
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_secs: Some(200),
            position_secs: 50,
            playing: true,
            cover_url: None,
        }
    }

    #[test]
    fn frame_has_little_endian_header_and_json_body() {
        let frame = encode_frame(OP_FRAME, &json!({"a": 1}));
        assert_eq!(&frame[..4], &1u32.to_le_bytes());
        let len = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
        assert_eq!(len, frame.len() - 8);
        assert_eq!(&frame[8..], br#"{"a":1}"#);
    }

    #[test]
    fn playing_activity_carries_timestamps_from_position() {
        let a = activity_json(&presence(), 1_000);
        assert_eq!(a["type"], 2);
        assert_eq!(a["details"], "Song");
        assert_eq!(a["state"], "Artist");
        assert_eq!(a["assets"]["large_text"], "Album");
        assert_eq!(a["timestamps"]["start"], 950_000);
        assert_eq!(a["timestamps"]["end"], 1_150_000);
    }

    #[test]
    fn a_stream_has_a_start_but_no_end() {
        let mut p = presence();
        p.duration_secs = None;
        let a = activity_json(&p, 1_000);
        assert_eq!(a["timestamps"]["start"], 950_000);
        assert!(a["timestamps"].get("end").is_none());
    }

    #[test]
    fn paused_activity_drops_timestamps_and_says_so() {
        let mut p = presence();
        p.playing = false;
        let a = activity_json(&p, 1_000);
        assert!(a.get("timestamps").is_none());
        assert_eq!(a["state"], "Paused · Artist");
    }

    #[test]
    fn empty_artist_and_album_are_left_out() {
        let mut p = presence();
        p.artist.clear();
        p.album.clear();
        let a = activity_json(&p, 1_000);
        assert!(a.get("state").is_none());
        assert!(a.get("assets").is_none());
    }

    #[test]
    fn a_cover_url_becomes_the_large_image_beside_the_album_text() {
        let mut p = presence();
        p.cover_url = Some("https://m.example/share/img/t?size=1200".into());
        let a = activity_json(&p, 1_000);
        assert_eq!(
            a["assets"]["large_image"],
            "https://m.example/share/img/t?size=1200"
        );
        assert_eq!(a["assets"]["large_text"], "Album");
        // No album text: the image still goes out.
        p.album.clear();
        assert!(activity_json(&p, 1_000)["assets"]["large_image"].is_string());
    }

    #[test]
    fn a_public_share_link_is_usable() {
        let url = "https://music.example.com/share/img/abc.def.ghi?size=1200";
        assert_eq!(usable_cover_url(url).as_deref(), Some(url));
    }

    #[test]
    fn a_link_with_credentials_is_refused() {
        for url in [
            "https://m.example/rest/getCoverArt?id=al-1&u=bob&t=abc&s=xyz",
            "https://m.example/rest/getCoverArt?id=al-1&apiKey=nds_secret",
            "https://m.example/img?size=1&JWT=x",
            "https://bob:pw@m.example/share/img/t",
        ] {
            assert_eq!(usable_cover_url(url), None, "{url}");
        }
    }

    #[test]
    fn a_link_discord_cannot_reach_is_refused() {
        for url in [
            "http://localhost:4533/share/img/t",
            "http://192.168.1.5:4533/share/img/t",
            "http://10.0.0.2/share/img/t",
            "http://127.0.0.1/share/img/t",
            "http://100.101.102.103/share/img/t",
            "http://nas.local/share/img/t",
            "ftp://m.example/share/img/t",
            "not a url",
        ] {
            assert_eq!(usable_cover_url(url), None, "{url}");
        }
        assert!(usable_cover_url("http://8.8.8.8/share/img/t").is_some());
    }

    #[test]
    fn a_cover_arriving_late_is_a_change() {
        let p = presence();
        let sent = PresenceKey::of(&p, 1_000);
        let mut with_cover = p;
        with_cover.cover_url = Some("https://m.example/share/img/t".into());
        assert!(PresenceKey::of(&with_cover, 1_000).differs_from(&sent));
    }

    #[test]
    fn text_is_fitted_to_discords_window() {
        assert_eq!(fit_text("x").chars().count(), 2);
        assert_eq!(fit_text(&"a".repeat(300)).chars().count(), 128);
        assert_eq!(fit_text("  hi  "), "hi");
    }

    #[test]
    fn set_activity_payload_nulls_the_activity_to_clear() {
        let p = set_activity_payload(None, 7, 3);
        assert_eq!(p["cmd"], "SET_ACTIVITY");
        assert_eq!(p["args"]["pid"], 7);
        assert!(p["args"]["activity"].is_null());
        assert_eq!(p["nonce"], "3");
    }

    #[test]
    fn steady_playback_is_not_a_change_but_a_seek_is() {
        let p = presence();
        let sent = PresenceKey::of(&p, 1_000);
        // Two seconds later the position advanced by two: same start stamp.
        let mut later = p.clone();
        later.position_secs += 2;
        assert!(!PresenceKey::of(&later, 1_002).differs_from(&sent));
        // A jump forward of 30 s moves the start stamp.
        later.position_secs = p.position_secs + 32;
        assert!(PresenceKey::of(&later, 1_002).differs_from(&sent));
    }

    #[test]
    fn nothing_sent_yet_always_sends_and_a_repeated_clear_does_not() {
        let key = Some(PresenceKey::of(&presence(), 1_000));
        assert!(presence_changed(None, &None));
        assert!(presence_changed(None, &key));
        assert!(!presence_changed(Some(&None), &None));
        assert!(presence_changed(Some(&None), &key));
        assert!(presence_changed(Some(&key), &None));
        assert!(!presence_changed(Some(&key), &key));
    }

    #[test]
    fn pause_and_track_change_are_changes() {
        let p = presence();
        let sent = PresenceKey::of(&p, 1_000);
        let mut paused = p.clone();
        paused.playing = false;
        assert!(PresenceKey::of(&paused, 1_000).differs_from(&sent));
        let mut next = p;
        next.title = "Other".into();
        assert!(PresenceKey::of(&next, 1_000).differs_from(&sent));
    }
}
