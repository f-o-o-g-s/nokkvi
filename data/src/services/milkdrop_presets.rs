//! The MilkDrop preset library: the bundled pack plus the user's own presets,
//! the curation file (hidden + favorite presets) and the shuffle bag that picks
//! the next preset.
//!
//! Iced-free and wgpu-free: this module never parses a preset, it only names
//! them and hands out their source text. Every path is a parameter; nothing
//! here calls `utils::paths` itself, so tests run against a temp dir.

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashSet},
    path::{Path, PathBuf},
};

use rand::{RngExt, seq::SliceRandom};

pub use crate::types::visualizer_config::MilkdropPresetSource;

/// nokkvi's own presets (theme-coloured, cover-aware) are named with this
/// prefix; the `nokkvi` preset source draws only from them.
pub const NOKKVI_PRESET_PREFIX: &str = "nokkvi - ";
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Where a preset's JSON comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetSource {
    /// Embedded in the binary (`assets/milkdrop/`).
    Bundled(&'static str),
    /// A `*.json` file in the user's MilkDrop directory.
    User(PathBuf),
}

impl PresetSource {
    /// The preset's JSON text. A user file is read from disk, so call this off
    /// the UI thread.
    pub fn read(&self) -> std::io::Result<Cow<'static, str>> {
        match self {
            Self::Bundled(text) => Ok(Cow::Borrowed(text)),
            Self::User(path) => std::fs::read_to_string(path).map(Cow::Owned),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetEntry {
    /// The file stem, which carries the author's credit.
    pub name: String,
    pub source: PresetSource,
}

/// Hidden and favorite presets, persisted as `curation.toml` in the user's
/// MilkDrop directory (`hidden = [...]`, `favorites = [...]`).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curation {
    pub hidden: BTreeSet<String>,
    pub favorites: BTreeSet<String>,
}

impl Curation {
    /// Missing file → empty curation; a file that cannot be read or parsed is
    /// an `Err` so the caller can refuse to write over the user's edits.
    pub fn try_load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        toml::from_str(&text).map_err(|e| format!("cannot parse {}: {e}", path.display()))
    }

    /// [`Self::try_load`], logging an error and treating it as empty; it never
    /// stops the mode from running.
    pub fn load(path: &Path) -> Self {
        Self::try_load(path).unwrap_or_else(|e| {
            warn!("milkdrop: ignoring the curation file: {e}");
            Self::default()
        })
    }

    /// Write atomically, creating the parent directory on first save.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self)?;
        crate::utils::paths::write_atomic(path, &text)
    }
}

/// The presets MilkDrop mode can draw from, and the order it draws them in.
#[derive(Debug, Default)]
pub struct PresetLibrary {
    bundled: &'static [(&'static str, &'static str)],
    user_dir: PathBuf,
    /// Sorted by name; a user file shadows a bundled preset with the same stem.
    entries: Vec<PresetEntry>,
    curation: Curation,
    source: MilkdropPresetSource,
    /// Names still to be drawn this round, popped from the end.
    bag: Vec<String>,
    /// Presets that failed to load this session: out of the rotation until
    /// the next login, never written anywhere.
    broken: HashSet<String>,
}

impl PresetLibrary {
    /// Build from the bundled table plus every `*.json` in `user_dir` (a missing
    /// directory simply adds nothing).
    pub fn new(
        bundled: &'static [(&'static str, &'static str)],
        user_dir: &Path,
        curation: Curation,
    ) -> Self {
        let mut library = Self {
            bundled,
            user_dir: user_dir.to_path_buf(),
            entries: Vec::new(),
            curation,
            source: MilkdropPresetSource::All,
            bag: Vec::new(),
            broken: HashSet::new(),
        };
        library.rescan_user_dir();
        library
    }

    /// Re-read the user directory (on login and on refresh, never per frame).
    pub fn rescan_user_dir(&mut self) {
        let mut entries: Vec<PresetEntry> = self
            .bundled
            .iter()
            .map(|(name, text)| PresetEntry {
                name: (*name).to_string(),
                source: PresetSource::Bundled(text),
            })
            .collect();

        for (name, path) in scan_user_dir(&self.user_dir) {
            match entries.iter_mut().find(|e| e.name == name) {
                Some(entry) => entry.source = PresetSource::User(path),
                None => entries.push(PresetEntry {
                    name,
                    source: PresetSource::User(path),
                }),
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        self.entries = entries;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[PresetEntry] {
        &self.entries
    }

    pub fn source(&self, name: &str) -> Option<&PresetSource> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| &e.source)
    }

    pub fn curation(&self) -> &Curation {
        &self.curation
    }

    /// Replace the curation (the file was re-read).
    pub fn set_curation(&mut self, curation: Curation) {
        self.curation = curation;
        self.bag.clear();
    }

    /// Take a preset that failed to load out of the rotation for this session.
    pub fn mark_broken(&mut self, name: &str) {
        self.bag.retain(|n| n != name);
        self.broken.insert(name.to_string());
    }

    /// Whether any preset is eligible (the allocation-free form of `eligible`).
    pub fn has_eligible(&self) -> bool {
        self.entries.iter().any(|e| self.is_drawable(&e.name))
    }

    fn is_drawable(&self, name: &str) -> bool {
        !self.curation.hidden.contains(name) && !self.broken.contains(name)
    }

    /// Which presets the draws come from (the Presets setting).
    pub fn set_source(&mut self, source: MilkdropPresetSource) {
        if self.source != source {
            self.source = source;
            self.bag.clear();
        }
    }

    pub fn is_hidden(&self, name: &str) -> bool {
        self.curation.hidden.contains(name)
    }

    pub fn is_favorite(&self, name: &str) -> bool {
        self.curation.favorites.contains(name)
    }

    /// Hide a preset for good. Returns `false` when it was already hidden.
    pub fn hide(&mut self, name: &str) -> bool {
        self.bag.retain(|n| n != name);
        self.curation.hidden.insert(name.to_string())
    }

    /// Flip a preset's favorite mark; returns the new state.
    pub fn toggle_favorite(&mut self, name: &str) -> bool {
        let now_favorite = if self.curation.favorites.remove(name) {
            false
        } else {
            self.curation.favorites.insert(name.to_string());
            true
        };
        if self.source == MilkdropPresetSource::FavoritesOnly {
            self.bag.clear();
        }
        now_favorite
    }

    /// Names the next draw may pick: not hidden (nor broken this session),
    /// narrowed by the source (favorites, or nokkvi's own presets). A source
    /// with nothing eligible falls back to every drawable preset.
    pub fn eligible(&self) -> Vec<&str> {
        let visible = self
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .filter(|n| self.is_drawable(n));
        let narrowed: Option<Vec<&str>> = match self.source {
            MilkdropPresetSource::All => None,
            MilkdropPresetSource::FavoritesOnly => Some(
                visible
                    .clone()
                    .filter(|n| self.curation.favorites.contains(*n))
                    .collect(),
            ),
            MilkdropPresetSource::Nokkvi => Some(
                visible
                    .clone()
                    .filter(|n| n.starts_with(NOKKVI_PRESET_PREFIX))
                    .collect(),
            ),
        };
        match narrowed {
            Some(names) if !names.is_empty() => names,
            Some(_) | None => visible.collect(),
        }
    }

    /// Draw the next preset from the shuffle bag: every eligible preset once
    /// before any repeats, and never `current` again while more than one preset
    /// is eligible. `None` when nothing is eligible.
    pub fn next<R: RngExt>(&mut self, current: Option<&str>, rng: &mut R) -> Option<String> {
        let eligible: Vec<String> = self.eligible().into_iter().map(str::to_string).collect();
        if eligible.is_empty() {
            self.bag.clear();
            return None;
        }
        let allowed: HashSet<&str> = eligible.iter().map(String::as_str).collect();
        self.bag.retain(|n| allowed.contains(n.as_str()));

        if self.bag.is_empty() {
            self.refill(&eligible, current, rng);
        }
        if eligible.len() > 1 && self.bag.last().map(String::as_str) == current {
            if self.bag.len() > 1 {
                let last = self.bag.len() - 1;
                self.bag.swap(0, last);
            } else {
                self.refill(&eligible, current, rng);
            }
        }
        self.bag.pop()
    }

    /// [`Self::next`] with the thread RNG.
    pub fn next_random(&mut self, current: Option<&str>) -> Option<String> {
        self.next(current, &mut rand::rng())
    }

    fn refill<R: RngExt>(&mut self, eligible: &[String], avoid_last: Option<&str>, rng: &mut R) {
        self.bag = eligible.to_vec();
        self.bag.shuffle(rng);
        let last = self.bag.len().saturating_sub(1);
        if self.bag.len() > 1 && self.bag.last().map(String::as_str) == avoid_last {
            self.bag.swap(0, last);
        }
    }
}

/// `(stem, path)` for every `*.json` directly in `dir`, sorted by stem.
fn scan_user_dir(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = read
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if !path.is_file() || path.extension().is_none_or(|e| e != "json") {
                return None;
            }
            Some((path.file_stem()?.to_str()?.to_string(), path))
        })
        .collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    static BUNDLED: &[(&str, &str)] = &[("a", "{\"a\":1}"), ("b", "{}"), ("c", "{}"), ("d", "{}")];

    fn library(dir: &Path) -> PresetLibrary {
        PresetLibrary::new(BUNDLED, dir, Curation::default())
    }

    fn no_user_dir() -> PathBuf {
        PathBuf::from("/nonexistent/nokkvi-milkdrop-test")
    }

    #[test]
    fn shuffle_bag_visits_every_eligible_before_repeating() {
        let mut lib = library(&no_user_dir());
        let mut rng = StdRng::seed_from_u64(7);
        let mut current: Option<String> = None;
        for _round in 0..3 {
            let mut seen = BTreeSet::new();
            for _ in 0..4 {
                let next = lib.next(current.as_deref(), &mut rng).expect("eligible");
                assert!(seen.insert(next.clone()), "{next} repeated within a round");
                current = Some(next);
            }
            assert_eq!(seen.len(), 4);
        }
    }

    #[test]
    fn next_never_repeats_current_when_two_or_more_are_eligible() {
        let mut lib = library(&no_user_dir());
        let mut rng = StdRng::seed_from_u64(1);
        let mut current: Option<String> = None;
        for _ in 0..200 {
            let next = lib.next(current.as_deref(), &mut rng).expect("eligible");
            assert_ne!(Some(next.as_str()), current.as_deref());
            current = Some(next);
        }
        // Down to two eligible: still alternates.
        lib.hide("a");
        lib.hide("b");
        for _ in 0..20 {
            let next = lib.next(current.as_deref(), &mut rng).expect("eligible");
            assert_ne!(Some(next.as_str()), current.as_deref());
            current = Some(next);
        }
        // One eligible: it may repeat (there is nothing else).
        lib.hide("c");
        assert_eq!(lib.next(Some("d"), &mut rng).as_deref(), Some("d"));
    }

    #[test]
    fn hidden_presets_are_never_drawn() {
        let mut lib = library(&no_user_dir());
        let mut rng = StdRng::seed_from_u64(3);
        let _ = lib.next(None, &mut rng);
        assert!(lib.hide("b"));
        assert!(!lib.hide("b"), "hiding twice reports no change");
        assert!(lib.is_hidden("b"));
        for _ in 0..50 {
            assert_ne!(lib.next(None, &mut rng).as_deref(), Some("b"));
        }
        for name in ["a", "c", "d"] {
            lib.hide(name);
        }
        assert_eq!(lib.next(None, &mut rng), None, "everything hidden → None");
    }

    #[test]
    fn favorites_only_falls_back_to_all_when_none_eligible() {
        let mut lib = library(&no_user_dir());
        lib.set_source(MilkdropPresetSource::FavoritesOnly);
        assert_eq!(lib.eligible(), ["a", "b", "c", "d"], "no favorites → all");

        assert!(lib.toggle_favorite("c"));
        assert!(lib.is_favorite("c"));
        assert_eq!(lib.eligible(), ["c"]);

        lib.hide("c");
        assert_eq!(
            lib.eligible(),
            ["a", "b", "d"],
            "hidden favorite → fall back"
        );

        assert!(
            !lib.toggle_favorite("c"),
            "second toggle clears the favorite"
        );
        lib.set_source(MilkdropPresetSource::All);
        assert_eq!(lib.eligible(), ["a", "b", "d"]);
    }

    #[test]
    fn user_preset_shadows_bundled_with_same_stem() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.json"), "{\"user\":true}").expect("write");
        std::fs::write(dir.path().join("zz mine.json"), "{}").expect("write");
        std::fs::write(dir.path().join("notes.txt"), "ignored").expect("write");
        let lib = library(dir.path());

        let names: Vec<&str> = lib.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c", "d", "zz mine"]);
        assert_eq!(
            lib.source("a"),
            Some(&PresetSource::User(dir.path().join("a.json")))
        );
        let text = lib.source("a").expect("a").read().expect("read");
        assert_eq!(text, "{\"user\":true}");
        assert_eq!(
            lib.source("b").expect("b").read().expect("read"),
            "{}",
            "unshadowed presets stay bundled"
        );
    }

    #[test]
    fn nokkvi_source_draws_only_nokkvi_presets_and_falls_back() {
        static TABLE: &[(&str, &str)] = &[("a", "{}"), ("nokkvi - x", "{}"), ("nokkvi - y", "{}")];
        let mut lib = PresetLibrary::new(TABLE, &no_user_dir(), Curation::default());
        lib.set_source(MilkdropPresetSource::Nokkvi);
        assert_eq!(lib.eligible(), ["nokkvi - x", "nokkvi - y"]);
        lib.hide("nokkvi - x");
        lib.hide("nokkvi - y");
        assert_eq!(lib.eligible(), ["a"], "none left → all");
    }

    #[test]
    fn broken_presets_leave_the_rotation() {
        let mut lib = library(&no_user_dir());
        lib.mark_broken("a");
        assert_eq!(lib.eligible(), ["b", "c", "d"]);
        for name in ["b", "c", "d"] {
            lib.hide(name);
        }
        assert!(!lib.has_eligible());
        assert_eq!(lib.next(None, &mut StdRng::seed_from_u64(1)), None);
    }

    #[test]
    fn rescan_picks_up_a_new_user_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut lib = library(dir.path());
        assert_eq!(lib.len(), 4);
        std::fs::write(dir.path().join("e.json"), "{}").expect("write");
        lib.rescan_user_dir();
        assert_eq!(lib.len(), 5);
        assert!(lib.source("e").is_some());
    }

    #[test]
    fn curation_round_trips_and_tolerates_missing_and_broken_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("milkdrop").join("curation.toml");

        assert_eq!(Curation::load(&path), Curation::default(), "missing file");

        let mut curation = Curation::default();
        curation.hidden.insert("Geiss - dud".to_string());
        curation
            .favorites
            .insert("flexi - \"quoted\" $name".to_string());
        curation.save(&path).expect("save creates the directory");
        assert_eq!(Curation::load(&path), curation);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("hidden = ["), "{text}");
        assert!(text.contains("favorites = ["), "{text}");

        std::fs::write(&path, "hidden = [unterminated").expect("write");
        assert_eq!(Curation::load(&path), Curation::default(), "broken file");
        assert!(Curation::try_load(&path).is_err(), "try_load reports it");
    }
}
