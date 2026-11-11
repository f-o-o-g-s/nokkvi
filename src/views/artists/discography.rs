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

impl DiscographyRow {
    /// The album this row shows; `None` for a section header.
    pub(crate) fn album(&self) -> Option<&AlbumUIViewData> {
        match self {
            Self::Album(album) => Some(album),
            Self::Section(_) => None,
        }
    }
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

/// The albums of the section whose header sits at `header_idx` in `rows`, in
/// display order: the header's own while collapsed, else the album rows up
/// to the next header. Empty when `header_idx` is not a header.
pub(crate) fn section_albums(
    rows: &[DiscographyRow],
    header_idx: usize,
) -> impl Iterator<Item = &AlbumUIViewData> {
    let (hidden, open): (&[AlbumUIViewData], &[DiscographyRow]) = match rows.get(header_idx) {
        Some(DiscographyRow::Section(header)) if header.is_collapsed() => (&header.hidden, &[]),
        Some(DiscographyRow::Section(_)) => (&[], &rows[header_idx + 1..]),
        Some(DiscographyRow::Album(_)) | None => (&[], &[]),
    };
    hidden
        .iter()
        .chain(open.iter().map_while(DiscographyRow::album))
}

/// The albums of section `group`, wherever its header sits; empty when the
/// rows have no such section.
pub(crate) fn albums_of_group(
    rows: &[DiscographyRow],
    group: DiscographyGroup,
) -> impl Iterator<Item = &AlbumUIViewData> {
    let header_idx = rows
        .iter()
        .position(|row| matches!(row, DiscographyRow::Section(header) if header.group == group));
    // `usize::MAX` is past every row, which `section_albums` reads as "none".
    section_albums(rows, header_idx.unwrap_or(usize::MAX))
}

/// The section the row at `idx` belongs to: the nearest header at or above
/// it. `None` in a flat list, which has no headers.
pub(crate) fn section_of(rows: &[DiscographyRow], idx: usize) -> Option<DiscographyGroup> {
    rows.get(..=idx)?.iter().rev().find_map(|row| match row {
        DiscographyRow::Section(header) => Some(header.group),
        DiscographyRow::Album(_) => None,
    })
}

/// The 1-based place of the album at `idx` among all of the artist's albums
/// (headers take no number; a collapsed section's albums keep theirs, so a
/// row's number never changes when a section above it folds).
pub(crate) fn album_ordinal(rows: &[DiscographyRow], idx: usize) -> usize {
    let before = rows.get(..idx).unwrap_or(rows);
    albums(before).count() + 1
}

/// Fold or unfold the section whose header sits at `header_idx`. Returns the
/// section and whether it is now collapsed; `None` (and no change) when the
/// row is not a header.
pub(crate) fn toggle_section(
    rows: &mut Vec<DiscographyRow>,
    header_idx: usize,
) -> Option<(DiscographyGroup, bool)> {
    let Some(DiscographyRow::Section(header)) = rows.get(header_idx) else {
        return None;
    };
    let group = header.group;
    if header.is_collapsed() {
        let Some(DiscographyRow::Section(header)) = rows.get_mut(header_idx) else {
            return None;
        };
        let members = std::mem::take(&mut header.hidden);
        let at = header_idx + 1;
        rows.splice(at..at, members.into_iter().map(DiscographyRow::Album));
        Some((group, false))
    } else {
        let start = header_idx + 1;
        let len = rows[start..]
            .iter()
            .take_while(|row| row.album().is_some())
            .count();
        let members: Vec<AlbumUIViewData> = rows
            .drain(start..start + len)
            .filter_map(|row| match row {
                DiscographyRow::Album(album) => Some(album),
                DiscographyRow::Section(_) => None,
            })
            .collect();
        if let Some(DiscographyRow::Section(header)) = rows.get_mut(header_idx) {
            header.hidden = members;
        }
        Some((group, true))
    }
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
    fn toggling_a_section_folds_and_unfolds_it_in_place() {
        let mut rows = build_rows(mixed(), ARTIST, true, &HashSet::new());
        let open = shape(&rows);

        assert_eq!(
            toggle_section(&mut rows, 3),
            Some((DiscographyGroup::Eps, true))
        );
        assert_eq!(
            shape(&rows),
            ["#Albums", "lp-new", "lp-old", "#EPs", "#Singles", "single"]
        );
        let DiscographyRow::Section(eps) = &rows[3] else {
            panic!("row 3 is the EPs header");
        };
        assert!(eps.is_collapsed());
        assert_eq!(eps.count, 2);

        assert_eq!(
            toggle_section(&mut rows, 3),
            Some((DiscographyGroup::Eps, false))
        );
        assert_eq!(shape(&rows), open);

        // The last section folds up to the end of the list.
        assert_eq!(
            toggle_section(&mut rows, 6),
            Some((DiscographyGroup::Singles, true))
        );
        assert_eq!(rows.len(), 7);
        // An album row is not a header.
        assert_eq!(toggle_section(&mut rows, 1), None);
        assert_eq!(toggle_section(&mut rows, 99), None);
    }

    #[test]
    fn section_albums_stop_at_the_next_header() {
        let rows = build_rows(mixed(), ARTIST, true, &HashSet::new());
        assert_eq!(ids(section_albums(&rows, 0)), ["lp-new", "lp-old"]);
        assert_eq!(ids(section_albums(&rows, 6)), ["single"]);
        assert!(section_albums(&rows, 1).next().is_none());
        assert!(section_albums(&rows, 99).next().is_none());
        assert_eq!(
            ids(albums_of_group(&rows, DiscographyGroup::Eps)),
            ["ep-new", "ep-old"]
        );
        assert!(
            albums_of_group(&rows, DiscographyGroup::Live)
                .next()
                .is_none()
        );
    }

    #[test]
    fn a_row_belongs_to_the_nearest_header_above_it() {
        let rows = build_rows(mixed(), ARTIST, true, &HashSet::new());
        assert_eq!(section_of(&rows, 0), Some(DiscographyGroup::Albums));
        assert_eq!(section_of(&rows, 2), Some(DiscographyGroup::Albums));
        assert_eq!(section_of(&rows, 4), Some(DiscographyGroup::Eps));
        assert_eq!(section_of(&rows, 7), Some(DiscographyGroup::Singles));
        assert_eq!(section_of(&rows, 99), None);
        let flat = build_rows(mixed(), ARTIST, false, &HashSet::new());
        assert_eq!(section_of(&flat, 3), None);
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
