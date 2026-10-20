//! Similar Artists modal handler: open it for an artist from a row's menu,
//! fill it when the similar-artists list lands, act on a row.
//!
//! The list comes from the same cache the Artists panel fills
//! (`Nokkvi.artist_recs`, asked through `artist_recs_task`), so an artist
//! the panel already showed opens ready, and one the modal asked about
//! shows in the panel later. ListenBrainz's ask needs the artist's
//! MusicBrainz id, which the `getArtistInfo2` fetch supplies for a queue row
//! (`artist_info_task`; `handle_artist_info_loaded` resumes the recs ask when
//! that lands); Deezer's goes by the artist name the modal holds.

use iced::Task;
use nokkvi_data::types::{artist_recs::LibraryMatch, filter::LibraryFilter};

use crate::{
    Nokkvi, View,
    app_message::{Message, NavigationMessage},
    state::PendingExpand,
    widgets::similar_artists_modal::{
        SIMILAR_ARTISTS_SEARCH_INPUT_ID, SimilarArtistsModalMessage, SimilarArtistsModalState,
        SimilarArtistsStatus,
    },
};

impl Nokkvi {
    pub(crate) fn handle_similar_artists_modal(
        &mut self,
        msg: SimilarArtistsModalMessage,
    ) -> Task<Message> {
        match msg {
            SimilarArtistsModalMessage::Open(artist) => {
                self.open_similar_artists_modal(artist.id, artist.name)
            }
            SimilarArtistsModalMessage::Close => {
                self.similar_artists_modal = None;
                Task::none()
            }
            SimilarArtistsModalMessage::SearchChanged(query) => {
                if let Some(state) = self.similar_artists_modal.as_mut() {
                    state.list.search_query = query;
                    state.refilter();
                }
                Task::none()
            }
            SimilarArtistsModalMessage::SlotListUp => {
                if let Some(state) = self.similar_artists_modal.as_mut() {
                    state.list.move_up();
                }
                Task::none()
            }
            SimilarArtistsModalMessage::SlotListDown => {
                if let Some(state) = self.similar_artists_modal.as_mut() {
                    state.list.move_down();
                }
                Task::none()
            }
            SimilarArtistsModalMessage::SlotListSetOffset(offset) => {
                if let Some(state) = self.similar_artists_modal.as_mut() {
                    state.list.set_offset(offset);
                }
                Task::none()
            }
            SimilarArtistsModalMessage::ClickItem(index) => {
                if let Some(state) = self.similar_artists_modal.as_mut() {
                    state.list.set_offset(index);
                }
                self.act_on_similar_artist(index)
            }
            SimilarArtistsModalMessage::ActivateCenter => {
                let centered = self.similar_artists_modal.as_ref().and_then(|state| {
                    state
                        .list
                        .slot_list
                        .get_effective_center_index(state.list.filtered.len())
                });
                match centered {
                    Some(index) => self.act_on_similar_artist(index),
                    None => Task::none(),
                }
            }
            SimilarArtistsModalMessage::OpenArtistPage(index) => {
                if let Some(rec) = self
                    .similar_artists_modal
                    .as_ref()
                    .and_then(|state| state.list.filtered.get(index))
                {
                    super::components::open_external_url(&rec.page_url());
                }
                refocus_search()
            }
        }
    }

    /// The Similar Artists key: open the modal for the focused row's artist,
    /// or close it when it is the modal on top (the key gate admits its own
    /// key there, as Get Info's does).
    pub(crate) fn handle_similar_artists_hotkey(&mut self) -> Task<Message> {
        if self.modal_is_top(crate::update::modals::ActiveModal::SimilarArtists) {
            self.similar_artists_modal = None;
            return Task::none();
        }
        match self.focused_artist() {
            Some(artist) => self.open_similar_artists_modal(artist.id, artist.name),
            None => {
                self.toast_warn(NO_ARTIST_TOAST);
                Task::none()
            }
        }
    }

    /// Open the modal for `artist_id` (a row's menu). A cached list shows at
    /// once; otherwise the artist's info and list are asked for, and the
    /// modal fills when they land. With both sources off nothing is asked
    /// and the modal says so.
    pub(crate) fn open_similar_artists_modal(
        &mut self,
        artist_id: String,
        artist_name: String,
    ) -> Task<Message> {
        if artist_id.is_empty() {
            self.toast_warn(NO_ARTIST_TOAST);
            return Task::none();
        }
        let status = if self.rec_sources().any() {
            SimilarArtistsStatus::Loading
        } else {
            SimilarArtistsStatus::Off
        };
        let mut state = SimilarArtistsModalState::new(artist_id.clone(), artist_name, status);
        let cached = self.artist_recs.cache.peek(&artist_id).cloned();
        let ask = cached.is_none() && status == SimilarArtistsStatus::Loading;
        if let Some(recs) = cached
            && status == SimilarArtistsStatus::Loading
        {
            self.fill_similar_artists(&mut state, recs);
        }
        // In place before the ask: Deezer's goes by the name it holds.
        self.similar_artists_modal = Some(state);
        let fetch = if ask {
            Task::batch([
                self.artist_info_task(artist_id.clone()),
                self.artist_recs_task(artist_id),
            ])
        } else {
            Task::none()
        };
        Task::batch([fetch, refocus_search()])
    }

    /// Show `recs` in `state`; an empty list with no MusicBrainz id known
    /// and Deezer off reads as "cannot be asked", not as "none found".
    fn fill_similar_artists(
        &self,
        state: &mut SimilarArtistsModalState,
        recs: Vec<nokkvi_data::types::artist_recs::ArtistRec>,
    ) {
        let no_mbid = recs.is_empty()
            && !self.rec_sources().deezer
            && self.artist_mbid(&state.artist_id).is_none();
        state.set_recs(recs);
        if no_mbid {
            state.status = SimilarArtistsStatus::NoMbid;
        }
    }

    /// The list for `artist_id` landed (`landed == false`: its ask failed):
    /// the modal open for that artist, if any, follows.
    pub(crate) fn sync_similar_artists_modal(&mut self, artist_id: &str, landed: bool) {
        if self
            .similar_artists_modal
            .as_ref()
            .is_none_or(|state| state.artist_id != artist_id)
        {
            return;
        }
        if !landed {
            if let Some(state) = self.similar_artists_modal.as_mut() {
                state.status = SimilarArtistsStatus::Failed;
            }
            return;
        }
        let Some(recs) = self.artist_recs.cache.peek(&artist_id.to_string()).cloned() else {
            return;
        };
        let Some(mut state) = self.similar_artists_modal.take() else {
            return;
        };
        self.fill_similar_artists(&mut state, recs);
        self.similar_artists_modal = Some(state);
    }

    /// A row pressed (or Enter on the centered one): an artist the library
    /// has is found and expanded in Artists, or, when they only appear on
    /// other artists' albums, their songs are shown in Songs; the modal
    /// closes for either. Anyone else opens their page on the service that
    /// listed them and the modal stays.
    fn act_on_similar_artist(&mut self, index: usize) -> Task<Message> {
        let Some(rec) = self
            .similar_artists_modal
            .as_ref()
            .and_then(|state| state.list.filtered.get(index))
            .cloned()
        else {
            return Task::none();
        };
        match rec.library {
            LibraryMatch::Found(artist) => {
                self.similar_artists_modal = None;
                let for_browsing_pane =
                    self.browsing_panel.is_some() && self.current_view == View::Queue;
                let navigation = if artist.album_artist {
                    NavigationMessage::Expand(PendingExpand::Artist {
                        artist_id: artist.id,
                        for_browsing_pane,
                    })
                } else {
                    NavigationMessage::NavigateAndFilter {
                        view: View::Songs,
                        filter: LibraryFilter::ArtistId {
                            id: artist.id,
                            name: rec.name,
                        },
                        for_browsing_pane,
                    }
                };
                Task::done(Message::Navigation(navigation))
            }
            LibraryMatch::Missing | LibraryMatch::Unknown => {
                super::components::open_external_url(&rec.page_url());
                refocus_search()
            }
        }
    }
}

/// What the Similar Artists entry and key say about a row that names no
/// library artist (a playlist or genre, a radio station, a song the server
/// sent without an artist id).
pub(crate) const NO_ARTIST_TOAST: &str = "No artist to look up on this row";

/// Keep typing in the search field: a press on a row's globe takes focus
/// off it, and Backspace would then step the list.
fn refocus_search() -> Task<Message> {
    crate::widgets::focus_text_input(SIMILAR_ARTISTS_SEARCH_INPUT_ID)
}
