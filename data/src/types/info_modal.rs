//! Info Modal Data Types
//!
//! Types for the "Get Info" modal. `InfoModalItem` stores owned data for
//! each item type and provides a uniform `properties()` method that returns
//! label/value pairs for the two-column table.

use crate::utils::formatters::{
    bool_icon, format_duration_hms, format_file_size, format_rating, format_relative_time,
};

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
                rows.push(("Duration".into(), format_duration_hms(*duration)));
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
                    duration.map(|d| format_duration_hms(d as u32)),
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
                rows.push(("Duration".into(), format_duration_hms(*duration as u32)));
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

    /// Construct an `Album` info modal item from an `AlbumUIViewData`.
    /// `representative_path` is caller-supplied (derived from the album's
    /// expanded song children) because it is not present on the projection;
    /// pass `Some(path)` to enable the modal's "Show in Folder" button.
    /// DRY helper used by the albums/artists/genres views and the Get-Info hotkey.
    pub fn from_album_view_data(
        album: &crate::backend::albums::AlbumUIViewData,
        representative_path: Option<String>,
    ) -> Self {
        Self::Album {
            name: album.name.clone(),
            album_artist: Some(album.artist.clone()),
            release_type: album.release_type.clone(),
            genre: album.genre.clone(),
            genres: album.genres.clone(),
            duration: album.duration,
            year: album.year,
            song_count: Some(album.song_count),
            compilation: album.compilation,
            size: album.size,
            is_starred: album.is_starred,
            rating: album.rating,
            play_count: album.play_count,
            play_date: album.play_date.clone(),
            updated_at: album.updated_at.clone(),
            created_at: album.created_at.clone(),
            mbz_album_id: album.mbz_album_id.clone(),
            comment: album.comment.clone(),
            id: album.id.clone(),
            tags: album.tags.clone(),
            participants: album.participants.clone(),
            representative_path,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::albums::AlbumUIViewData;

    /// Build a minimal `AlbumUIViewData` from album JSON for the threading test.
    fn view_from_json(value: serde_json::Value) -> AlbumUIViewData {
        let album = serde_json::from_value(value).expect("valid album json");
        AlbumUIViewData::from_album(&album, "http://srv", "u=x")
    }

    /// `representative_path` is caller-supplied and threads straight through to
    /// the Album variant, where it gates the modal's "Show in Folder" button via
    /// `folder_path()`. This pins the exact divergence the audit flagged:
    /// `Some(path)` enables the button, `None` disables it.
    #[test]
    fn from_album_view_data_threads_representative_path() {
        let view = view_from_json(serde_json::json!({
            "id": "al-1",
            "name": "Test Album",
        }));

        // Some(path) → representative_path threaded + folder_path() resolves.
        let item =
            InfoModalItem::from_album_view_data(&view, Some("/music/al-1/01.flac".to_string()));
        match &item {
            InfoModalItem::Album {
                representative_path,
                ..
            } => assert_eq!(
                representative_path.as_deref(),
                Some("/music/al-1/01.flac"),
                "representative_path must thread through unchanged"
            ),
            other => panic!("expected Album variant, got {other:?}"),
        }
        assert_eq!(
            item.folder_path().as_deref(),
            Some("/music/al-1"),
            "folder_path() must enable the 'Show in Folder' button when a path is supplied"
        );

        // None → representative_path stays None + folder_path() returns None.
        let item_none = InfoModalItem::from_album_view_data(&view, None);
        match &item_none {
            InfoModalItem::Album {
                representative_path,
                ..
            } => assert!(
                representative_path.is_none(),
                "representative_path must stay None when caller passes None"
            ),
            other => panic!("expected Album variant, got {other:?}"),
        }
        assert_eq!(
            item_none.folder_path(),
            None,
            "folder_path() must be None (no 'Show in Folder' button) when no path is supplied"
        );
    }

    /// Guards the 21-field mapping against future drift inside the single source
    /// of truth. Builds an `AlbumUIViewData` with distinctive non-default values
    /// and asserts every field lands in the right place, with `album_artist` and
    /// `song_count` correctly `Some`-wrapped.
    #[test]
    fn from_album_view_data_maps_all_fields() {
        let view = AlbumUIViewData {
            id: "al-7".to_string(),
            name: "Distinctive Name".to_string(),
            artist: "The Artist".to_string(),
            artist_id: "ar-7".to_string(),
            song_count: 7,
            artwork_url: "http://srv/art".to_string(),
            year: Some(1999),
            genre: Some("Rock".to_string()),
            genres: Some("Rock • Pop".to_string()),
            duration: Some(2400.0),
            is_starred: true,
            play_count: Some(42),
            created_at: Some("2026-01-01".to_string()),
            play_date: Some("2026-02-02".to_string()),
            rating: Some(4),
            compilation: Some(true),
            size: Some(123_456),
            updated_at: Some("2026-03-03".to_string()),
            mbz_album_id: Some("mbz-7".to_string()),
            release_type: Some("EP".to_string()),
            comment: Some("a comment".to_string()),
            tags: vec![("Media".to_string(), "CD".to_string())],
            participants: vec![("Producer".to_string(), "Someone".to_string())],
            release_date: Some("1999-05-05".to_string()),
            original_date: Some("1999-01-01".to_string()),
            original_year: Some(1999),
            searchable_lower: "distinctive name the artist".to_string(),
        };

        let item = InfoModalItem::from_album_view_data(&view, None);
        let InfoModalItem::Album {
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
            representative_path,
        } = item
        else {
            panic!("expected Album variant");
        };

        assert_eq!(name, "Distinctive Name");
        assert_eq!(album_artist.as_deref(), Some("The Artist"));
        assert_eq!(release_type.as_deref(), Some("EP"));
        assert_eq!(genre.as_deref(), Some("Rock"));
        assert_eq!(genres.as_deref(), Some("Rock • Pop"));
        assert_eq!(duration, Some(2400.0));
        assert_eq!(year, Some(1999));
        assert_eq!(song_count, Some(7));
        assert_eq!(compilation, Some(true));
        assert_eq!(size, Some(123_456));
        assert!(is_starred);
        assert_eq!(rating, Some(4));
        assert_eq!(play_count, Some(42));
        assert_eq!(play_date.as_deref(), Some("2026-02-02"));
        assert_eq!(updated_at.as_deref(), Some("2026-03-03"));
        assert_eq!(created_at.as_deref(), Some("2026-01-01"));
        assert_eq!(mbz_album_id.as_deref(), Some("mbz-7"));
        assert_eq!(comment.as_deref(), Some("a comment"));
        assert_eq!(id, "al-7");
        assert_eq!(tags, vec![("Media".to_string(), "CD".to_string())]);
        assert_eq!(
            participants,
            vec![("Producer".to_string(), "Someone".to_string())]
        );
        assert_eq!(representative_path, None);
    }
}
