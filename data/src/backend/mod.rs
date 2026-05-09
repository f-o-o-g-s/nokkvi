//! TEA-aligned backend services — orchestration and domain state
//!
//! `AppService` is the top-level orchestrator composing `PlaybackController` and
//! per-domain services (Albums, Artists, Songs, Genres, Playlists, Queue, Settings, Auth).
//! Each service owns its API client and provides view-ready data.

pub mod albums;
pub mod app_service;
pub mod artists;
pub mod auth;
pub mod genres;
pub mod library_orchestrator;
pub mod playback_controller;
pub mod playlists;
pub mod queue;
pub mod settings;
pub mod songs;

pub use library_orchestrator::LibraryOrchestrator;

/// Trait for entities whose starred status can be updated.
/// Abstracts over field name differences (`starred` vs `is_starred`).
pub trait Starable {
    fn entity_id(&self) -> &str;
    fn set_starred(&mut self, starred: bool);
    /// Display name for debug logging (e.g. "title - artist" or "name")
    fn display_label(&self) -> String;
}

/// Trait for entities whose rating can be updated.
pub trait Ratable {
    fn entity_id(&self) -> &str;
    fn set_rating(&mut self, rating: Option<u32>);
    fn display_label(&self) -> String;
}

/// Update starred status for the first matching entity in a slice.
/// Returns true if an entity was found and updated.
pub fn update_starred_in_list<T: Starable>(
    items: &mut [T],
    id: &str,
    starred: bool,
    entity_type: &str,
) -> bool {
    for item in items.iter_mut() {
        if item.entity_id() == id {
            item.set_starred(starred);
            tracing::debug!(
                "✅ Updated local {}: {} starred={}",
                entity_type,
                item.display_label(),
                starred
            );
            return true;
        }
    }
    false
}

/// Update rating for the first matching entity in a slice.
/// Returns true if an entity was found and updated.
pub fn update_rating_in_list<T: Ratable>(
    items: &mut [T],
    id: &str,
    rating: Option<u32>,
    entity_type: &str,
) -> bool {
    for item in items.iter_mut() {
        if item.entity_id() == id {
            item.set_rating(rating);
            tracing::debug!(
                "✅ Updated {}: {} rating={:?}",
                entity_type,
                item.display_label(),
                rating
            );
            return true;
        }
    }
    false
}

/// Trait for entities whose play count can be incremented.
pub trait PlayCountable {
    fn entity_id(&self) -> &str;
    fn play_count(&self) -> Option<u32>;
    fn set_play_count(&mut self, count: Option<u32>);
    fn display_label(&self) -> String;
}

/// Bump the play count of the first matching entity in a slice by 1.
/// `None` becomes `Some(1)`. Returns true if an entity was found.
pub fn increment_play_count_in_list<T: PlayCountable>(
    items: &mut [T],
    id: &str,
    entity_type: &str,
) -> bool {
    for item in items.iter_mut() {
        if item.entity_id() == id {
            let next = item.play_count().unwrap_or(0).saturating_add(1);
            item.set_play_count(Some(next));
            tracing::debug!(
                "✅ Bumped {}: {} play_count={}",
                entity_type,
                item.display_label(),
                next
            );
            return true;
        }
    }
    false
}

/// Flatten a participants map (role → artist list) into sorted display pairs.
/// Groups by role, appending sub-roles in parentheses like Feishin does.
/// Skips `albumartist` and `artist` roles (already shown as dedicated fields).
pub fn flatten_participants(
    participants: Option<&std::collections::HashMap<String, Vec<crate::types::album::Participant>>>,
) -> Vec<(String, String)> {
    let Some(map) = participants else {
        return Vec::new();
    };

    // Roles already displayed as dedicated fields — skip them
    const SKIP_ROLES: &[&str] = &["albumartist", "artist"];

    let mut pairs = Vec::new();
    for (role, artists) in map {
        if SKIP_ROLES.contains(&role.to_lowercase().as_str()) {
            continue;
        }
        // Group by sub-role
        let mut sub_groups: std::collections::BTreeMap<Option<String>, Vec<String>> =
            std::collections::BTreeMap::new();
        for artist in artists {
            sub_groups
                .entry(artist.sub_role.clone())
                .or_default()
                .push(artist.name.clone());
        }
        for (sub_role, names) in sub_groups {
            let label = match sub_role {
                Some(sr) if !sr.is_empty() => {
                    format!("{} ({})", titlecase_role(role), sr)
                }
                _ => titlecase_role(role),
            };
            let value = names.join(" • ");
            pairs.push((label, value));
        }
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

/// Title-case a participant role name (e.g. "composer" → "Composer").
fn titlecase_role(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{upper}{}", chars.collect::<String>())
        }
        None => String::new(),
    }
}
