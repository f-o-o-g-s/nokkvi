//! Info Modal Data Types
//!
//! Types for the "Get Info" modal. `InfoModalItem` stores owned data for
//! each item type and provides a uniform `properties()` method that returns
//! label/value pairs for the two-column table.

/// Items that can be shown in the info modal.
/// Stores owned, cloned data so the modal state is self-contained.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // Always boxed at usage sites (InfoModalMessage::Open, *Action::ShowInfo)
pub enum InfoModalItem {
    Song {
        title: String,
        artist: String,
        album_artist: Option<String>,
        album: String,
        disc: Option<u32>,
        track: Option<u32>,
        year: Option<u32>,
        genre: Option<String>,
        duration: u32,
        compilation: Option<bool>,
        suffix: Option<String>,
        bitrate: Option<u32>,
        sample_rate: Option<u32>,
        bit_depth: Option<u32>,
        channels: Option<u32>,
        size: u64,
        bpm: Option<u32>,
        is_starred: bool,
        rating: Option<u32>,
        play_count: Option<u32>,
        play_date: Option<String>,
        updated_at: Option<String>,
        created_at: Option<String>,
        replay_gain_album: Option<f64>,
        replay_gain_track: Option<f64>,
        replay_peak_album: Option<f64>,
        replay_peak_track: Option<f64>,
        comment: Option<String>,
        path: String,
        id: String,
        tags: Vec<(String, String)>,
        participants: Vec<(String, String)>,
    },
    Album {
        name: String,
        album_artist: Option<String>,
        release_type: Option<String>,
        genre: Option<String>,
        genres: Option<String>,
        duration: Option<f64>,
        year: Option<u32>,
        song_count: Option<u32>,
        compilation: Option<bool>,
        size: Option<u64>,
        is_starred: bool,
        rating: Option<u32>,
        play_count: Option<u32>,
        play_date: Option<String>,
        updated_at: Option<String>,
        created_at: Option<String>,
        mbz_album_id: Option<String>,
        comment: Option<String>,
        id: String,
        tags: Vec<(String, String)>,
        participants: Vec<(String, String)>,
        /// Path of one representative song in this album, used to locate the folder locally.
        representative_path: Option<String>,
    },
    Artist {
        name: String,
        song_count: Option<u32>,
        album_count: Option<u32>,
        is_starred: bool,
        rating: Option<u32>,
        play_count: Option<u32>,
        play_date: Option<String>,
        size: Option<u64>,
        mbz_artist_id: Option<String>,
        biography: Option<String>,
        external_url: Option<String>,
        id: String,
    },
    Playlist {
        name: String,
        comment: String,
        duration: f32,
        song_count: u32,
        size: i64,
        owner_name: String,
        public: bool,
        created_at: String,
        updated_at: String,
        id: String,
    },
}

impl InfoModalItem {
    /// Title shown in the modal header.
    pub fn title(&self) -> &str {
        match self {
            Self::Song { title, .. } => title,
            Self::Album { name, .. } => name,
            Self::Artist { name, .. } => name,
            Self::Playlist { name, .. } => name,
        }
    }

    /// Returns the local filesystem directory to open in the file manager, if applicable.
    /// For `Song`, this is the parent directory of the song's file path.
    /// For `Album`, this is derived from the `representative_path` field.
    /// Returns `None` for Artist and Playlist (no file path available).
    pub fn folder_path(&self) -> Option<String> {
        let raw_path = match self {
            Self::Song { path, .. } => path.as_str(),
            Self::Album {
                representative_path,
                ..
            } => representative_path.as_deref()?,
            _ => return None,
        };
        std::path::Path::new(raw_path)
            .parent()
            .and_then(|p| p.to_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    }

    /// Property rows as (label, value) pairs.
    /// Empty/None values are filtered out automatically.
    pub fn properties(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();

        match self {
            Self::Song {
                title,
                artist,
                album_artist,
                album,
                disc,
                track,
                year,
                genre,
                duration,
                compilation,
                suffix,
                bitrate,
                sample_rate,
                bit_depth,
                channels,
                size,
                bpm,
                is_starred,
                rating,
                play_count,
                play_date,
                updated_at,
                created_at,
                replay_gain_album,
                replay_gain_track,
                replay_peak_album,
                replay_peak_track,
                comment,
                path,
                id,
                tags,
                participants,
            } => {
                rows.push(("Title".into(), title.clone()));
                push_str(&mut rows, "Path", path);
                push_opt_str(&mut rows, "Album Artist", album_artist);
                rows.push(("Artist".into(), artist.clone()));
                rows.push(("Album".into(), album.clone()));
                push_opt_u32(&mut rows, "Disc", disc);
                push_opt_u32(&mut rows, "Track", track);
                push_opt_u32(&mut rows, "Year", year);
                push_opt_str(&mut rows, "Genre", genre);
                rows.push(("Duration".into(), format_duration_secs(*duration)));
                push_opt(&mut rows, "Compilation", compilation.map(bool_icon));
                push_opt_str(&mut rows, "Codec", suffix);
                push_opt(&mut rows, "Bitrate", bitrate.map(|b| format!("{b} kbps")));
                push_opt(&mut rows, "Sample Rate", sample_rate.map(|r| r.to_string()));
                // Hide bit depth 0 (meaningless for lossy codecs like MP3)
                push_opt(
                    &mut rows,
                    "Bit Depth",
                    bit_depth.and_then(|d| if d > 0 { Some(d.to_string()) } else { None }),
                );
                push_opt(&mut rows, "Channels", channels.map(|c| c.to_string()));
                if *size > 0 {
                    rows.push(("Size".into(), format_file_size(*size)));
                }
                push_opt(&mut rows, "BPM", bpm.map(|b| b.to_string()));
                rows.push(("Favorite".into(), bool_icon(*is_starred)));
                push_opt(&mut rows, "Rating", rating.map(format_rating));
                push_opt(&mut rows, "Play Count", play_count.map(|c| c.to_string()));
                push_opt(
                    &mut rows,
                    "Last Played",
                    play_date.as_ref().map(|d| format_relative_time(d)),
                );
                push_opt(
                    &mut rows,
                    "Modified",
                    updated_at.as_ref().map(|d| format_relative_time(d)),
                );
                push_opt(
                    &mut rows,
                    "Added",
                    created_at.as_ref().map(|d| format_relative_time(d)),
                );
                push_opt(
                    &mut rows,
                    "Album Gain",
                    replay_gain_album.map(|g| format!("{g:.2} dB")),
                );
                push_opt(
                    &mut rows,
                    "Track Gain",
                    replay_gain_track.map(|g| format!("{g:.2} dB")),
                );
                push_opt(
                    &mut rows,
                    "Album Peak",
                    replay_peak_album.map(|p| format!("{p:.6}")),
                );
                push_opt(
                    &mut rows,
                    "Track Peak",
                    replay_peak_track.map(|p| format!("{p:.6}")),
                );
                push_opt_str(&mut rows, "Comment", comment);
                push_str(&mut rows, "ID", id);

                // Dynamic tags section
                if !tags.is_empty() {
                    // Separator header showing count
                    rows.push(("Tags".to_string(), tags.len().to_string()));
                    for (key, value) in tags {
                        rows.push((key.clone(), value.clone()));
                    }
                }

                // Participants section
                if !participants.is_empty() {
                    rows.push(("Participants".to_string(), participants.len().to_string()));
                    for (role, names) in participants {
                        rows.push((role.clone(), names.clone()));
                    }
                }
            }
            Self::Album {
                name,
                album_artist,
                release_type,
                genre,
                genres,
                duration,
                year,
                song_count,
                compilation,
                size,
                is_starred,
                rating,
                play_count,
                play_date,
                updated_at,
                created_at,
                mbz_album_id,
                comment,
                id,
                tags,
                participants,
                ..
            } => {
                rows.push(("Title".into(), name.clone()));
                push_opt_str(&mut rows, "Album Artist", album_artist);
                push_opt_str(&mut rows, "Release Type", release_type);
                // Prefer multi-genre string ("Black Metal • Heavy Metal") over single genre
                if let Some(g) = genres {
                    if !g.is_empty() {
                        rows.push(("Genres".into(), g.clone()));
                    }
                } else {
                    push_opt_str(&mut rows, "Genre", genre);
                }
                push_opt(
                    &mut rows,
                    "Duration",
                    duration.map(|d| format_duration_secs(d as u32)),
                );
                push_opt_u32(&mut rows, "Year", year);
                push_opt_u32(&mut rows, "Songs", song_count);
                push_opt(&mut rows, "Compilation", compilation.map(bool_icon));
                if let Some(s) = size
                    && *s > 0
                {
                    rows.push(("Size".into(), format_file_size(*s)));
                }
                rows.push(("Favorite".into(), bool_icon(*is_starred)));
                push_opt(&mut rows, "Rating", rating.map(format_rating));
                push_opt(&mut rows, "Play Count", play_count.map(|c| c.to_string()));
                push_opt(
                    &mut rows,
                    "Last Played",
                    play_date.as_ref().map(|d| format_relative_time(d)),
                );
                push_opt(
                    &mut rows,
                    "Modified",
                    updated_at.as_ref().map(|d| format_relative_time(d)),
                );
                push_opt(
                    &mut rows,
                    "Added",
                    created_at.as_ref().map(|d| format_relative_time(d)),
                );
                push_opt(
                    &mut rows,
                    "MusicBrainz",
                    mbz_album_id
                        .as_ref()
                        .filter(|s| !s.is_empty())
                        .map(|id| format!("https://musicbrainz.org/release/{id}")),
                );
                push_opt_str(&mut rows, "Comment", comment);
                push_str(&mut rows, "ID", id);

                // Dynamic tags section
                if !tags.is_empty() {
                    rows.push(("Tags".to_string(), tags.len().to_string()));
                    for (key, value) in tags {
                        rows.push((key.clone(), value.clone()));
                    }
                }

                // Participants section
                if !participants.is_empty() {
                    rows.push(("Participants".to_string(), participants.len().to_string()));
                    for (role, names) in participants {
                        rows.push((role.clone(), names.clone()));
                    }
                }
            }
            Self::Artist {
                name,
                song_count,
                album_count,
                is_starred,
                rating,
                play_count,
                play_date,
                size,
                mbz_artist_id,
                biography,
                external_url,
                id,
            } => {
                rows.push(("Name".into(), name.clone()));
                push_opt_u32(&mut rows, "Albums", album_count);
                push_opt_u32(&mut rows, "Songs", song_count);
                rows.push(("Favorite".into(), bool_icon(*is_starred)));
                push_opt(&mut rows, "Rating", rating.map(format_rating));
                push_opt(&mut rows, "Play Count", play_count.map(|c| c.to_string()));
                push_opt(
                    &mut rows,
                    "Last Played",
                    play_date.as_ref().map(|d| format_relative_time(d)),
                );
                push_opt(&mut rows, "Size", size.map(format_file_size));
                push_opt(
                    &mut rows,
                    "MusicBrainz",
                    mbz_artist_id
                        .as_ref()
                        .filter(|s| !s.is_empty())
                        .map(|id| format!("https://musicbrainz.org/artist/{id}")),
                );
                push_opt_str(&mut rows, "External URL", external_url);
                push_opt(
                    &mut rows,
                    "Biography",
                    biography.as_ref().map(|b| strip_html_tags(b)),
                );
                push_str(&mut rows, "ID", id);
            }
            Self::Playlist {
                name,
                comment,
                duration,
                song_count,
                size,
                owner_name,
                public,
                created_at,
                updated_at,
                id,
            } => {
                rows.push(("Title".into(), name.clone()));
                if !comment.is_empty() {
                    rows.push(("Description".into(), comment.clone()));
                }
                rows.push(("Duration".into(), format_duration_secs(*duration as u32)));
                rows.push(("Songs".into(), song_count.to_string()));
                if *size as u64 > 0 {
                    rows.push(("Size".into(), format_file_size(*size as u64)));
                }
                if !owner_name.is_empty() {
                    rows.push(("Owner".into(), owner_name.clone()));
                }
                rows.push(("Public".into(), bool_icon(*public)));
                if !created_at.is_empty() {
                    rows.push(("Created".into(), created_at.clone()));
                }
                if !updated_at.is_empty() {
                    rows.push(("Modified".into(), updated_at.clone()));
                }
                push_str(&mut rows, "ID", id);
            }
        }

        rows
    }
}

// =============================================================================
// Formatting Helpers
// =============================================================================

fn push_str(rows: &mut Vec<(String, String)>, label: &str, value: &str) {
    if !value.is_empty() {
        rows.push((label.to_string(), value.to_string()));
    }
}

fn push_opt_str(rows: &mut Vec<(String, String)>, label: &str, value: &Option<String>) {
    if let Some(v) = value
        && !v.is_empty()
    {
        rows.push((label.to_string(), v.clone()));
    }
}

fn push_opt_u32(rows: &mut Vec<(String, String)>, label: &str, value: &Option<u32>) {
    if let Some(v) = value {
        rows.push((label.to_string(), v.to_string()));
    }
}

fn push_opt(rows: &mut Vec<(String, String)>, label: &str, value: Option<String>) {
    if let Some(v) = value
        && !v.is_empty()
    {
        rows.push((label.to_string(), v));
    }
}

fn format_duration_secs(total_secs: u32) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GiB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KiB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Format an ISO 8601 timestamp as a relative time string ("3 months ago", "9 days ago").
/// Falls back to the raw string if parsing fails.
fn format_relative_time(timestamp: &str) -> String {
    use chrono::{DateTime, Datelike, FixedOffset, Utc};

    let parsed = timestamp
        .parse::<DateTime<FixedOffset>>()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| timestamp.parse::<DateTime<Utc>>());

    let Ok(dt) = parsed else {
        return timestamp.to_string();
    };

    let now = Utc::now();
    let duration = now.signed_duration_since(dt);

    if duration.num_seconds() < 0 {
        return "just now".to_string();
    }

    let seconds = duration.num_seconds();
    let minutes = duration.num_minutes();
    let hours = duration.num_hours();
    let days = duration.num_days();

    // Calendar-aware month/year calculation
    let months = {
        let y = (now.year() - dt.year()) * 12;
        let m = now.month() as i32 - dt.month() as i32;
        let total = y + m;
        // Adjust: if we haven't reached the same day-of-month yet, subtract 1
        if now.day() < dt.day() {
            (total - 1).max(0)
        } else {
            total.max(0)
        }
    };
    let years = months / 12;

    if seconds < 60 {
        "just now".to_string()
    } else if minutes < 60 {
        if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{minutes} minutes ago")
        }
    } else if hours < 24 {
        if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        }
    } else if days < 30 {
        if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{days} days ago")
        }
    } else if months < 12 {
        if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{months} months ago")
        }
    } else if years == 1 {
        "1 year ago".to_string()
    } else {
        format!("{years} years ago")
    }
}

fn format_rating(rating: u32) -> String {
    let filled = rating.min(5) as usize;
    let empty = 5 - filled;
    format!("{}{}", "★".repeat(filled), "☆".repeat(empty))
}

fn bool_icon(value: bool) -> String {
    if value { "✓" } else { "✗" }.to_string()
}

/// Strip HTML tags from text, converting `<a href="URL">text</a>` to `text (URL)`.
/// Handles the common Last.fm biography format.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Collect the tag content
            let mut tag = String::new();
            for tc in chars.by_ref() {
                if tc == '>' {
                    break;
                }
                tag.push(tc);
            }

            let tag_lower = tag.to_lowercase();

            if tag_lower.starts_with("a ") || tag_lower.starts_with("a\t") {
                // Extract href from <a ...href="URL"...>
                let href = extract_href(&tag);

                // Collect link text until </a>
                let mut link_text = String::new();
                while let Some(&nc) = chars.peek() {
                    if nc == '<' {
                        // Check for </a>
                        chars.next();
                        let mut end_tag = String::new();
                        for tc in chars.by_ref() {
                            if tc == '>' {
                                break;
                            }
                            end_tag.push(tc);
                        }
                        if end_tag.trim().eq_ignore_ascii_case("/a") {
                            break;
                        }
                        // Not </a>, keep the content
                        link_text.push('<');
                        link_text.push_str(&end_tag);
                        link_text.push('>');
                    } else {
                        link_text.push(nc);
                        chars.next();
                    }
                }

                let text = link_text.trim();
                if let Some(url) = href {
                    if text.is_empty() || text == url.as_str() {
                        result.push_str(&url);
                    } else {
                        result.push_str(text);
                        result.push_str(" (");
                        result.push_str(&url);
                        result.push(')');
                    }
                } else if !text.is_empty() {
                    result.push_str(text);
                }
            }
            // else: skip the tag entirely (strip it)
        } else {
            result.push(ch);
        }
    }

    result
}

/// Extract the href value from an anchor tag's attributes string.
fn extract_href(tag_attrs: &str) -> Option<String> {
    let lower = tag_attrs.to_lowercase();
    let href_pos = lower.find("href=")?;
    let after_href = &tag_attrs[href_pos + 5..];
    let trimmed = after_href.trim_start();

    if let Some(content) = trimmed.strip_prefix('"') {
        let end = content.find('"')?;
        Some(content[..end].to_string())
    } else if let Some(content) = trimmed.strip_prefix('\'') {
        let end = content.find('\'')?;
        Some(content[..end].to_string())
    } else {
        let end = trimmed
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(trimmed.len());
        Some(trimmed[..end].to_string())
    }
}

impl InfoModalItem {
    /// Construct a `Song` info modal item from a raw `Song` struct.
    /// DRY helper used by all view files and the queue handler.
    pub fn from_song(song: &crate::types::song::Song) -> Self {
        let rg = song.replay_gain.as_ref();
        Self::Song {
            title: song.title.clone(),
            artist: song.artist.clone(),
            album_artist: song.album_artist.clone(),
            album: song.album.clone(),
            disc: song.disc,
            track: song.track,
            year: song.year,
            genre: song.genre.clone(),
            duration: song.duration,
            compilation: song.compilation,
            suffix: song.suffix.clone(),
            bitrate: song.bitrate,
            sample_rate: song.sample_rate,
            bit_depth: song.bit_depth,
            channels: song.channels,
            size: song.size,
            bpm: song.bpm,
            is_starred: song.starred,
            rating: song.rating,
            play_count: song.play_count,
            play_date: song.play_date.clone(),
            updated_at: song.updated_at.clone(),
            created_at: song.created_at.clone(),
            replay_gain_album: rg.and_then(|r| r.album_gain),
            replay_gain_track: rg.and_then(|r| r.track_gain),
            replay_peak_album: rg.and_then(|r| r.album_peak),
            replay_peak_track: rg.and_then(|r| r.track_peak),
            comment: song.comment.clone(),
            path: song.path.clone(),
            id: song.id.clone(),
            tags: flatten_tags(song.tags.as_ref()),
            participants: crate::backend::flatten_participants(song.participants.as_ref()),
        }
    }

    /// Construct a `Song` info modal item from a `SongUIViewData`.
    /// DRY helper used by songs, albums, artists, and playlists views.
    pub fn from_song_view_data(song: &crate::backend::songs::SongUIViewData) -> Self {
        let rg = song.replay_gain.as_ref();
        Self::Song {
            title: song.title.clone(),
            artist: song.artist.clone(),
            album_artist: song.album_artist.clone(),
            album: song.album.clone(),
            disc: song.disc,
            track: song.track,
            year: song.year,
            genre: song.genre.clone(),
            duration: song.duration,
            compilation: song.compilation,
            suffix: song.suffix.clone(),
            bitrate: song.bitrate,
            sample_rate: song.sample_rate,
            bit_depth: song.bit_depth,
            channels: song.channels,
            size: song.size,
            bpm: song.bpm,
            is_starred: song.is_starred,
            rating: song.rating,
            play_count: song.play_count,
            play_date: song.play_date.clone(),
            updated_at: song.updated_at.clone(),
            created_at: song.created_at.clone(),
            replay_gain_album: rg.and_then(|r| r.album_gain),
            replay_gain_track: rg.and_then(|r| r.track_gain),
            replay_peak_album: rg.and_then(|r| r.album_peak),
            replay_peak_track: rg.and_then(|r| r.track_peak),
            comment: song.comment.clone(),
            path: song.path.clone(),
            id: song.id.clone(),
            tags: flatten_tags(song.tags.as_ref()),
            participants: song.participants.clone(),
        }
    }
}

/// Flatten HashMap tags into sorted (key, joined_values) pairs for display.
/// Filters out keys already shown as dedicated fields (genre, artist, etc.).
fn flatten_tags(
    tags: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> Vec<(String, String)> {
    let Some(map) = tags else {
        return Vec::new();
    };

    // Keys already displayed as dedicated fields — skip them
    const SKIP_KEYS: &[&str] = &[
        "artist",
        "albumartist",
        "album",
        "title",
        "tracknumber",
        "discnumber",
        "date",
        "comment",
        "recordlabel",
        "releasetype",
        "albumversion",
    ];

    let mut pairs: Vec<(String, String)> = map
        .iter()
        .filter(|(k, _)| !SKIP_KEYS.contains(&k.to_lowercase().as_str()))
        .map(|(k, v)| {
            // Title-case the key for display
            let label = titlecase(k);
            let value = v.join(" • ");
            (label, value)
        })
        .collect();

    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

/// Simple title-case: capitalize first letter of each word.
fn titlecase(s: &str) -> String {
    s.split('_')
        .flat_map(|word| word.split(' '))
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{upper}{}", chars.collect::<String>())
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
