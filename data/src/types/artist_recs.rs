//! Similar artists for the Artists view and the Similar Artists modal: who
//! listeners play alongside an artist, from ListenBrainz and Deezer, each
//! checked against the library.
//!
//! ListenBrainz's list (`services::listenbrainz_similar`) is keyed on the
//! artist's MusicBrainz id, Deezer's (`services::deezer_similar`) on the
//! artist's name, since Deezer has no MusicBrainz ids. Both name artists the
//! library lacks as readily as ones it has. [`blend`] puts the artists both
//! name first and interleaves the rest, each name once;
//! `AppService::similar_artists` then looks each name up on
//! the server, stamps the outcome on the row and puts the artists the library
//! lacks first ([`unowned_first`]), so a view leads with artists to discover,
//! links an artist the library has and sends the rest to the page of the
//! service that listed them.

/// A library artist a recommendation matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryArtist {
    /// The artist's Navidrome id.
    pub id: String,
    /// Whether the artist has albums of their own, so the Artists view lists
    /// and expands them. A featured-only artist (guest verses, a remix) is in
    /// the library through other artists' albums: their songs are what to
    /// show.
    pub album_artist: bool,
}

/// Where a recommended artist stands against the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryMatch {
    /// The library has the artist.
    Found(LibraryArtist),
    /// The library has no artist by that MusicBrainz id or name.
    Missing,
    /// The lookup failed, so nothing is known about the library.
    Unknown,
}

/// Which services list a recommended artist, with each one's key for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecSource {
    /// ListenBrainz, keyed by the artist's MusicBrainz id.
    ListenBrainz { mbid: String },
    /// Deezer, keyed by its own artist id.
    Deezer { id: u64 },
    /// Both list the artist.
    Both { mbid: String, deezer_id: u64 },
}

impl RecSource {
    /// The artist's MusicBrainz id, when ListenBrainz listed them.
    pub fn mbid(&self) -> Option<&str> {
        match self {
            Self::ListenBrainz { mbid } | Self::Both { mbid, .. } => Some(mbid),
            Self::Deezer { .. } => None,
        }
    }

    /// The artist's Deezer id, when Deezer listed them.
    pub fn deezer_id(&self) -> Option<u64> {
        match self {
            Self::Deezer { id } | Self::Both { deezer_id: id, .. } => Some(*id),
            Self::ListenBrainz { .. } => None,
        }
    }

    /// The artist's page on the service that listed them: ListenBrainz's
    /// when it did, else Deezer's.
    pub fn page_url(&self) -> String {
        match self {
            Self::ListenBrainz { mbid } | Self::Both { mbid, .. } => {
                format!("https://listenbrainz.org/artist/{mbid}/")
            }
            Self::Deezer { id } => format!("https://www.deezer.com/artist/{id}"),
        }
    }

    /// Which services list the artist, in words.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ListenBrainz { .. } => "ListenBrainz",
            Self::Deezer { .. } => "Deezer",
            Self::Both { .. } => "ListenBrainz and Deezer",
        }
    }

    /// This source with `other`'s keys folded in: the same artist, listed by
    /// another service too. A key this one already has is kept.
    fn merged(&self, other: &Self) -> Self {
        match (
            self.mbid().or_else(|| other.mbid()),
            self.deezer_id().or_else(|| other.deezer_id()),
        ) {
            (Some(mbid), Some(deezer_id)) => Self::Both {
                mbid: mbid.to_string(),
                deezer_id,
            },
            (Some(mbid), None) => Self::ListenBrainz {
                mbid: mbid.to_string(),
            },
            (None, Some(id)) => Self::Deezer { id },
            // Every source carries a key, so this cannot happen.
            (None, None) => self.clone(),
        }
    }
}

/// One artist ListenBrainz or Deezer finds similar to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistRec {
    /// Who listed the artist, and the key each one has for them.
    pub source: RecSource,
    pub name: String,
    /// MusicBrainz's disambiguation comment ("UK rock band"), when
    /// ListenBrainz listed the artist and MusicBrainz has one.
    pub comment: Option<String>,
    pub library: LibraryMatch,
}

impl ArtistRec {
    /// The artist's page on the service that listed them.
    pub fn page_url(&self) -> String {
        self.source.page_url()
    }

    /// The library artist this is, when the library has it.
    pub fn library_artist(&self) -> Option<&LibraryArtist> {
        match &self.library {
            LibraryMatch::Found(artist) => Some(artist),
            LibraryMatch::Missing | LibraryMatch::Unknown => None,
        }
    }

    /// The library's id for the artist, when it has it.
    pub fn library_id(&self) -> Option<&str> {
        self.library_artist().map(|artist| artist.id.as_str())
    }
}

/// Which services the similar-artists list asks: the two settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecSources {
    pub listenbrainz: bool,
    pub deezer: bool,
}

impl RecSources {
    /// Whether any service is asked, so a list shows at all.
    pub fn any(self) -> bool {
        self.listenbrainz || self.deezer
    }
}

/// How many similar artists are shown for one artist, out of the hundred
/// ListenBrainz sends and the twenty Deezer does. Each one shown costs a
/// server lookup.
pub const ARTIST_RECS_SHOWN: usize = 10;

/// How far down each service's list [`blend`] reads: Deezer sends twenty,
/// and ListenBrainz's picks past its twentieth (of a hundred) are too loose
/// a match to lead the list, or to take a place in it.
pub const BLEND_DEPTH: usize = 20;

/// Blend ListenBrainz's list and Deezer's into the first `limit` names. The
/// artists both services name lead: two services that count listening
/// differently agreeing on an artist is the strongest sign there is. The rest
/// follow taking turns, ListenBrainz first, so each service's best picks come
/// early, neither list crowds out the other, and a list that runs short leaves
/// its room to the other. Each group keeps the order the turns reached its
/// names in. An artist both name is kept once, with both services' keys and
/// ListenBrainz's comment. Only the first [`BLEND_DEPTH`] picks of each list
/// are read. Names are the only key the two services share, so two artists
/// who share a name are taken for one.
pub fn blend(listenbrainz: Vec<ArtistRec>, deezer: Vec<ArtistRec>, limit: usize) -> Vec<ArtistRec> {
    let mut kept: Vec<ArtistRec> = Vec::with_capacity(2 * BLEND_DEPTH);
    let mut listenbrainz = listenbrainz.into_iter().take(BLEND_DEPTH);
    let mut deezer = deezer.into_iter().take(BLEND_DEPTH);
    loop {
        let pair = [listenbrainz.next(), deezer.next()];
        if pair.iter().all(Option::is_none) {
            break;
        }
        for rec in pair.into_iter().flatten() {
            if let Some(same) = kept
                .iter_mut()
                .find(|k| same_artist_name(&k.name, &rec.name))
            {
                same.source = same.source.merged(&rec.source);
                if same.comment.is_none() {
                    same.comment = rec.comment;
                }
            } else {
                kept.push(rec);
            }
        }
    }
    kept.sort_by_key(|rec| !matches!(rec.source, RecSource::Both { .. }));
    kept.truncate(limit);
    kept
}

/// Put the artists the library lacks ahead of the ones it has, each group in
/// its own order: the list is for finding artists, and the library's own are
/// a click away anyway. An artist whose lookup failed counts as lacking.
pub fn unowned_first(recs: &mut [ArtistRec]) {
    recs.sort_by_key(|rec| rec.library_artist().is_some());
}

/// Pick the library artist that is `rec_mbid` / `rec_name` out of a name
/// search's candidates `(id, name, MusicBrainz id, album artist)`: the one
/// whose MusicBrainz id matches, else the first whose name matches exactly,
/// ignoring case and surrounding whitespace. Navidrome's name search matches
/// substrings ("Muse" finds "Musetta"), so the exact comparison is what keeps
/// a recommendation from linking to a different artist. A recommendation
/// from Deezer has no MusicBrainz id and matches on the name alone.
pub fn match_library<'a>(
    rec_mbid: Option<&str>,
    rec_name: &str,
    candidates: impl IntoIterator<Item = (&'a str, &'a str, Option<&'a str>, bool)>,
) -> Option<LibraryArtist> {
    let mut by_name = None;
    for (id, name, mbid, album_artist) in candidates {
        let found = LibraryArtist {
            id: id.to_string(),
            album_artist,
        };
        if let (Some(mbid), Some(rec_mbid)) = (mbid, rec_mbid)
            && mbid.eq_ignore_ascii_case(rec_mbid)
        {
            return Some(found);
        }
        if by_name.is_none() && same_artist_name(name, rec_name) {
            by_name = Some(found);
        }
    }
    by_name
}

/// Whether two artist names are the same name: equal after trimming, with
/// Unicode case folded (`to_lowercase`), so "the beatles" is "The Beatles"
/// and "Björk" is "björk".
pub(crate) fn same_artist_name(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    !a.is_empty() && (a == b || a.to_lowercase() == b.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIRVANA: &str = "5b11f4ce-a62d-471e-81fc-a69a8278c7da";

    fn found(id: &str) -> LibraryMatch {
        LibraryMatch::Found(LibraryArtist {
            id: id.into(),
            album_artist: true,
        })
    }

    fn rec(library: LibraryMatch) -> ArtistRec {
        ArtistRec {
            source: RecSource::ListenBrainz {
                mbid: NIRVANA.into(),
            },
            name: "Nirvana".into(),
            comment: None,
            library,
        }
    }

    fn lb(name: &str) -> ArtistRec {
        ArtistRec {
            source: RecSource::ListenBrainz {
                mbid: format!("mbid-{name}"),
            },
            name: name.into(),
            comment: Some(format!("note on {name}")),
            library: LibraryMatch::Unknown,
        }
    }

    fn dz(name: &str, id: u64) -> ArtistRec {
        ArtistRec {
            source: RecSource::Deezer { id },
            name: name.into(),
            comment: None,
            library: LibraryMatch::Unknown,
        }
    }

    fn names(recs: &[ArtistRec]) -> Vec<&str> {
        recs.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn the_page_is_listenbrainz_by_mbid_else_deezer_by_id() {
        assert_eq!(
            rec(LibraryMatch::Missing).page_url(),
            "https://listenbrainz.org/artist/5b11f4ce-a62d-471e-81fc-a69a8278c7da/"
        );
        assert_eq!(
            dz("Blur", 743).page_url(),
            "https://www.deezer.com/artist/743"
        );
        let both = RecSource::Both {
            mbid: NIRVANA.into(),
            deezer_id: 415,
        };
        assert_eq!(
            both.page_url(),
            "https://listenbrainz.org/artist/5b11f4ce-a62d-471e-81fc-a69a8278c7da/"
        );
    }

    #[test]
    fn only_a_found_artist_has_a_library_id() {
        assert_eq!(rec(found("ar1")).library_id(), Some("ar1"));
        assert_eq!(rec(LibraryMatch::Missing).library_id(), None);
        assert_eq!(rec(LibraryMatch::Unknown).library_id(), None);
    }

    /// The two lists take turns, ListenBrainz first, up to the limit.
    #[test]
    fn the_blend_takes_turns_listenbrainz_first() {
        let blended = blend(
            vec![lb("A"), lb("B"), lb("C")],
            vec![dz("X", 1), dz("Y", 2), dz("Z", 3)],
            5,
        );
        assert_eq!(names(&blended), ["A", "X", "B", "Y", "C"]);
    }

    /// A list that runs short (an artist ListenBrainz knows nothing about)
    /// leaves the room to the other.
    #[test]
    fn a_short_list_leaves_its_room_to_the_other() {
        let blended = blend(Vec::new(), vec![dz("X", 1), dz("Y", 2)], 10);
        assert_eq!(names(&blended), ["X", "Y"]);
        let blended = blend(vec![lb("A"), lb("B"), lb("C")], vec![dz("X", 1)], 10);
        assert_eq!(names(&blended), ["A", "X", "B", "C"]);
    }

    /// An artist both list is kept once, with both keys and ListenBrainz's
    /// note, whichever list named them first.
    #[test]
    fn an_artist_both_list_is_kept_once_with_both_keys() {
        let blended = blend(
            vec![lb("A"), lb("B"), lb("Blur")],
            vec![dz("blur", 743), dz("Y", 2)],
            10,
        );
        let blur = blended
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case("blur"))
            .expect("kept");
        assert_eq!(
            blended
                .iter()
                .filter(|r| r.name.eq_ignore_ascii_case("blur"))
                .count(),
            1
        );
        assert_eq!(
            blur.source,
            RecSource::Both {
                mbid: "mbid-Blur".into(),
                deezer_id: 743
            }
        );
        assert_eq!(blur.comment.as_deref(), Some("note on Blur"));
        assert_eq!(blur.source.label(), "ListenBrainz and Deezer");
    }

    /// The artists both services name lead, in the order the turns reached
    /// them, even when one list names them late; the rest follow taking
    /// turns.
    #[test]
    fn the_artists_both_name_come_first() {
        let blended = blend(
            vec![lb("A"), lb("B"), lb("C"), lb("Pixies"), lb("Blur")],
            vec![dz("Blur", 743), dz("X", 1), dz("Pixies", 652), dz("Y", 2)],
            5,
        );
        assert_eq!(names(&blended), ["Blur", "Pixies", "A", "B", "X"]);
    }

    /// Only the first [`BLEND_DEPTH`] picks of each list count: ListenBrainz
    /// sends a hundred, and a name deep in that tail is too loose a match to
    /// lead the list, or to take a place at all.
    #[test]
    fn a_pick_past_the_depth_neither_counts_as_agreement_nor_shows() {
        let mut listenbrainz: Vec<ArtistRec> =
            (0..BLEND_DEPTH).map(|i| lb(&format!("LB {i}"))).collect();
        listenbrainz.push(lb("Blur"));
        let blended = blend(listenbrainz, vec![dz("Blur", 743)], 40);
        let blur = blended.iter().find(|r| r.name == "Blur").expect("Deezer's");
        assert_eq!(blur.source, RecSource::Deezer { id: 743 });
        assert_ne!(blended[0].name, "Blur", "no agreement to lead with");
    }

    #[test]
    fn the_artists_the_library_lacks_come_first_in_their_own_order() {
        let mut recs = vec![
            ArtistRec {
                library: found("ar1"),
                ..lb("Owned 1")
            },
            lb("Missing 1"),
            ArtistRec {
                library: found("ar2"),
                ..lb("Owned 2")
            },
            ArtistRec {
                library: LibraryMatch::Missing,
                ..dz("Missing 2", 9)
            },
        ];
        unowned_first(&mut recs);
        assert_eq!(
            names(&recs),
            ["Missing 1", "Missing 2", "Owned 1", "Owned 2"]
        );
    }

    /// The MusicBrainz id outranks the name: a library artist tagged with
    /// the id is the match even when another candidate carries the name.
    #[test]
    fn a_musicbrainz_id_match_outranks_a_name_match() {
        let candidates = [
            ("ar1", "Nirvana", None, true),
            (
                "ar2",
                "Nirvana (UK)",
                Some("5B11F4CE-a62d-471e-81fc-a69a8278c7da"),
                false,
            ),
        ];
        assert_eq!(
            match_library(Some(NIRVANA), "Nirvana", candidates),
            Some(LibraryArtist {
                id: "ar2".into(),
                album_artist: false
            })
        );
    }

    /// Without an id match, the first exact name wins, case-insensitively;
    /// a substring hit from the server's search is never a match.
    #[test]
    fn a_name_matches_exactly_or_not_at_all() {
        let candidates = [
            ("ar1", "Musetta", Some("other"), true),
            ("ar2", " muse ", None, true),
            ("ar3", "Muse", None, true),
        ];
        assert_eq!(
            match_library(Some("9c9f1380"), "Muse", candidates).map(|a| a.id),
            Some("ar2".to_string())
        );
        assert_eq!(
            match_library(Some("9c9f1380"), "Muse", [("ar1", "Musetta", None, true)]),
            None
        );
        assert_eq!(match_library(None, "", [("ar1", "", None, true)]), None);
    }

    /// A Deezer pick has no MusicBrainz id: the name alone matches it, and a
    /// tagged library artist by that name is still found.
    #[test]
    fn a_deezer_pick_matches_on_the_name_alone() {
        let candidates = [("ar1", "Blur", Some("some-mbid"), true)];
        assert_eq!(
            match_library(None, "blur", candidates).map(|a| a.id),
            Some("ar1".to_string())
        );
    }
}
