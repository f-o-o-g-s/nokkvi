//! Countable nouns for user-facing counts: `1 song`, `2 songs`.
//!
//! Every count the UI shows goes through a [`Noun`], so none of them reads
//! `1 songs`. Log lines are exempt: they keep plain plurals.

use std::fmt::Display;

/// A noun's singular and plural spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Noun {
    one: &'static str,
    many: &'static str,
}

impl Noun {
    pub const ALBUM: Self = Self::new("album", "albums");
    pub const ARTIST: Self = Self::new("artist", "artists");
    pub const DUPLICATE: Self = Self::new("duplicate", "duplicates");
    pub const GENRE: Self = Self::new("genre", "genres");
    pub const ITEM: Self = Self::new("item", "items");
    pub const MIN: Self = Self::new("min", "mins");
    pub const PICK: Self = Self::new("pick", "picks");
    pub const PLAY: Self = Self::new("play", "plays");
    pub const PLAYLIST: Self = Self::new("playlist", "playlists");
    pub const SEED: Self = Self::new("seed", "seeds");
    pub const SONG: Self = Self::new("song", "songs");
    pub const STATION: Self = Self::new("station", "stations");
    pub const TRACK: Self = Self::new("track", "tracks");

    /// A noun the constants don't cover; give both spellings, since English
    /// plurals are not always `+s` (`entry` / `entries`).
    pub const fn new(one: &'static str, many: &'static str) -> Self {
        Self { one, many }
    }

    /// The spelling that agrees with `n`: `word(1)` is `song`, `word(0)` and
    /// `word(2)` are `songs`. For phrases where the count isn't right before
    /// the noun (`100 of 4210 songs`).
    pub fn word<N: PartialEq + From<u8>>(self, n: N) -> &'static str {
        if n == N::from(1) { self.one } else { self.many }
    }

    /// `n` and the agreeing spelling: `1 song`, `3 songs`.
    pub fn count<N: Display + PartialEq + From<u8> + Copy>(self, n: N) -> String {
        format!("{n} {}", self.word(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_is_singular_only_for_one() {
        assert_eq!(Noun::SONG.count(0_u32), "0 songs");
        assert_eq!(Noun::SONG.count(1_u32), "1 song");
        assert_eq!(Noun::SONG.count(2_u32), "2 songs");
        assert_eq!(Noun::ALBUM.count(1_usize), "1 album");
        assert_eq!(Noun::PLAY.count(12_i64), "12 plays");
    }

    #[test]
    fn word_agrees_with_the_count() {
        assert_eq!(Noun::TRACK.word(1_u32), "track");
        assert_eq!(Noun::TRACK.word(0_u32), "tracks");
        assert_eq!(Noun::TRACK.word(5_usize), "tracks");
    }

    #[test]
    fn a_custom_noun_keeps_its_irregular_plural() {
        let entry = Noun::new("entry", "entries");
        assert_eq!(entry.count(1_u32), "1 entry");
        assert_eq!(entry.count(3_u32), "3 entries");
    }
}
