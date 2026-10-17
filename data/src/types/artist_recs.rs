//! Similar artists for the Artists view: who ListenBrainz's listeners play in
//! the same sessions as the centred artist, each checked against the library.
//!
//! The list comes from ListenBrainz's similar-artists data
//! (`services::listenbrainz_similar`), keyed on the artist's MusicBrainz id, so
//! it names artists the library does not have as readily as ones it does.
//! `AppService::similar_artists` then looks each name up on the server and
//! stamps the outcome on the row, so the view can link an artist the library
//! has and send the rest to their ListenBrainz page.

/// Where a recommended artist stands against the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryMatch {
    /// The library has the artist, under this Navidrome id.
    Found { artist_id: String },
    /// The library has no artist by that MusicBrainz id or name.
    Missing,
    /// The lookup failed, so nothing is known about the library.
    Unknown,
}

/// One artist ListenBrainz finds similar to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistRec {
    /// The artist's MusicBrainz id, which is also its ListenBrainz page.
    pub mbid: String,
    pub name: String,
    /// MusicBrainz's disambiguation comment ("UK rock band"), when the
    /// artist has one.
    pub comment: Option<String>,
    /// ListenBrainz's similarity score; higher is more similar. Only its
    /// order means anything.
    pub score: u32,
    pub library: LibraryMatch,
}

impl ArtistRec {
    /// The artist's page on ListenBrainz, where the rest of the list came
    /// from.
    pub fn listenbrainz_url(&self) -> String {
        format!("https://listenbrainz.org/artist/{}/", self.mbid)
    }

    /// The library's id for the artist, when it has it.
    pub fn library_id(&self) -> Option<&str> {
        match &self.library {
            LibraryMatch::Found { artist_id } => Some(artist_id),
            LibraryMatch::Missing | LibraryMatch::Unknown => None,
        }
    }
}

/// How many similar artists the Artists view shows for one artist, out of
/// the hundred ListenBrainz sends. Each one shown costs a server lookup.
pub const ARTIST_RECS_SHOWN: usize = 10;

/// Pick the library artist that is `rec_mbid` / `rec_name` out of a name
/// search's candidates `(id, name, MusicBrainz id)`: the one whose MusicBrainz
/// id matches, else the first whose name matches exactly, ignoring case and
/// surrounding whitespace. Navidrome's name search matches substrings
/// ("Muse" finds "Musetta"), so the exact comparison is what keeps a
/// recommendation from linking to a different artist.
pub fn match_library<'a>(
    rec_mbid: &str,
    rec_name: &str,
    candidates: impl IntoIterator<Item = (&'a str, &'a str, Option<&'a str>)>,
) -> Option<String> {
    let mut by_name = None;
    for (id, name, mbid) in candidates {
        if mbid.is_some_and(|m| m.eq_ignore_ascii_case(rec_mbid)) {
            return Some(id.to_string());
        }
        if by_name.is_none() && same_artist_name(name, rec_name) {
            by_name = Some(id.to_string());
        }
    }
    by_name
}

/// Whether two artist names are the same name: equal after trimming, with
/// Unicode case folded (`to_lowercase`), so "the beatles" is "The Beatles"
/// and "Björk" is "björk".
fn same_artist_name(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    !a.is_empty() && (a == b || a.to_lowercase() == b.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(library: LibraryMatch) -> ArtistRec {
        ArtistRec {
            mbid: "5b11f4ce-a62d-471e-81fc-a69a8278c7da".into(),
            name: "Nirvana".into(),
            comment: None,
            score: 1,
            library,
        }
    }

    #[test]
    fn the_listenbrainz_page_is_the_artist_page_by_mbid() {
        assert_eq!(
            rec(LibraryMatch::Missing).listenbrainz_url(),
            "https://listenbrainz.org/artist/5b11f4ce-a62d-471e-81fc-a69a8278c7da/"
        );
    }

    #[test]
    fn only_a_found_artist_has_a_library_id() {
        assert_eq!(
            rec(LibraryMatch::Found {
                artist_id: "ar1".into()
            })
            .library_id(),
            Some("ar1")
        );
        assert_eq!(rec(LibraryMatch::Missing).library_id(), None);
        assert_eq!(rec(LibraryMatch::Unknown).library_id(), None);
    }

    /// The MusicBrainz id outranks the name: a library artist tagged with
    /// the id is the match even when another candidate carries the name.
    #[test]
    fn a_musicbrainz_id_match_outranks_a_name_match() {
        let candidates = [
            ("ar1", "Nirvana", None),
            (
                "ar2",
                "Nirvana (UK)",
                Some("5B11F4CE-a62d-471e-81fc-a69a8278c7da"),
            ),
        ];
        assert_eq!(
            match_library(
                "5b11f4ce-a62d-471e-81fc-a69a8278c7da",
                "Nirvana",
                candidates
            ),
            Some("ar2".to_string())
        );
    }

    /// Without an id match, the first exact name wins, case-insensitively;
    /// a substring hit from the server's search is never a match.
    #[test]
    fn a_name_matches_exactly_or_not_at_all() {
        let candidates = [
            ("ar1", "Musetta", Some("other")),
            ("ar2", " muse ", None),
            ("ar3", "Muse", None),
        ];
        assert_eq!(
            match_library("9c9f1380", "Muse", candidates),
            Some("ar2".to_string())
        );
        assert_eq!(
            match_library("9c9f1380", "Muse", [("ar1", "Musetta", None)]),
            None
        );
        assert_eq!(match_library("x", "", [("ar1", "", None)]), None);
    }
}
