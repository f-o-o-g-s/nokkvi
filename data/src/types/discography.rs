//! An artist's discography, sorted into sections by release type.
//!
//! Navidrome's `/api/album?artist_id=` answers with every album an artist is
//! on, as album artist or as a track artist, in one flat list. This module
//! reads each album's release types (the MusicBrainz vocabulary, as the
//! `releasetype` tag or `mbzAlbumType` carries it) and sorts the list into
//! the sections the Artists view shows under an expanded artist.

use std::collections::HashMap;

use crate::backend::albums::AlbumUIViewData;

/// One MusicBrainz release-group type: a primary type (`Album`, `Ep`,
/// `Single`, `Broadcast`, `Other`) or a secondary one (the rest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseType {
    Album,
    Ep,
    Single,
    Broadcast,
    Other,
    Compilation,
    Soundtrack,
    Spokenword,
    Interview,
    Audiobook,
    AudioDrama,
    Live,
    Remix,
    DjMix,
    Mixtape,
    Demo,
    FieldRecording,
}

impl ReleaseType {
    /// Read one tag value. Case and surrounding space don't matter; the
    /// obvious spellings of a type are accepted; anything outside the
    /// vocabulary (a tagger's `MP3`, `LP`, `12"`, `Reissue`) is `None`.
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_lowercase().as_str() {
            "album" => Self::Album,
            "ep" | "mini-album" | "mini album" => Self::Ep,
            "single" => Self::Single,
            "broadcast" => Self::Broadcast,
            "other" => Self::Other,
            "compilation" => Self::Compilation,
            "soundtrack" => Self::Soundtrack,
            "spokenword" | "spoken word" => Self::Spokenword,
            "interview" => Self::Interview,
            "audiobook" | "audio book" => Self::Audiobook,
            "audio drama" | "audiodrama" => Self::AudioDrama,
            "live" => Self::Live,
            "remix" => Self::Remix,
            "dj-mix" | "djmix" | "dj mix" => Self::DjMix,
            "mixtape/street" | "mixtape" => Self::Mixtape,
            "demo" => Self::Demo,
            "field recording" | "fieldrecording" => Self::FieldRecording,
            _ => return None,
        })
    }

    /// Whether this is one of MusicBrainz's primary types.
    pub fn is_primary(self) -> bool {
        match self {
            Self::Album | Self::Ep | Self::Single | Self::Broadcast | Self::Other => true,
            Self::Compilation
            | Self::Soundtrack
            | Self::Spokenword
            | Self::Interview
            | Self::Audiobook
            | Self::AudioDrama
            | Self::Live
            | Self::Remix
            | Self::DjMix
            | Self::Mixtape
            | Self::Demo
            | Self::FieldRecording => false,
        }
    }

    /// The type as a row tag reads it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Album => "Album",
            Self::Ep => "EP",
            Self::Single => "Single",
            Self::Broadcast => "Broadcast",
            Self::Other => "Other",
            Self::Compilation => "Compilation",
            Self::Soundtrack => "Soundtrack",
            Self::Spokenword => "Spoken Word",
            Self::Interview => "Interview",
            Self::Audiobook => "Audiobook",
            Self::AudioDrama => "Audio Drama",
            Self::Live => "Live",
            Self::Remix => "Remix",
            Self::DjMix => "DJ Mix",
            Self::Mixtape => "Mixtape",
            Self::Demo => "Demo",
            Self::FieldRecording => "Field Recording",
        }
    }
}

/// Characters that separate several types packed into one value
/// (`album; compilation`, `Album/Live`).
const TYPE_SEPARATORS: [char; 3] = [';', '/', ','];

/// Push every type `raw` names onto `out`, once each. `mixtape/street`
/// splits at its slash like any other value: `mixtape` alone is the type and
/// `street` drops out.
fn push_release_types(raw: &str, out: &mut Vec<ReleaseType>) {
    for parsed in raw.split(TYPE_SEPARATORS).filter_map(ReleaseType::parse) {
        if !out.contains(&parsed) {
            out.push(parsed);
        }
    }
}

/// The values of tag `key` (keys compare without case), empty when absent.
fn tag_values<'a>(tags: Option<&'a HashMap<String, Vec<String>>>, key: &str) -> &'a [String] {
    tags.and_then(|map| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, values)| values.as_slice())
    })
    .unwrap_or_default()
}

/// The album's release types: the `releasetype` tag's values, or
/// `mbzAlbumType` when the tag names none. Junk values drop out, so an album
/// tagged only `FLAC` has no types at all.
pub fn release_types(
    tags: Option<&HashMap<String, Vec<String>>>,
    mbz_album_type: Option<&str>,
) -> Vec<ReleaseType> {
    let mut types = Vec::new();
    for value in tag_values(tags, "releasetype") {
        push_release_types(value, &mut types);
    }
    if types.is_empty()
        && let Some(raw) = mbz_album_type
    {
        push_release_types(raw, &mut types);
    }
    types
}

/// The first non-blank value of tag `key`, trimmed.
pub fn first_tag_value(tags: Option<&HashMap<String, Vec<String>>>, key: &str) -> Option<String> {
    tag_values(tags, key)
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Every non-blank value of tag `key`, trimmed and joined with a bullet.
pub fn joined_tag_values(tags: Option<&HashMap<String, Vec<String>>>, key: &str) -> Option<String> {
    let joined = tag_values(tags, key)
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{2022} ");
    (!joined.is_empty()).then_some(joined)
}

/// Whether the `explicitstatus` tag marks the album explicit (`e`, as
/// Navidrome stores it, or the word).
pub fn is_explicit(tags: Option<&HashMap<String, Vec<String>>>) -> bool {
    tag_values(tags, "explicitstatus").iter().any(|value| {
        let value = value.trim();
        value.eq_ignore_ascii_case("e") || value.eq_ignore_ascii_case("explicit")
    })
}

/// A section of an artist's discography. Declaration order is display order
/// ([`Self::ALL`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiscographyGroup {
    Albums,
    Eps,
    Singles,
    Live,
    Compilations,
    Soundtracks,
    Remixes,
    DjMixes,
    Mixtapes,
    Demos,
    Other,
    AppearsOn,
}

impl DiscographyGroup {
    /// Every section, in the order the view lists them.
    pub const ALL: [Self; 12] = [
        Self::Albums,
        Self::Eps,
        Self::Singles,
        Self::Live,
        Self::Compilations,
        Self::Soundtracks,
        Self::Remixes,
        Self::DjMixes,
        Self::Mixtapes,
        Self::Demos,
        Self::Other,
        Self::AppearsOn,
    ];

    /// The section header's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Albums => "Albums",
            Self::Eps => "EPs",
            Self::Singles => "Singles",
            Self::Live => "Live",
            Self::Compilations => "Compilations",
            Self::Soundtracks => "Soundtracks",
            Self::Remixes => "Remixes",
            Self::DjMixes => "DJ Mixes",
            Self::Mixtapes => "Mixtapes",
            Self::Demos => "Demos",
            Self::Other => "Other",
            Self::AppearsOn => "Appears On",
        }
    }

    /// What one row of the section is called: full-lengths are albums, the
    /// rest (and Appears On, which mixes types) are releases.
    fn noun(self, count: usize) -> &'static str {
        let albums = match self {
            Self::Albums | Self::Live | Self::Compilations | Self::Soundtracks => true,
            Self::Eps
            | Self::Singles
            | Self::Remixes
            | Self::DjMixes
            | Self::Mixtapes
            | Self::Demos
            | Self::Other
            | Self::AppearsOn => false,
        };
        match (albums, count == 1) {
            (true, true) => "album",
            (true, false) => "albums",
            (false, true) => "release",
            (false, false) => "releases",
        }
    }

    /// The header's count, e.g. `7 albums`, `1 album`, `3 releases`.
    pub fn count_label(self, count: usize) -> String {
        format!("{count} {}", self.noun(count))
    }
}

/// The section an album's own types put it in (whose album it is plays no
/// part; see [`classify`]).
///
/// An EP or Single is that outright. Otherwise the first of live,
/// compilation, soundtrack, remix, DJ mix, mixtape, demo decides; any other
/// secondary type, or a broadcast / other primary, is Other. An untyped album
/// the server flags as a compilation is a compilation, and everything else
/// is an album.
pub fn release_group(types: &[ReleaseType], compilation: bool) -> DiscographyGroup {
    use DiscographyGroup as Group;
    use ReleaseType as Type;

    let has = |wanted: Type| types.contains(&wanted);
    if has(Type::Ep) {
        return Group::Eps;
    }
    if has(Type::Single) {
        return Group::Singles;
    }
    const BY_PRIORITY: [(ReleaseType, DiscographyGroup); 7] = [
        (Type::Live, Group::Live),
        (Type::Compilation, Group::Compilations),
        (Type::Soundtrack, Group::Soundtracks),
        (Type::Remix, Group::Remixes),
        (Type::DjMix, Group::DjMixes),
        (Type::Mixtape, Group::Mixtapes),
        (Type::Demo, Group::Demos),
    ];
    if let Some((_, group)) = BY_PRIORITY.iter().find(|(wanted, _)| has(*wanted)) {
        return *group;
    }
    if types.iter().any(|t| *t != Type::Album) {
        // Spoken word, interview, audiobook, audio drama, field recording,
        // broadcast, other: real types with no section of their own.
        return Group::Other;
    }
    if types.is_empty() && compilation {
        return Group::Compilations;
    }
    Group::Albums
}

/// Whether `artist_id` is one of the album's album artists. An album that
/// names no album artist at all counts as the artist's own.
pub fn is_own_album(album: &AlbumUIViewData, artist_id: &str) -> bool {
    album.album_artist_ids.is_empty() || album.album_artist_ids.iter().any(|id| id == artist_id)
}

/// The section `album` goes in under artist `artist_id`: Appears On when the
/// artist is only a guest on it, else the section its types name.
pub fn classify(album: &AlbumUIViewData, artist_id: &str) -> DiscographyGroup {
    if is_own_album(album, artist_id) {
        release_group(&album.release_types, album.compilation == Some(true))
    } else {
        DiscographyGroup::AppearsOn
    }
}

/// Sort an artist's albums into sections, in display order. A section with
/// no album is left out, and inside a section the albums keep the order they
/// came in (the server's, newest first).
pub fn group_albums(
    albums: Vec<AlbumUIViewData>,
    artist_id: &str,
) -> Vec<(DiscographyGroup, Vec<AlbumUIViewData>)> {
    let mut sections: Vec<(DiscographyGroup, Vec<AlbumUIViewData>)> = DiscographyGroup::ALL
        .iter()
        .map(|group| (*group, Vec::new()))
        .collect();
    for album in albums {
        let group = classify(&album, artist_id);
        if let Some((_, members)) = sections.iter_mut().find(|(g, _)| *g == group) {
            members.push(album);
        }
    }
    sections.retain(|(_, members)| !members.is_empty());
    sections
}

/// The earliest and latest known year among `albums`; `None` when no album
/// carries one (the server sends 0 for an undated album).
pub fn year_span<'a>(albums: impl IntoIterator<Item = &'a AlbumUIViewData>) -> Option<(u32, u32)> {
    albums
        .into_iter()
        .filter_map(|album| album.year.filter(|year| *year > 0))
        .fold(None, |span, year| match span {
            None => Some((year, year)),
            Some((first, last)) => Some((first.min(year), last.max(year))),
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ARTIST: &str = "ar-1";

    /// An album as the artist's own, with `extra` merged over the base JSON.
    fn album(id: &str, extra: serde_json::Value) -> AlbumUIViewData {
        let mut value = json!({
            "id": id,
            "name": id,
            "albumArtistId": ARTIST,
            "participants": { "albumartist": [{ "id": ARTIST, "name": "Artist" }] },
        });
        if let (Some(base), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            for (key, v) in extra {
                base.insert(key.clone(), v.clone());
            }
        }
        let album = serde_json::from_value(value).expect("valid album json");
        AlbumUIViewData::from_album(&album, "http://srv", "u=x")
    }

    fn typed(id: &str, types: &[&str]) -> AlbumUIViewData {
        album(id, json!({ "tags": { "releasetype": types } }))
    }

    fn group_of(types: &[&str]) -> DiscographyGroup {
        classify(&typed("al", types), ARTIST)
    }

    #[test]
    fn every_section_has_its_bucket() {
        use DiscographyGroup as G;
        let cases: [(&[&str], G); 12] = [
            (&["album"], G::Albums),
            (&["ep"], G::Eps),
            (&["single"], G::Singles),
            (&["album", "live"], G::Live),
            (&["album", "compilation"], G::Compilations),
            (&["album", "soundtrack"], G::Soundtracks),
            (&["album", "remix"], G::Remixes),
            (&["album", "dj-mix"], G::DjMixes),
            (&["album", "mixtape/street"], G::Mixtapes),
            (&["album", "demo"], G::Demos),
            (&["spokenword"], G::Other),
            (&["broadcast"], G::Other),
        ];
        for (types, want) in cases {
            assert_eq!(group_of(types), want, "{types:?}");
        }
    }

    #[test]
    fn an_ep_or_single_wins_over_its_secondary_types() {
        assert_eq!(group_of(&["ep", "live"]), DiscographyGroup::Eps);
        assert_eq!(group_of(&["single", "remix"]), DiscographyGroup::Singles);
        assert_eq!(group_of(&["live", "ep"]), DiscographyGroup::Eps);
    }

    #[test]
    fn secondary_types_follow_the_priority_order() {
        use DiscographyGroup as G;
        assert_eq!(
            group_of(&["album", "compilation", "dj-mix"]),
            G::Compilations
        );
        assert_eq!(group_of(&["album", "compilation", "live"]), G::Live);
        assert_eq!(group_of(&["album", "demo", "remix"]), G::Remixes);
        assert_eq!(
            group_of(&["album", "soundtrack", "compilation"]),
            G::Compilations
        );
    }

    #[test]
    fn the_other_secondary_types_and_primaries_go_to_other() {
        use DiscographyGroup as G;
        for types in [
            &["album", "spokenword"][..],
            &["album", "interview"],
            &["album", "audiobook"],
            &["album", "audio drama"],
            &["album", "field recording"],
            &["other"],
            &["broadcast"],
        ] {
            assert_eq!(group_of(types), G::Other, "{types:?}");
        }
        // A sectioned secondary still wins over them.
        assert_eq!(group_of(&["other", "live"]), G::Live);
    }

    #[test]
    fn junk_values_are_dropped() {
        for junk in [
            "MP3", "FLAC", "WAV", "ALAC", "AAC", "LP", "12\"", "7\"", "Reissue", "Stereo",
        ] {
            assert_eq!(ReleaseType::parse(junk), None, "{junk}");
            assert_eq!(group_of(&[junk]), DiscographyGroup::Albums, "{junk}");
        }
        // Junk beside a real type leaves the real type standing.
        assert_eq!(group_of(&["FLAC", "ep"]), DiscographyGroup::Eps);
        assert_eq!(typed("al", &["LP", "Reissue"]).release_types, Vec::new());
    }

    #[test]
    fn case_space_and_spelling_are_normalized() {
        use ReleaseType as T;
        assert_eq!(
            group_of(&["Album", "Compilation"]),
            DiscographyGroup::Compilations
        );
        assert_eq!(group_of(&[" EP "]), DiscographyGroup::Eps);
        for (raw, want) in [
            ("Mini-Album", T::Ep),
            ("djmix", T::DjMix),
            ("DJ Mix", T::DjMix),
            ("Mixtape", T::Mixtape),
            ("Mixtape/Street", T::Mixtape),
            ("Spoken Word", T::Spokenword),
        ] {
            assert_eq!(ReleaseType::parse(raw), Some(want), "{raw}");
        }
    }

    #[test]
    fn several_types_in_one_value_are_split() {
        use ReleaseType as T;
        assert_eq!(
            typed("al", &["album; compilation"]).release_types,
            vec![T::Album, T::Compilation]
        );
        assert_eq!(
            typed("al", &["Album/Live", "live"]).release_types,
            vec![T::Album, T::Live]
        );
        assert_eq!(
            typed("al", &["mixtape/street"]).release_types,
            vec![T::Mixtape]
        );
        assert_eq!(
            typed("al", &["Album; Mixtape/Street"]).release_types,
            vec![T::Album, T::Mixtape]
        );
    }

    #[test]
    fn missing_tags_fall_back_to_mbz_album_type() {
        use DiscographyGroup as G;
        let by_mbz = |raw: &str| classify(&album("al", json!({ "mbzAlbumType": raw })), ARTIST);
        assert_eq!(by_mbz("EP"), G::Eps);
        assert_eq!(by_mbz("album; soundtrack"), G::Soundtracks);
        assert_eq!(by_mbz("Album/Live"), G::Live);
        assert_eq!(by_mbz("single,remix"), G::Singles);
        assert_eq!(by_mbz("FLAC"), G::Albums);
        // The tag wins when it names a type...
        let both = album(
            "al",
            json!({ "mbzAlbumType": "single", "tags": { "releasetype": ["ep"] } }),
        );
        assert_eq!(classify(&both, ARTIST), G::Eps);
        // ...and mbzAlbumType answers when the tag holds only junk.
        let junk_tag = album(
            "al",
            json!({ "mbzAlbumType": "single", "tags": { "releasetype": ["FLAC"] } }),
        );
        assert_eq!(classify(&junk_tag, ARTIST), G::Singles);
    }

    #[test]
    fn an_album_with_no_type_is_an_album_unless_flagged_a_compilation() {
        use DiscographyGroup as G;
        assert_eq!(classify(&album("al", json!({})), ARTIST), G::Albums);
        assert_eq!(
            classify(&album("al", json!({ "compilation": true })), ARTIST),
            G::Compilations
        );
        assert_eq!(
            classify(&album("al", json!({ "compilation": false })), ARTIST),
            G::Albums
        );
        // A usable type outranks the flag.
        let typed_flagged = album(
            "al",
            json!({ "compilation": true, "tags": { "releasetype": ["ep"] } }),
        );
        assert_eq!(classify(&typed_flagged, ARTIST), G::Eps);
    }

    #[test]
    fn a_guest_album_appears_on_whatever_its_type() {
        use DiscographyGroup as G;
        let guest = album(
            "al",
            json!({
                "albumArtistId": "ar-2",
                "participants": {
                    "albumartist": [{ "id": "ar-2", "name": "Someone Else" }],
                    "artist": [{ "id": ARTIST, "name": "Artist" }],
                },
                "tags": { "releasetype": ["ep"] },
            }),
        );
        assert_eq!(classify(&guest, ARTIST), G::AppearsOn);
        assert_eq!(classify(&guest, "ar-2"), G::Eps);
    }

    #[test]
    fn one_of_several_album_artists_owns_the_album() {
        let split = album(
            "al",
            json!({
                "albumArtistId": "ar-2",
                "participants": { "albumartist": [
                    { "id": "ar-2", "name": "Someone Else" },
                    { "id": ARTIST, "name": "Artist" },
                ] },
            }),
        );
        assert_eq!(classify(&split, ARTIST), DiscographyGroup::Albums);
        assert_eq!(classify(&split, "ar-3"), DiscographyGroup::AppearsOn);
    }

    #[test]
    fn without_album_artist_participants_the_album_artist_id_decides() {
        use DiscographyGroup as G;
        let by_id = |id: Option<&str>| {
            let mut value = json!({ "id": "al", "name": "al", "artistId": "ar-9" });
            if let Some(id) = id {
                value["albumArtistId"] = json!(id);
            }
            let album = serde_json::from_value(value).expect("valid album json");
            AlbumUIViewData::from_album(&album, "http://srv", "u=x")
        };
        assert_eq!(classify(&by_id(Some(ARTIST)), ARTIST), G::Albums);
        assert_eq!(classify(&by_id(Some("ar-2")), ARTIST), G::AppearsOn);
        // Neither participants nor an album artist id: the artist's own.
        assert_eq!(classify(&by_id(None), ARTIST), G::Albums);
        assert_eq!(classify(&by_id(Some("")), ARTIST), G::Albums);
    }

    #[test]
    fn grouping_orders_sections_and_keeps_the_server_order_inside() {
        use DiscographyGroup as G;
        let guest = album(
            "guest",
            json!({ "participants": { "albumartist": [{ "id": "ar-2", "name": "X" }] } }),
        );
        let albums = vec![
            typed("single-new", &["single"]),
            guest,
            typed("lp-new", &["album"]),
            typed("live", &["album", "live"]),
            typed("lp-old", &["album"]),
            typed("single-old", &["single"]),
        ];
        let grouped: Vec<(G, Vec<String>)> = group_albums(albums, ARTIST)
            .into_iter()
            .map(|(group, members)| (group, members.into_iter().map(|a| a.id).collect()))
            .collect();
        assert_eq!(
            grouped,
            vec![
                (G::Albums, vec!["lp-new".to_string(), "lp-old".to_string()]),
                (
                    G::Singles,
                    vec!["single-new".to_string(), "single-old".to_string()]
                ),
                (G::Live, vec!["live".to_string()]),
                (G::AppearsOn, vec!["guest".to_string()]),
            ]
        );
        assert!(group_albums(Vec::new(), ARTIST).is_empty());
    }

    #[test]
    fn all_lists_every_section_once_in_display_order() {
        let labels: Vec<&str> = DiscographyGroup::ALL.iter().map(|g| g.label()).collect();
        assert_eq!(
            labels,
            [
                "Albums",
                "EPs",
                "Singles",
                "Live",
                "Compilations",
                "Soundtracks",
                "Remixes",
                "DJ Mixes",
                "Mixtapes",
                "Demos",
                "Other",
                "Appears On"
            ]
        );
    }

    #[test]
    fn count_labels_name_albums_or_releases() {
        use DiscographyGroup as G;
        assert_eq!(G::Albums.count_label(7), "7 albums");
        assert_eq!(G::Albums.count_label(1), "1 album");
        assert_eq!(G::Live.count_label(2), "2 albums");
        assert_eq!(G::Eps.count_label(3), "3 releases");
        assert_eq!(G::Singles.count_label(1), "1 release");
        assert_eq!(G::AppearsOn.count_label(4), "4 releases");
    }

    #[test]
    fn year_span_skips_undated_albums() {
        let dated = |year: u32| album("al", json!({ "maxYear": year }));
        assert_eq!(
            year_span(&[dated(2016), dated(2003), dated(2009)]),
            Some((2003, 2016))
        );
        assert_eq!(year_span(&[dated(2011)]), Some((2011, 2011)));
        assert_eq!(year_span(&[dated(0), dated(1999)]), Some((1999, 1999)));
        assert_eq!(year_span(&[dated(0), album("al", json!({}))]), None);
        assert_eq!(year_span(&[]), None);
    }

    #[test]
    fn version_label_and_explicit_are_read_from_the_tags() {
        let tagged = album(
            "al",
            json!({ "tags": {
                "albumversion": ["  Deluxe Edition ", "Remastered"],
                "recordlabel": ["Warp", "Sire"],
                "explicitstatus": ["e"],
            } }),
        );
        assert_eq!(tagged.version.as_deref(), Some("Deluxe Edition"));
        assert_eq!(tagged.record_label.as_deref(), Some("Warp \u{2022} Sire"));
        assert!(tagged.explicit);

        let clean = album(
            "al",
            json!({ "tags": { "explicitstatus": ["c"], "albumversion": [" "] } }),
        );
        assert!(!clean.explicit);
        assert_eq!(clean.version, None);
        assert_eq!(clean.record_label, None);
        assert!(album("al", json!({ "tags": { "explicitstatus": ["Explicit"] } })).explicit);
        assert!(!album("al", json!({})).explicit);
    }
}
