//! Tests for common view-action component update handlers.

use crate::{View, state::ActivePlaylistContext, test_helpers::*};

fn make_playlist_ctx() -> ActivePlaylistContext {
    ActivePlaylistContext {
        id: "pl_42".to_string(),
        name: "Sunday Set".to_string(),
        comment: "weekend rotation".to_string(),
        song_count: 12,
        duration_secs: 2730.0,
        public: true,
        updated: "2026-05-28T10:00:00Z".to_string(),
        smart: Some(false),
        readonly: None,
    }
}

// View Action Handlers (components.rs)
// ============================================================================

#[test]
fn handle_common_view_action_refresh_returns_task() {
    let app = test_app();

    let persist_fn = |_s, _m, _a| async { Ok(()) };

    let task = app.handle_common_view_action(
        crate::views::CommonViewAction::RefreshViewData,
        crate::app_message::Message::LoadAlbums,
        "albums",
        crate::widgets::view_header::SortMode::Name,
        true,
        persist_fn,
    );

    assert!(task.is_some(), "RefreshViewData should return a task");
}

#[test]
fn handle_common_view_action_navigate_and_search_returns_task() {
    let app = test_app();
    let persist_fn = |_s, _m, _a| async { Ok(()) };

    let task = app.handle_common_view_action(
        crate::views::CommonViewAction::NavigateAndFilter(
            View::Artists,
            nokkvi_data::types::filter::LibraryFilter::ArtistId {
                id: "Beatles".to_string(),
                name: "Beatles".to_string(),
            },
        ),
        crate::app_message::Message::LoadAlbums,
        "albums",
        crate::widgets::view_header::SortMode::Name,
        true,
        persist_fn,
    );

    assert!(
        task.is_some(),
        "NavigateAndFilter should be handled by common action handler"
    );
}

// ============================================================================
// guard_play_action / enter_new_playback_context
// ============================================================================
//
// Decomposed from a single helper after the 2026-05-12 regression where
// `QueueAction::PlaySong` cleared the loaded-playlist header. The guard now
// only handles the universal radio→queue transition;
// `enter_new_playback_context` carries the cleanup that must NOT run for
// in-queue plays.

#[test]
fn guard_play_action_preserves_active_playlist_info() {
    // Regression: clicking play on a song in the queue must keep the loaded
    // playlist header visible. The guard alone — which is all `PlaySong`
    // calls — must not touch `active_playlist_info`.
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());

    app.guard_play_action();

    assert!(
        app.active_playlist_info.is_some(),
        "guard alone must preserve the loaded-playlist header — \
         clearing belongs in enter_new_playback_context"
    );
}

#[test]
fn enter_new_playback_context_clears_active_playlist_info() {
    // Queue-replacing plays (album / artist / playlist / song / batch /
    // roulette) call this helper after the guard to drop the previous
    // playlist context.
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());
    app.library.start_progressive_queue_load(5);

    app.enter_new_playback_context();

    assert!(
        app.active_playlist_info.is_none(),
        "new-context entry must clear the playlist header"
    );
    assert!(
        app.library.queue_loading_total().is_none(),
        "new-context entry must cancel the in-progress queue load target"
    );
}

#[test]
fn clear_active_playlist_resets_strip_expansion() {
    // The read-only playlist strip's hover-expansion flag is transient: when
    // the active playlist clears, a stale expansion must not carry into the
    // next playlist (or render over an empty context).
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());
    app.queue_page.playlist_strip_expanded = true;

    app.clear_active_playlist();

    assert!(
        app.active_playlist_info.is_none(),
        "clear drops the playlist context"
    );
    assert!(
        !app.queue_page.playlist_strip_expanded,
        "clear must collapse the playlist strip"
    );
}

#[test]
fn guard_play_action_transitions_radio_to_queue() {
    let mut app = test_app();
    seed_radio_playback(&mut app);

    app.guard_play_action();

    assert!(
        app.active_playback.is_queue(),
        "active radio must transition back to queue mode"
    );
}

// ============================================================================
// Batch play owns the play prologue (play_batch_task + Similar)
// ============================================================================

/// Radio playing, a loaded-playlist header, and a stale progressive-load
/// target: everything a queue-replacing play must reset.
fn radio_app_with_stale_context() -> crate::Nokkvi {
    let mut app = test_app();
    seed_radio_playback(&mut app);
    app.active_playlist_info = Some(make_playlist_ctx());
    app.library.start_progressive_queue_load(5);
    app
}

fn assert_batch_play_entered_new_context(app: &crate::Nokkvi, path: &str) {
    assert!(
        app.active_playback.is_queue(),
        "{path}: a batch play must transition active radio back to queue mode"
    );
    assert!(
        app.active_playlist_info.is_none(),
        "{path}: a batch play replaces the queue, so the playlist header must go"
    );
    assert!(
        app.library.queue_loading_total().is_none(),
        "{path}: a batch play must clear the stale progressive-load count"
    );
}

#[test]
fn albums_multi_select_enter_batch_play_leaves_radio() {
    use crate::{views::AlbumsMessage, widgets::SlotListPageMessage};

    let mut app = radio_app_with_stale_context();
    seed_albums(
        &mut app,
        vec![
            make_album("a1", "Album 1", "Artist"),
            make_album("a2", "Album 2", "Artist"),
        ],
    );
    let selection = &mut app.albums_page.common.slot_list.selected_indices;
    selection.insert(0);
    selection.insert(1);

    let _ = app.update(crate::app_message::Message::Albums(
        AlbumsMessage::SlotList(SlotListPageMessage::ActivateCenter(false)),
    ));

    assert_batch_play_entered_new_context(&app, "Albums Enter on a multi-selection");
}

#[test]
fn albums_shuffle_play_batch_leaves_radio() {
    use crate::{views::AlbumsMessage, widgets::context_menu::LibraryContextEntry};

    let mut app = radio_app_with_stale_context();
    seed_albums(&mut app, vec![make_album("a1", "Album 1", "Artist")]);

    let _ = app.update(crate::app_message::Message::Albums(
        AlbumsMessage::ContextMenuAction(0, LibraryContextEntry::ShufflePlay),
    ));

    assert_batch_play_entered_new_context(&app, "Albums Shuffle Play");
}

#[test]
fn songs_shuffle_play_batch_leaves_radio() {
    use crate::{views::SongsMessage, widgets::context_menu::LibraryContextEntry};

    let mut app = radio_app_with_stale_context();
    seed_songs(&mut app, songs_indexed(2));

    let _ = app.update(crate::app_message::Message::Songs(
        SongsMessage::ContextMenuAction(0, LibraryContextEntry::ShufflePlay),
    ));

    assert_batch_play_entered_new_context(&app, "Songs Shuffle Play");
}

#[test]
fn artists_shuffle_play_batch_leaves_radio() {
    use crate::{views::ArtistsMessage, widgets::context_menu::LibraryContextEntry};

    let mut app = radio_app_with_stale_context();
    seed_artists(&mut app, vec![make_artist("ar1", "Artist 1")]);

    let _ = app.update(crate::app_message::Message::Artists(
        ArtistsMessage::ContextMenuAction(0, LibraryContextEntry::ShufflePlay),
    ));

    assert_batch_play_entered_new_context(&app, "Artists Shuffle Play");
}

#[test]
fn genres_shuffle_play_batch_leaves_radio() {
    use crate::{views::GenresMessage, widgets::context_menu::LibraryContextEntry};

    let mut app = radio_app_with_stale_context();
    seed_genres(&mut app, vec![make_genre("g1", "Ambient")]);

    let _ = app.update(crate::app_message::Message::Genres(
        GenresMessage::ContextMenuAction(0, LibraryContextEntry::ShufflePlay),
    ));

    assert_batch_play_entered_new_context(&app, "Genres Shuffle Play");
}

#[test]
fn playlists_shuffle_play_batch_leaves_radio() {
    use crate::{views::PlaylistsMessage, widgets::context_menu::LibraryContextEntry};

    let mut app = radio_app_with_stale_context();
    app.library
        .playlists
        .set_from_vec(vec![make_test_playlist("p1", "Playlist 1")]);

    let _ = app.update(crate::app_message::Message::Playlists(
        PlaylistsMessage::ContextMenuAction(0, LibraryContextEntry::ShufflePlay),
    ));

    assert_batch_play_entered_new_context(&app, "Playlists Shuffle Play");
}

#[test]
fn similar_replace_queue_with_all_found_leaves_radio() {
    use crate::{views::SimilarMessage, widgets::context_menu::LibraryContextEntry};

    let mut app = radio_app_with_stale_context();
    app.similar_songs = Some(crate::state::SimilarSongsState {
        songs: vec![make_song("s1", "Song 1", "Artist").into()],
        source: crate::state::SimilarSource::SimilarTo("Song 0".into()),
        loading: false,
    });

    let _ = app.update(crate::app_message::Message::Similar(
        SimilarMessage::ContextMenuAction(0, LibraryContextEntry::ReplaceQueueWithAllFound),
    ));

    assert_batch_play_entered_new_context(&app, "Similar Replace Queue With All Found");
}

// ============================================================================
// A failed queue play hands radio mode back while the station still streams
// ============================================================================

fn station_failure(app: &mut crate::Nokkvi, attempt: u64) {
    let _ = app.update(crate::app_message::Message::Playback(
        crate::app_message::PlaybackMessage::QueuePlayFailedOnStation { attempt },
    ));
}

#[test]
fn guard_remembers_the_station_it_leaves() {
    let mut app = test_app();
    seed_radio_playback(&mut app);
    let before = app.playback.play_attempt;

    app.guard_play_action();

    assert_eq!(app.playback.play_attempt, before + 1);
    assert_eq!(
        app.playback
            .station_left_for_play
            .as_ref()
            .map(|r| r.station.id.as_str()),
        Some("r1")
    );
}

#[test]
fn failed_batch_play_on_a_live_station_restores_radio() {
    let mut app = test_app();
    seed_radio_playback(&mut app);
    let _ = app.play_batch_task(nokkvi_data::types::batch::BatchPayload::new(), false);
    assert!(
        app.active_playback.is_queue(),
        "setup: the play left radio mode"
    );

    let attempt = app.playback.play_attempt;
    station_failure(&mut app, attempt);

    assert!(
        app.active_playback.is_radio(),
        "the play failed and the station never stopped, so radio mode comes back"
    );
}

#[test]
fn failed_play_restores_nothing_after_a_newer_play() {
    // A newer play is in flight; its outcome, not the stale failure, decides.
    let mut app = test_app();
    seed_radio_playback(&mut app);
    let _ = app.play_batch_task(nokkvi_data::types::batch::BatchPayload::new(), false);
    let failed_attempt = app.playback.play_attempt;
    app.guard_play_action();

    station_failure(&mut app, failed_attempt);

    assert!(app.active_playback.is_queue());
}

#[test]
fn failed_play_keeps_a_station_started_since() {
    let mut app = test_app();
    seed_radio_playback(&mut app);
    let _ = app.play_batch_task(nokkvi_data::types::batch::BatchPayload::new(), false);
    let attempt = app.playback.play_attempt;
    let mut other = app
        .playback
        .station_left_for_play
        .clone()
        .expect("setup: the guard remembered the station");
    other.station.id = "r2".into();
    app.active_playback = crate::state::ActivePlayback::Radio(other);

    station_failure(&mut app, attempt);

    assert_eq!(
        app.active_playback.radio_station().map(|s| s.id.as_str()),
        Some("r2"),
        "the station the user switched to stays"
    );
}

#[test]
fn queue_play_failure_hands_radio_back_only_when_the_station_is_still_on() {
    use crate::app_message::{Message, PlaybackMessage, ToastMessage};

    let e = anyhow::anyhow!("nothing to play");
    match crate::Nokkvi::queue_play_failure_message(&e, "Failed to play batch", Some(7)) {
        Message::Toast(ToastMessage::PushThen(toast, next)) => {
            assert_eq!(toast.message, "Failed to play batch: nothing to play");
            assert!(matches!(
                *next,
                Message::Playback(PlaybackMessage::QueuePlayFailedOnStation { attempt: 7 })
            ));
        }
        other => panic!("expected the error toast, then the radio hand-back: {other:?}"),
    }
    assert!(matches!(
        crate::Nokkvi::queue_play_failure_message(&e, "Failed to play batch", None),
        Message::Toast(ToastMessage::Push(_))
    ));
}

// ============================================================================
// Queue replacements stop a running Songs progressive load
// ============================================================================

/// A Songs "play all" whose remaining pages are still being appended.
fn app_with_running_songs_load() -> (crate::Nokkvi, u64) {
    let mut app = test_app();
    let chain = app.library.start_progressive_queue_load(500);
    (app, chain)
}

#[test]
fn batch_play_stops_a_running_songs_load() {
    use crate::{views::AlbumsMessage, widgets::context_menu::LibraryContextEntry};

    let (mut app, chain) = app_with_running_songs_load();
    seed_albums(&mut app, vec![make_album("a1", "Album 1", "Artist")]);

    let _ = app.update(crate::app_message::Message::Albums(
        AlbumsMessage::ContextMenuAction(0, LibraryContextEntry::ShufflePlay),
    ));

    assert!(
        !app.library.progressive_queue_generation.is_current(chain),
        "the album replaced the queue, so the Songs pages must stop appending"
    );
}

#[test]
fn clear_queue_stops_a_running_songs_load() {
    let (mut app, chain) = app_with_running_songs_load();

    let _ = app.clear_queue_action();

    assert!(!app.library.progressive_queue_generation.is_current(chain));
    assert!(app.library.queue_loading_total().is_none());
}

#[test]
fn starting_a_queue_pull_keeps_the_songs_load_running() {
    // A pull with no saved queue, or a failed one, replaces nothing; the
    // pull stops the load inside AppService::pull_queue, right before it
    // replaces the queue, and only then.
    let (mut app, chain) = app_with_running_songs_load();

    let _ = app.pull_queue_task();

    assert!(app.library.progressive_queue_generation.is_current(chain));
    assert_eq!(app.library.queue_loading_total(), Some(500));
}

#[test]
fn a_stopped_chains_count_leaves_the_header() {
    // A pull stops the chain from its async task with a bare bump: nothing
    // clears the target, so the header must stop showing it on its own.
    let (app, _chain) = app_with_running_songs_load();

    let _ = app.library.progressive_queue_generation.bump();

    assert_eq!(app.library.queue_loading_total(), None);
}

#[test]
fn genre_roulette_stops_a_running_songs_load() {
    let (mut app, chain) = app_with_running_songs_load();
    seed_genres(&mut app, vec![make_genre("g1", "Ambient")]);

    let _ = app.roulette_settle_play(View::Genres, 0, 1);

    assert!(!app.library.progressive_queue_generation.is_current(chain));
    assert!(app.library.queue_loading_total().is_none());
}

#[test]
fn artist_roulette_stops_a_running_songs_load() {
    let (mut app, chain) = app_with_running_songs_load();
    seed_artists(&mut app, vec![make_artist("ar1", "Artist 1")]);

    let _ = app.roulette_settle_play(View::Artists, 0, 1);

    assert!(!app.library.progressive_queue_generation.is_current(chain));
}

#[test]
fn in_queue_play_keeps_the_songs_load_running() {
    // Playing a row of the queue only moves the cursor: the chain is still
    // filling this same queue and must keep going.
    use crate::{views::QueueMessage, widgets::SlotListPageMessage};

    let (mut app, chain) = app_with_running_songs_load();
    app.library.queue_songs = vec![make_queue_song("s1", "One", "Artist", "Album")];

    let _ = app.update(crate::app_message::Message::Queue(QueueMessage::SlotList(
        SlotListPageMessage::ActivateCenter(false),
    )));

    assert!(app.library.progressive_queue_generation.is_current(chain));
    assert_eq!(app.library.queue_loading_total(), Some(500));
}

/// Enter set to Append and Play, a running Songs load, a station on air.
fn append_and_play_app() -> (crate::Nokkvi, u64) {
    let (mut app, chain) = app_with_running_songs_load();
    app.settings.enter_behavior = nokkvi_data::types::player_settings::EnterBehavior::AppendAndPlay;
    seed_radio_playback(&mut app);
    (app, chain)
}

fn assert_append_kept_the_load(app: &crate::Nokkvi, chain: u64, path: &str) {
    assert!(
        app.library.progressive_queue_generation.is_current(chain),
        "{path}: Append and Play only appends, so the Songs load must keep going"
    );
    assert_eq!(app.library.queue_loading_total(), Some(500), "{path}");
    assert!(
        app.active_playback.is_queue(),
        "{path}: Append and Play starts queue playback, so it leaves radio mode"
    );
}

#[test]
fn append_and_play_keeps_the_songs_load_running_in_every_view() {
    use crate::{
        app_message::Message,
        views::{AlbumsMessage, ArtistsMessage, GenresMessage, PlaylistsMessage, SongsMessage},
        widgets::SlotListPageMessage::ActivateCenter,
    };

    let (mut app, chain) = append_and_play_app();
    seed_albums(&mut app, vec![make_album("a1", "Album 1", "Artist")]);
    let _ = app.update(Message::Albums(AlbumsMessage::SlotList(ActivateCenter(
        false,
    ))));
    assert_append_kept_the_load(&app, chain, "Albums");

    let (mut app, chain) = append_and_play_app();
    seed_artists(&mut app, vec![make_artist("ar1", "Artist 1")]);
    let _ = app.update(Message::Artists(ArtistsMessage::SlotList(ActivateCenter(
        false,
    ))));
    assert_append_kept_the_load(&app, chain, "Artists");

    let (mut app, chain) = append_and_play_app();
    seed_genres(&mut app, vec![make_genre("g1", "Ambient")]);
    let _ = app.update(Message::Genres(GenresMessage::SlotList(ActivateCenter(
        false,
    ))));
    assert_append_kept_the_load(&app, chain, "Genres");

    let (mut app, chain) = append_and_play_app();
    app.library
        .playlists
        .set_from_vec(vec![make_test_playlist("p1", "Playlist 1")]);
    let _ = app.update(Message::Playlists(PlaylistsMessage::SlotList(
        ActivateCenter(false),
    )));
    assert_append_kept_the_load(&app, chain, "Playlists");

    let (mut app, chain) = append_and_play_app();
    seed_songs(&mut app, songs_indexed(1));
    let _ = app.update(Message::Songs(SongsMessage::SlotList(ActivateCenter(
        false,
    ))));
    assert_append_kept_the_load(&app, chain, "Songs");
}

#[test]
fn stale_chain_done_leaves_the_newer_loads_count() {
    let (mut app, stale_chain) = app_with_running_songs_load();
    let _newer_chain = app.library.start_progressive_queue_load(50);

    let _ = app.update(crate::app_message::Message::ProgressiveQueueDone {
        generation: stale_chain,
    });

    assert_eq!(
        app.library.queue_loading_total(),
        Some(50),
        "a superseded chain finishing must not clear the running chain's count"
    );
}

#[test]
fn logout_stops_a_running_songs_load() {
    // The chain's in-flight task holds a clone of the counter; a fresh
    // counter would leave that clone current and let the page append.
    let _sse = super::SSE_SLOT_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (mut app, chain) = app_with_running_songs_load();
    let task_copy = app.library.progressive_queue_generation.clone();

    let _ = app.reset_session_state();

    assert!(!task_copy.is_current(chain));
    let next_chain = app.library.progressive_queue_generation.bump();
    assert!(
        task_copy.is_current(next_chain) && next_chain > chain,
        "the new session keeps counting on the same counter, never reusing a generation"
    );
}

// ============================================================================
// Browsing-panel Enter adds to the queue: radio and the header stay put
// ============================================================================

/// Split view open, a station streaming, a playlist header on the queue.
fn radio_app_with_browsing_panel() -> crate::Nokkvi {
    let mut app = test_app();
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    seed_radio_playback(&mut app);
    app.active_playlist_info = Some(make_playlist_ctx());
    app
}

fn assert_add_left_playback_alone(app: &crate::Nokkvi, path: &str) {
    assert!(
        app.active_playback.is_radio(),
        "{path}: adding to the queue must not leave radio mode while the station plays"
    );
    assert!(
        app.active_playlist_info.is_some(),
        "{path}: adding to the queue must keep the playlist header, like Add to Queue does"
    );
}

fn activate_center(msg: crate::app_message::Message, app: &mut crate::Nokkvi) {
    let _ = app.update(msg);
}

#[test]
fn browsing_panel_albums_enter_adds_without_leaving_radio() {
    use crate::{views::AlbumsMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    seed_albums(&mut app, vec![make_album("a1", "Album 1", "Artist")]);
    activate_center(
        crate::app_message::Message::Albums(AlbumsMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Albums");
}

#[test]
fn browsing_panel_artists_enter_adds_without_leaving_radio() {
    use crate::{views::ArtistsMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    seed_artists(&mut app, vec![make_artist("ar1", "Artist 1")]);
    activate_center(
        crate::app_message::Message::Artists(ArtistsMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Artists");
}

#[test]
fn browsing_panel_genres_enter_adds_without_leaving_radio() {
    use crate::{views::GenresMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    seed_genres(&mut app, vec![make_genre("g1", "Ambient")]);
    activate_center(
        crate::app_message::Message::Genres(GenresMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Genres");
}

#[test]
fn browsing_panel_playlists_enter_adds_without_leaving_radio() {
    use crate::{views::PlaylistsMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    app.library
        .playlists
        .set_from_vec(vec![make_test_playlist("p1", "Playlist 1")]);
    activate_center(
        crate::app_message::Message::Playlists(PlaylistsMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Playlists");
}

#[test]
fn browsing_panel_songs_enter_adds_without_leaving_radio() {
    use crate::{views::SongsMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    seed_songs(&mut app, songs_indexed(1));
    activate_center(
        crate::app_message::Message::Songs(SongsMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Songs");
}

#[test]
fn browsing_panel_album_track_enter_adds_without_leaving_radio() {
    use crate::{views::AlbumsMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    seed_albums(&mut app, vec![make_album("a1", "Album 1", "Artist")]);
    expand_albums_with(&mut app, "a1", vec![make_song("t1", "Track", "Artist")]);
    app.albums_page.common.slot_list.selected_offset = Some(1);
    activate_center(
        crate::app_message::Message::Albums(AlbumsMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Albums track");
}

#[test]
fn browsing_panel_artist_album_enter_adds_without_leaving_radio() {
    use crate::{views::ArtistsMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    seed_artists(&mut app, vec![make_artist("ar1", "Artist 1")]);
    expand_artists_with(
        &mut app,
        "ar1",
        vec![make_album("a1", "Album 1", "Artist 1")],
    );
    app.artists_page.common.slot_list.selected_offset = Some(1);
    activate_center(
        crate::app_message::Message::Artists(ArtistsMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Artists album");
}

#[test]
fn browsing_panel_genre_album_enter_adds_without_leaving_radio() {
    use crate::{views::GenresMessage, widgets::SlotListPageMessage};
    let mut app = radio_app_with_browsing_panel();
    seed_genres(&mut app, vec![make_genre("g1", "Ambient")]);
    expand_genres_with(&mut app, "g1", vec![make_album("a1", "Album 1", "Artist")]);
    app.genres_page.common.slot_list.selected_offset = Some(1);
    activate_center(
        crate::app_message::Message::Genres(GenresMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false),
        )),
        &mut app,
    );
    assert_add_left_playback_alone(&app, "Genres album");
}

// ============================================================================
// redirect_play_to_queue_in_browsing_panel (components.rs)
// ============================================================================

#[test]
fn redirect_play_returns_none_when_browsing_panel_closed() {
    // Single-pane play actions must NOT be redirected — the caller proceeds
    // to its normal "replace queue" play flow.
    let mut app = test_app();
    assert!(app.browsing_panel.is_none());

    let mut add_fired = false;
    let mut insert_fired = false;
    let task = app.redirect_play_to_queue_in_browsing_panel(
        |_app| {
            add_fired = true;
            iced::Task::none()
        },
        |_app, _pos| {
            insert_fired = true;
            iced::Task::none()
        },
    );

    assert!(task.is_none(), "no redirect without browsing panel");
    assert!(!add_fired, "add closure must not fire when closed");
    assert!(!insert_fired, "insert closure must not fire when closed");
}

#[test]
fn redirect_play_invokes_add_when_no_pending_insert() {
    // Browsing panel open, no drag-drop position → append branch.
    let mut app = test_app();
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    app.cross_pane_drag.pending_queue_insert_position = None;

    let mut add_fired = false;
    let mut insert_fired = false;
    let task = app.redirect_play_to_queue_in_browsing_panel(
        |_app| {
            add_fired = true;
            iced::Task::none()
        },
        |_app, _pos| {
            insert_fired = true;
            iced::Task::none()
        },
    );

    assert!(task.is_some(), "browsing-panel redirect must return Some");
    assert!(
        add_fired,
        "add closure must run when no insert position pending"
    );
    assert!(!insert_fired, "insert closure must NOT run");
}

// ============================================================================
// find_current_rating (components.rs)
// ============================================================================

#[test]
fn find_current_rating_returns_zero_on_miss() {
    // Optimistic rating updates need the prior value to revert on API
    // failure. When the id isn't in the slice, the helper must return 0
    // (the prior contract before the lookup was lifted from 4 inline sites).
    use crate::Nokkvi;

    let mut song = make_song("s1", "Song 1", "Artist");
    song.rating = Some(4);
    let items = vec![song];

    let rating = Nokkvi::find_current_rating(&items, "missing-id", |s| s.id.as_str(), |s| s.rating);

    assert_eq!(
        rating, 0,
        "miss must default to 0 — the inline `.unwrap_or(0)` contract"
    );
}

#[test]
fn find_current_rating_returns_rating_on_hit() {
    use crate::Nokkvi;

    let mut song = make_song("s1", "Song 1", "Artist");
    song.rating = Some(4);
    let items = vec![song, make_song("s2", "Song 2", "Artist")];

    let rating = Nokkvi::find_current_rating(&items, "s1", |s| s.id.as_str(), |s| s.rating);

    assert_eq!(rating, 4, "hit must return the item's rating");
}

#[test]
fn find_current_rating_unrated_item_returns_zero() {
    // Item is found but its rating is None → still 0 (the `.and_then` →
    // `.unwrap_or(0)` chain in the original).
    use crate::Nokkvi;

    let song = make_song("s1", "Song 1", "Artist"); // rating defaults to None
    let items = vec![song];

    let rating = Nokkvi::find_current_rating(&items, "s1", |s| s.id.as_str(), |s| s.rating);

    assert_eq!(rating, 0, "unrated item must yield 0 — same as miss");
}

// ============================================================================
// play_batch_task (components.rs)
// ============================================================================

#[test]
fn play_batch_task_clears_active_playlist() {
    // play_batch_task replaces the queue → the loaded-playlist header must
    // be cleared so it doesn't outlive the playlist it was named after.
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());

    let payload = nokkvi_data::types::batch::BatchPayload::new().with_item(
        nokkvi_data::types::batch::BatchItem::Album("a1".to_string()),
    );
    let _task = app.play_batch_task(payload, false);

    assert!(
        app.active_playlist_info.is_none(),
        "play_batch_task must clear active_playlist_info — the queue is being replaced"
    );
}

/// The plain-activate (Enter/click) shuffle directive resolves from the
/// `enter_shuffle` setting, with `force_shuffle` (Ctrl+Enter) overriding it and
/// `anchored` selecting AnchorFirst. This is the observable seam the play
/// handlers consult (the wrapper directive itself is closure-buried).
#[test]
fn activate_shuffle_directive_resolves_from_setting_and_force() {
    use nokkvi_data::types::OneShotShuffle;

    let mut app = test_app();

    // Setting off, no force → linear.
    app.settings.enter_shuffle = false;
    assert_eq!(
        app.activate_shuffle_directive(false, false),
        OneShotShuffle::None
    );
    assert_eq!(
        app.activate_shuffle_directive(false, true),
        OneShotShuffle::None
    );

    // Setting off, force (Ctrl+Enter) → always Full, even on a clicked track
    // (explicit surfaces are unanchored — Q4).
    assert_eq!(
        app.activate_shuffle_directive(true, false),
        OneShotShuffle::Full
    );
    assert_eq!(
        app.activate_shuffle_directive(true, true),
        OneShotShuffle::Full
    );

    // Setting on → plain activate shuffles.
    app.settings.enter_shuffle = true;
    assert_eq!(
        app.activate_shuffle_directive(false, false),
        OneShotShuffle::Full
    );
    assert_eq!(
        app.activate_shuffle_directive(false, true),
        OneShotShuffle::AnchorFirst
    );
}

#[test]
fn redirect_play_invokes_insert_and_consumes_position() {
    // Browsing panel open with a drag-drop position → insert branch, AND
    // the position is consumed via `take()` so the next play sees None.
    let mut app = test_app();
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    app.cross_pane_drag.pending_queue_insert_position = Some(3);

    let mut add_fired = false;
    let mut received_pos: Option<usize> = None;
    let task = app.redirect_play_to_queue_in_browsing_panel(
        |_app| {
            add_fired = true;
            iced::Task::none()
        },
        |_app, pos| {
            received_pos = Some(pos);
            iced::Task::none()
        },
    );

    assert!(task.is_some(), "browsing-panel redirect must return Some");
    assert!(!add_fired, "add closure must NOT run");
    assert_eq!(
        received_pos,
        Some(3),
        "insert closure receives the position"
    );
    assert!(
        app.cross_pane_drag.pending_queue_insert_position.is_none(),
        "pending_queue_insert_position must be consumed via take()"
    );
}

// ============================================================================
// PlayPlaylistFromTrack prologue (I9 — update/playlists.rs)
// ============================================================================
//
// The from-track play arm must run the same prologue as its album sibling
// (`AlbumsAction::PlayAlbumFromTrack`): `guard_play_action()` (radio→queue
// transition) then `enter_new_playback_context()` (clears a
// stale `queue_loading_target`), BEFORE it sets `active_playlist_info`.

fn make_test_playlist(id: &str, name: &str) -> nokkvi_data::backend::playlists::PlaylistUIViewData {
    nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        comment: String::new(),
        duration: 0.0,
        song_count: 1,
        owner_name: String::new(),
        public: false,
        updated_at: String::new(),
        artwork_album_ids: vec![],
        uploaded_image: None,
        is_smart: false,
        rules: None,
        evaluated_at: None,
        is_file_backed: false,
        sync: false,
        owner_id: String::new(),
        searchable_lower: name.to_lowercase(),
        image: Default::default(),
    }
}

/// Seed one playlist `p1` expanded with one child track and center the
/// playlists slot list on the child so `ActivateCenter` resolves to
/// `PlayPlaylistFromTrack("p1", 0)`. Flattened layout is `[p1=0, t1=1]`;
/// `selected_offset = Some(1)` forces the effective center onto the child.
fn seed_expanded_playlist_centered_on_child(app: &mut crate::Nokkvi) {
    app.library
        .playlists
        .append_page(vec![make_test_playlist("p1", "Playlist One")], 1);
    expand_playlists_with(app, "p1", vec![make_song("t1", "Track One", "Artist")]);
    app.playlists_page.common.slot_list.selected_offset = Some(1);
}

fn activate_center_on_playlists(app: &mut crate::Nokkvi) {
    use crate::{views::PlaylistsMessage, widgets::SlotListPageMessage};
    let _ = app.update(crate::app_message::Message::Playlists(
        PlaylistsMessage::SlotList(SlotListPageMessage::ActivateCenter(false)),
    ));
}

#[test]
fn play_playlist_from_track_transitions_radio_to_queue() {
    use crate::state::{ActivePlayback, RadioPlaybackState};

    let mut app = test_app();
    seed_expanded_playlist_centered_on_child(&mut app);
    app.active_playback = ActivePlayback::Radio(RadioPlaybackState {
        station: nokkvi_data::types::radio_station::RadioStation {
            id: "r1".into(),
            name: "Test".into(),
            stream_url: "http://example.invalid/stream".into(),
            home_page_url: None,
            cover_art: None,
        },
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });

    activate_center_on_playlists(&mut app);

    assert!(
        app.active_playback.is_queue(),
        "playing a playlist from a track must transition active radio back to queue mode \
         (guard_play_action prologue)"
    );
}

#[test]
fn play_playlist_from_track_clears_stale_queue_loading_target() {
    let mut app = test_app();
    seed_expanded_playlist_centered_on_child(&mut app);
    app.library.start_progressive_queue_load(5);

    activate_center_on_playlists(&mut app);

    assert!(
        app.library.queue_loading_total().is_none(),
        "playing a playlist from a track must clear the stale queue_loading_target \
         (enter_new_playback_context prologue)"
    );
}

#[test]
fn play_playlist_from_track_during_edit_proceeds_and_sets_banner() {
    use nokkvi_data::types::playlist_edit::PlaylistEditState;

    use crate::state::PlaylistEditorState;

    let mut app = test_app();
    seed_expanded_playlist_centered_on_child(&mut app);
    app.active_playlist_info = Some(make_playlist_ctx());
    app.playlist_editor = Some(PlaylistEditorState::new(PlaylistEditState::new(
        "pl_42".into(),
        "Sunday Set".into(),
        String::new(),
        false,
        Vec::new(),
    )));

    activate_center_on_playlists(&mut app);

    // The editor's buffer is decoupled from the queue, so a play is no longer
    // blocked during an edit — it proceeds and re-points the banner at the played
    // playlist, exactly like the no-edit case.
    assert_eq!(
        app.active_playlist_info.as_ref().map(|ctx| ctx.id.as_str()),
        Some("p1"),
        "an edit-mode from-track play now proceeds and sets the banner to the played playlist"
    );
}

#[test]
fn play_playlist_from_track_sets_active_playlist_info_when_unblocked() {
    // Regression guard on the prologue ordering: enter_new_playback_context()
    // NULLs active_playlist_info, so it MUST run before the set+persist. A
    // normal (no-radio / no-edit) play must end with the banner pointing at the
    // played playlist — reversing the order would leave it None.
    let mut app = test_app();
    seed_expanded_playlist_centered_on_child(&mut app);

    activate_center_on_playlists(&mut app);

    assert_eq!(
        app.active_playlist_info.as_ref().map(|ctx| ctx.id.as_str()),
        Some("p1"),
        "an unblocked from-track play must set the banner to the played playlist \
         (set+persist runs after enter_new_playback_context)"
    );
}
