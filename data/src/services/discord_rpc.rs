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
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tracing::{debug, trace, warn};

/// nokkvi's own Discord application, so the status reads "Listening to
/// nokkvi" with its icon and nobody has to register an app. An application
/// id is public by design: it names the app, it grants nothing.
pub const NOKKVI_APP_ID: &str = "1573133205307924603";

/// The application presence is published under: the user's override when
/// set, else [`NOKKVI_APP_ID`].
pub fn effective_app_id(override_id: &str) -> &str {
    match override_id.trim() {
        "" => NOKKVI_APP_ID,
        id => id,
    }
}

/// `status_display_type`: which activity field the member list shows after
/// "Listening to".
const STATUS_DISPLAY_NAME: u8 = 0;
const STATUS_DISPLAY_STATE: u8 = 1;

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

/// Name suffixes that resolve only inside a home network or a tailnet.
const PRIVATE_NAME_SUFFIXES: [&str; 6] = [
    ".local",
    ".localhost",
    ".lan",
    ".home.arpa",
    ".internal",
    ".ts.net",
];

fn ipv4_is_public(ip: std::net::Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        // Carrier-grade NAT / Tailscale (100.64.0.0/10).
        || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1])))
}

/// Whether Discord's servers can reach `host`. Navidrome builds its share
/// links from the host the client used, so a LAN or tailnet address lands
/// here and would show viewers a broken image instead of the app's icon.
fn host_is_public(host: url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            // A single-label name (`localhost`, a Docker service, a bare
            // tailnet machine name) only resolves locally.
            name.contains('.') && !PRIVATE_NAME_SUFFIXES.iter().any(|s| name.ends_with(s))
        }
        url::Host::Ipv4(ip) => ipv4_is_public(ip),
        url::Host::Ipv6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => ipv4_is_public(v4),
            None => {
                let first = ip.segments()[0];
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    // Unique local fc00::/7 (Tailscale's fd7a:115c:a1e0::/48 among them).
                    || (first & 0xfe00) == 0xfc00
                    // Link-local fe80::/10.
                    || (first & 0xffc0) == 0xfe80)
            }
        },
    }
}

/// `url` when Discord can fetch it and showing it leaks nothing: http(s), a
/// public host ([`host_is_public`]), and none of [`CREDENTIAL_QUERY_KEYS`] in
/// the query.
pub fn usable_cover_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let reachable = host_is_public(parsed.host()?);
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
    // The member list reads "Listening to <artist>" while a track plays. A
    // paused state line ("Paused · …") would read oddly there, so a pause, or
    // a track with no artist, falls back to the application name.
    let show_artist = presence.playing && !presence.artist.trim().is_empty();
    activity["status_display_type"] = json!(if show_artist {
        STATUS_DISPLAY_STATE
    } else {
        STATUS_DISPLAY_NAME
    });
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
    // A test build never reaches the runner's Discord: it would publish the
    // test's track on their real profile (code-standards "Tests run on empty
    // app dirs").
    if crate::utils::paths::TEST_DIRS {
        return Vec::new();
    }
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
                // Vesktop's Flatpak, whose arRPC stands in for Discord.
                d.join(".flatpak/dev.vencord.Vesktop/xdg-run"),
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
        #[cfg(test)]
        pub fn from_stream(stream: UnixStream) -> Self {
            Self { stream }
        }

        /// Open the first live `discord-ipc-N` socket and complete the handshake.
        /// A refusal is a `warn!` the first time (`refusal_reported` false) and
        /// a `debug!` on every retry after, so a wrong id logs once, not every
        /// [`RECONNECT_INTERVAL`].
        pub fn open(app_id: &str, refusal_reported: &mut bool) -> Option<Self> {
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
                            if *refusal_reported {
                                debug!("Discord RPC handshake refused again: {reply}");
                            } else {
                                warn!("Discord RPC handshake refused: {reply}");
                                *refusal_reported = true;
                            }
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

        /// Whether the connection is no longer usable: Discord hung up (it
        /// quit or restarted), or sent something unasked. Discord only writes
        /// in answer to a frame, and every answer is read at once, so an idle
        /// socket that would block on a read is open and in step; end-of-file,
        /// an error or stray bytes mean reconnect. (`UnixStream::peek` is
        /// still unstable, hence a non-blocking read.)
        pub fn is_closed(&mut self) -> bool {
            if self.stream.set_nonblocking(true).is_err() {
                return true;
            }
            let open = matches!(
                self.stream.read(&mut [0u8; 1]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
            );
            !open || self.stream.set_nonblocking(false).is_err()
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
        pub fn open(_app_id: &str, _refusal_reported: &mut bool) -> Option<Self> {
            None
        }

        pub fn write(&mut self, _opcode: u32, _payload: &Value) -> std::io::Result<()> {
            Ok(())
        }

        pub fn is_closed(&mut self) -> bool {
            false
        }

        pub fn read_reply(&mut self) -> std::io::Result<Value> {
            Ok(Value::Null)
        }
    }
}

use conn::Connection;

/// Handle to the worker thread; the thread ends when this is dropped.
pub struct DiscordRpc {
    tx: mpsc::Sender<(Option<Presence>, i64)>,
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

    /// Show `presence`, or clear the activity with `None`. `sampled_unix` is
    /// the unix second its position was read, so a frame the rate limit or a
    /// late Discord holds back still carries the right start time.
    pub fn set(&self, presence: Option<Presence>, sampled_unix: i64) {
        // A closed channel means the worker is gone; there is nothing to do about it.
        if self.tx.send((presence, sampled_unix)).is_err() {
            debug!("Discord RPC worker is gone; presence dropped");
        }
    }
}

/// What the worker last wanted on screen, and what Discord was last told.
struct Worker<'a> {
    app_id: &'a str,
    conn: Option<Connection>,
    wanted: Option<Presence>,
    /// Unix second `wanted`'s position was read.
    wanted_at: i64,
    /// `true` once `wanted` is what Discord shows.
    delivered: bool,
    nonce: u64,
    last_send: Option<Instant>,
    last_connect_try: Option<Instant>,
    /// The handshake refusal for this `app_id` has been logged at `warn!`.
    refusal_reported: bool,
    /// An error answer to an activity has been logged at `warn!`.
    error_reported: bool,
}

impl<'a> Worker<'a> {
    fn new(app_id: &'a str) -> Self {
        Self {
            app_id,
            conn: None,
            wanted: None,
            wanted_at: 0,
            delivered: true,
            nonce: 0,
            last_send: None,
            last_connect_try: None,
            refusal_reported: false,
            error_reported: false,
        }
    }

    /// The `SET_ACTIVITY` frame for `wanted`, timed from when it was sampled.
    fn payload(&self) -> Value {
        let activity = self
            .wanted
            .as_ref()
            .map(|p| activity_json(p, self.wanted_at));
        set_activity_payload(activity, std::process::id(), self.nonce)
    }

    /// Drop a connection Discord hung up on, and have the activity sent again
    /// once it is back: a restarted Discord shows nothing until told.
    fn notice_hangup(&mut self) {
        if self.conn.as_mut().is_some_and(Connection::is_closed) {
            debug!("Discord RPC connection closed; will reconnect");
            self.conn = None;
            if self.wanted.is_some() {
                self.delivered = false;
            }
        }
    }

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
            self.conn = Connection::open(self.app_id, &mut self.refusal_reported);
        }
        if self.conn.is_none() {
            return;
        }
        self.nonce += 1;
        let payload = self.payload();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let sent = conn
            .write(OP_FRAME, &payload)
            .and_then(|()| conn.read_reply());
        self.last_send = Some(Instant::now());
        match sent {
            Ok(reply) => {
                // Discord answers a refused activity with an ERROR event; it
                // shows nothing, and sending the same frame again won't help.
                if reply.get("evt").and_then(Value::as_str) == Some("ERROR") {
                    if self.error_reported {
                        debug!("Discord RPC refused the activity again: {reply}");
                    } else {
                        warn!("Discord RPC refused the activity: {reply}");
                        self.error_reported = true;
                    }
                }
                self.delivered = true;
            }
            Err(e) => {
                debug!("Discord RPC send failed, will reconnect: {e}");
                self.conn = None;
            }
        }
    }
}

fn run_worker(app_id: &str, rx: &mpsc::Receiver<(Option<Presence>, i64)>) {
    let mut worker = Worker::new(app_id);
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok((presence, sampled_unix)) => {
                worker.wanted = presence;
                worker.wanted_at = sampled_unix;
                worker.delivered = false;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        worker.notice_hangup();
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

    #[test]
    fn private_hosts_are_refused_and_public_ones_kept() {
        for url in [
            "http://[fd7a:115c:a1e0::1]:4533/share/img/t",
            "http://[fe80::1]/share/img/t",
            "http://[::ffff:192.168.1.5]/share/img/t",
            "http://navidrome:4533/share/img/t",
            "http://localhost/share/img/t",
            "http://music.lan/share/img/t",
            "http://music.home.arpa/share/img/t",
            "http://music.internal/share/img/t",
            "https://noatun.tail1234.ts.net/share/img/t",
            "http://100.64.78.27:4533/share/img/t",
        ] {
            assert_eq!(usable_cover_url(url), None, "{url} should be refused");
        }
        for url in [
            "https://music.example.com/share/img/t",
            "http://[2606:4700::1]/share/img/t",
            "https://music.example.com./share/img/t",
        ] {
            assert!(usable_cover_url(url).is_some(), "{url} should be kept");
        }
    }

    #[test]
    fn a_test_build_finds_no_discord() {
        assert!(socket_dirs().is_empty());
    }

    #[test]
    fn a_held_frame_keeps_the_start_from_when_it_was_sampled() {
        let mut worker = Worker::new("1");
        worker.wanted = Some(presence());
        worker.wanted_at = 1_000;
        let start = worker.payload()["args"]["activity"]["timestamps"]["start"].clone();
        // presence() is 50 s in, read at unix 1000.
        assert_eq!(start, (1_000 - 50) * 1_000);
    }

    #[cfg(unix)]
    #[test]
    fn a_hangup_is_noticed_and_the_activity_goes_out_again() {
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut worker = Worker::new("1");
        worker.conn = Some(Connection::from_stream(ours));
        worker.wanted = Some(presence());
        worker.notice_hangup();
        assert!(worker.conn.is_some(), "an open, quiet socket stays");
        assert!(worker.delivered);
        drop(theirs);
        worker.notice_hangup();
        assert!(worker.conn.is_none());
        assert!(
            !worker.delivered,
            "the activity must be sent to the new Discord"
        );
    }

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
    fn the_member_list_shows_the_artist_while_playing_and_the_app_otherwise() {
        let mut p = presence();
        assert_eq!(activity_json(&p, 1_000)["status_display_type"], 1);
        p.playing = false;
        assert_eq!(activity_json(&p, 1_000)["status_display_type"], 0);
        p.playing = true;
        p.artist.clear();
        assert_eq!(activity_json(&p, 1_000)["status_display_type"], 0);
    }

    #[test]
    fn an_empty_override_publishes_as_nokkvi() {
        assert_eq!(effective_app_id(""), NOKKVI_APP_ID);
        assert_eq!(effective_app_id("  "), NOKKVI_APP_ID);
        assert_eq!(effective_app_id(" 123 "), "123");
    }

    #[test]
    fn nokkvi_app_id_is_a_discord_snowflake() {
        // Discord ids are 17-20 digit snowflakes; anything else is refused at
        // the handshake and the status never shows.
        assert!(
            (17..=20).contains(&NOKKVI_APP_ID.len())
                && NOKKVI_APP_ID.bytes().all(|b| b.is_ascii_digit()),
            "NOKKVI_APP_ID is not an application id: {NOKKVI_APP_ID}"
        );
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
