//! Multi-library filter — popover open/close, toggle, refresh.
//!
//! Drives the nav-bar library selector. The four `LibraryMessage`
//! variants split as:
//!
//! - `OpenChange { open: true }` opens the `LibrarySelector` overlay and
//!   triggers a lazy fetch of the library list when the cache is cold.
//!   Mutual exclusion with the other overlay menus (hamburger, kebab,
//!   column dropdown, context menu) is enforced implicitly: assigning
//!   `self.open_menu = Some(LibrarySelector { … })` atomically replaces
//!   whatever was open.
//! - `OpenChange { open: false }` clears `self.open_menu`.
//! - `Toggle(id)` flips membership of `id` in the backend's active set
//!   (persisted via `AppService::toggle_library`), then invalidates every
//!   paged library buffer so the next render fetches with the new
//!   filter. The buffer for the currently-visible view fetches eagerly;
//!   the rest refetch lazily on next visit via the standard
//!   `LoadAlbums`/`LoadArtists`/… handlers checking `is_empty()`.
//! - `Loaded(libs)` applies a refreshed library list to the backend
//!   (which also prunes `active_library_ids` of any deleted entries).
//! - `LoadFailed(err)` surfaces a toast.

use iced::{Rectangle, Task};
use tracing::{trace, warn};

use crate::{
    Nokkvi, View,
    app_message::{LibraryMessage, Message, OpenMenu},
};

impl Nokkvi {
    pub(crate) fn handle_library_message(&mut self, msg: LibraryMessage) -> Task<Message> {
        match msg {
            LibraryMessage::OpenChange {
                open,
                trigger_bounds,
            } => self.handle_library_open_change(open, trigger_bounds),
            LibraryMessage::Toggle(id) => self.handle_library_toggle(id),
            LibraryMessage::Loaded(libs) => self.handle_library_loaded(libs),
            LibraryMessage::LoadFailed(err) => {
                warn!(error = %err, "library list fetch failed");
                self.toast_error(format!("Failed to load libraries: {err}"));
                Task::none()
            }
        }
    }

    /// Open or close the LibrarySelector popover. When opening with a
    /// cold cache, fires a `refresh_libraries` task so the popover has
    /// rows to render by the time the user looks at it.
    fn handle_library_open_change(
        &mut self,
        open: bool,
        trigger_bounds: Option<Rectangle>,
    ) -> Task<Message> {
        if !open {
            self.open_menu = None;
            return Task::none();
        }

        // Anchor the overlay at the captured trigger bounds. If the
        // caller didn't pass any (shouldn't happen in normal UI flow,
        // but the type allows None), fall back to a zero rect so the
        // overlay still opens — the popover code handles a degenerate
        // rect by anchoring at (0, 0) rather than panicking.
        self.open_menu = Some(OpenMenu::LibrarySelector {
            trigger_bounds: trigger_bounds.unwrap_or(Rectangle {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
        });

        // If we already have a cached list, the popover renders
        // immediately — skip the refetch to avoid stomping the visible
        // rows mid-interaction.
        let needs_refresh = self
            .app_service
            .as_ref()
            .is_some_and(|s| s.all_libraries().is_empty());
        if !needs_refresh {
            return Task::none();
        }

        trace!("library list cache cold; refreshing on popover open");
        self.shell_task(
            |shell| async move { shell.refresh_libraries().await },
            |result: anyhow::Result<Vec<nokkvi_data::types::library::Library>>| match result {
                Ok(libs) => Message::Library(LibraryMessage::Loaded(libs)),
                Err(e) => Message::Library(LibraryMessage::LoadFailed(format!("{e:#}"))),
            },
        )
    }

    /// Flip membership of `id` in the active library set, invalidate
    /// every paged library buffer, and refetch the currently-visible
    /// view. Other views refetch lazily on next visit through the
    /// standard `LoadX` handlers checking `is_empty()` after the clear.
    fn handle_library_toggle(&mut self, id: i32) -> Task<Message> {
        if let Some(shell) = &self.app_service {
            let now_active = shell.toggle_library(id);
            trace!(
                id,
                now_active, "library toggled; invalidating paged buffers"
            );
        } else {
            // No shell yet (pre-login) — handler shouldn't ever fire in
            // this state because the trigger widget is hidden until
            // `library_count > 1`, but stay defensive.
            warn!("Library::Toggle dispatched without an active AppService; dropping");
            return Task::none();
        }

        // Invalidate every paged buffer so the next visit refetches
        // with the new active-library filter. `clear()` bumps each
        // buffer's generation so any in-flight fetch result discards
        // itself when it lands (paged_buffer.rs:158).
        self.library.albums.clear();
        self.library.artists.clear();
        self.library.songs.clear();
        self.library.genres.clear();
        self.library.playlists.clear();

        // Refetch the currently-visible view eagerly. Queue / Radios /
        // Settings have no library-scoped paged buffer, so they
        // contribute no task. The other views' `handle_load_*` build
        // tasks that pick up `active_library_ids_vec()` through their
        // `_with_libraries` plumbing.
        match self.current_view {
            View::Albums => self.handle_load_albums(false, None),
            View::Artists => self.handle_load_artists(false, None),
            View::Songs => self.handle_load_songs(false, None),
            View::Genres => self.handle_load_genres(),
            View::Playlists => self.handle_load_playlists(),
            View::Queue | View::Radios | View::Settings => Task::none(),
        }
    }

    /// Apply a freshly-fetched library list. The backend handles
    /// pruning: any `active_library_ids` entries no longer in the list
    /// (i.e. deleted libraries) are dropped and persisted.
    fn handle_library_loaded(
        &mut self,
        libs: Vec<nokkvi_data::types::library::Library>,
    ) -> Task<Message> {
        if let Some(shell) = &self.app_service {
            let count = libs.len();
            shell.apply_library_refresh(libs);
            trace!(count, "library list applied to backend cache");
        } else {
            warn!("Library::Loaded dispatched without an active AppService; dropping");
        }
        Task::none()
    }
}
