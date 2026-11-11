//! The rows under an expanded artist: the artist's albums, and, when the
//! discography spans more than plain albums, a header row above each
//! release-type section (Albums, EPs, Singles, … Appears On).
//!
//! A header is a real slot (`DiscographyRow::Section`), so the expansion's
//! index mapping, navigation and selection treat it like any other child. A
//! collapsed section's albums move into its header (`SectionHeader::hidden`):
//! every album lives in exactly one place, and [`albums`] / [`albums_mut`]
//! walk them all, collapsed or not.

use std::collections::HashSet;

use nokkvi_data::{
    backend::albums::AlbumUIViewData,
    types::discography::{DiscographyGroup, group_albums, year_span},
};

/// A release-type section's header row.
#[derive(Debug, Clone)]
pub struct SectionHeader {
    pub group: DiscographyGroup,
    /// How many albums the section holds, open or collapsed.
    pub count: usize,
    /// The section's earliest and latest release year, when any is known.
    pub years: Option<(u32, u32)>,
    /// The section's albums while it is collapsed; empty while it is open
    /// (they are then the album rows that follow the header).
    pub hidden: Vec<AlbumUIViewData>,
}

impl SectionHeader {
    /// Whether the section's albums are folded into the header.
    pub(crate) fn is_collapsed(&self) -> bool {
        !self.hidden.is_empty()
    }

    /// The header's right-hand year text: `2003 – 2016`, one year when the
    /// section spans a single year, nothing when no album is dated.
    pub(crate) fn years_label(&self) -> Option<String> {
        self.years.map(|(first, last)| {
            if first == last {
                first.to_string()
            } else {
                format!("{first} \u{2013} {last}")
            }
        })
    }
}

/// One row under an expanded artist.
#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "nearly every row is an album; boxing them for the few header rows would cost an allocation per album"
)]
pub enum DiscographyRow {
    Section(SectionHeader),
    Album(AlbumUIViewData),
}

impl From<AlbumUIViewData> for DiscographyRow {
    fn from(album: AlbumUIViewData) -> Self {
        Self::Album(album)
    }
}

/// Build the rows for `artist_id`'s albums, which arrive in the server's
/// order (newest first).
///
/// With `grouped` off, or when grouping would say nothing (the artist has
/// only plain albums), the rows are the albums as they came. Otherwise each
/// section gets a header, and a section whose kind is in `collapsed` starts
/// folded.
pub(crate) fn build_rows(
    albums: Vec<AlbumUIViewData>,
    artist_id: &str,
    grouped: bool,
    collapsed: &HashSet<DiscographyGroup>,
) -> Vec<DiscographyRow> {
    if !grouped {
        return albums.into_iter().map(DiscographyRow::Album).collect();
    }
    let sections = group_albums(albums, artist_id);
    let headers_say_something = match sections.as_slice() {
        [] => false,
        [(only, _)] => *only != DiscographyGroup::Albums,
        [_, _, ..] => true,
    };
    let mut rows = Vec::new();
    for (group, members) in sections {
        if !headers_say_something {
            rows.extend(members.into_iter().map(DiscographyRow::Album));
            continue;
        }
        let mut header = SectionHeader {
            group,
            count: members.len(),
            years: year_span(&members),
            hidden: Vec::new(),
        };
        if collapsed.contains(&group) {
            header.hidden = members;
            rows.push(DiscographyRow::Section(header));
        } else {
            rows.push(DiscographyRow::Section(header));
            rows.extend(members.into_iter().map(DiscographyRow::Album));
        }
    }
    rows
}

/// Every album under the expanded artist in display order, the ones folded
/// into a collapsed header included.
pub(crate) fn albums(rows: &[DiscographyRow]) -> impl Iterator<Item = &AlbumUIViewData> {
    rows.iter().flat_map(|row| match row {
        DiscographyRow::Album(album) => std::slice::from_ref(album).iter(),
        DiscographyRow::Section(header) => header.hidden.iter(),
    })
}

/// [`albums`], mutably: star, rating and play-count sync reach a collapsed
/// section's albums through the same walk.
pub(crate) fn albums_mut(
    rows: &mut [DiscographyRow],
) -> impl Iterator<Item = &mut AlbumUIViewData> {
    rows.iter_mut().flat_map(|row| match row {
        DiscographyRow::Album(album) => std::slice::from_mut(album).iter_mut(),
        DiscographyRow::Section(header) => header.hidden.iter_mut(),
    })
}

/// The 1-based place of the album at `idx` among all of the artist's albums
/// (headers take no number; a collapsed section's albums keep theirs, so a
/// row's number never changes when a section above it folds).
pub(crate) fn album_ordinal(rows: &[DiscographyRow], idx: usize) -> usize {
    let before = rows.get(..idx).unwrap_or(rows);
    albums(before).count() + 1
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::discography::ReleaseType;

    use super::*;
    use crate::test_helpers::make_album;

    const ARTIST: &str = "ar1";

    fn typed(id: &str, types: &[ReleaseType], year: u32) -> AlbumUIViewData {
        let mut album = make_album(id, id, "Artist");
        album.album_artist_ids = vec![ARTIST.to_string()];
        album.release_types = types.to_vec();
        album.year = Some(year);
        album
    }

    fn mixed() -> Vec<AlbumUIViewData> {
        vec![
            typed("ep-new", &[ReleaseType::Ep], 2020),
            typed("lp-new", &[ReleaseType::Album], 2016),
            typed("single", &[ReleaseType::Single], 2012),
            typed("ep-old", &[ReleaseType::Ep], 2009),
            typed("lp-old", &[ReleaseType::Album], 2003),
        ]
    }

    /// The rows as short strings: `#EPs` for a header, the id for an album.
    fn shape(rows: &[DiscographyRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                DiscographyRow::Section(header) => format!("#{}", header.group.label()),
                DiscographyRow::Album(album) => album.id.clone(),
            })
            .collect()
    }

    fn ids<'a>(albums: impl Iterator<Item = &'a AlbumUIViewData>) -> Vec<&'a str> {
        albums.map(|a| a.id.as_str()).collect()
    }

    #[test]
    fn a_mixed_discography_gets_a_header_per_section() {
        let rows = build_rows(mixed(), ARTIST, true, &HashSet::new());
        assert_eq!(
            shape(&rows),
            [
                "#Albums", "lp-new", "lp-old", "#EPs", "ep-new", "ep-old", "#Singles", "single"
            ]
        );
        let DiscographyRow::Section(albums_header) = &rows[0] else {
            panic!("row 0 is the Albums header");
        };
        assert_eq!(albums_header.count, 2);
        assert_eq!(albums_header.years, Some((2003, 2016)));
        assert_eq!(
            albums_header.years_label().as_deref(),
            Some("2003 \u{2013} 2016")
        );
        let DiscographyRow::Section(singles) = &rows[6] else {
            panic!("row 6 is the Singles header");
        };
        assert_eq!(singles.years_label().as_deref(), Some("2012"));
    }

    #[test]
    fn plain_albums_only_stay_a_flat_list() {
        let plain = vec![
            typed("b", &[ReleaseType::Album], 2010),
            typed("a", &[], 2005),
        ];
        let rows = build_rows(plain, ARTIST, true, &HashSet::new());
        assert_eq!(shape(&rows), ["b", "a"]);
        assert!(build_rows(Vec::new(), ARTIST, true, &HashSet::new()).is_empty());
    }

    #[test]
    fn a_single_section_that_is_not_albums_gets_its_header() {
        let eps = vec![typed("e1", &[ReleaseType::Ep], 2010)];
        assert_eq!(
            shape(&build_rows(eps, ARTIST, true, &HashSet::new())),
            ["#EPs", "e1"]
        );
        let mut guest = typed("g1", &[ReleaseType::Album], 2010);
        guest.album_artist_ids = vec!["someone-else".to_string()];
        assert_eq!(
            shape(&build_rows(vec![guest], ARTIST, true, &HashSet::new())),
            ["#Appears On", "g1"]
        );
    }

    #[test]
    fn grouping_off_keeps_the_server_order() {
        let rows = build_rows(mixed(), ARTIST, false, &HashSet::new());
        assert_eq!(
            shape(&rows),
            ["ep-new", "lp-new", "single", "ep-old", "lp-old"]
        );
    }

    #[test]
    fn a_remembered_collapse_folds_the_section_on_build() {
        let collapsed = HashSet::from([DiscographyGroup::Eps]);
        let rows = build_rows(mixed(), ARTIST, true, &collapsed);
        assert_eq!(
            shape(&rows),
            ["#Albums", "lp-new", "lp-old", "#EPs", "#Singles", "single"]
        );
        // The folded albums are still there for every walk.
        assert_eq!(
            ids(albums(&rows)),
            ["lp-new", "lp-old", "ep-new", "ep-old", "single"]
        );
    }

    #[test]
    fn album_numbers_skip_headers_and_survive_a_fold() {
        let mut rows = build_rows(mixed(), ARTIST, true, &HashSet::new());
        assert_eq!(album_ordinal(&rows, 1), 1);
        assert_eq!(album_ordinal(&rows, 2), 2);
        assert_eq!(album_ordinal(&rows, 4), 3);
        assert_eq!(album_ordinal(&rows, 7), 5);
        // With the EPs folded, "single" sits two rows up and keeps its number.
        rows = build_rows(
            mixed(),
            ARTIST,
            true,
            &HashSet::from([DiscographyGroup::Eps]),
        );
        assert_eq!(album_ordinal(&rows, 5), 5);
    }

    #[test]
    fn albums_mut_reaches_a_folded_album() {
        let mut rows = build_rows(
            mixed(),
            ARTIST,
            true,
            &HashSet::from([DiscographyGroup::Eps]),
        );
        for album in albums_mut(&mut rows) {
            album.is_starred = true;
        }
        assert!(albums(&rows).all(|a| a.is_starred));
        assert_eq!(albums(&rows).count(), 5);
    }
}
