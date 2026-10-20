//! Similar Artists modal: the artists ListenBrainz and Deezer find similar to
//! one in the library, opened from a row's menu (the queue's), so the list is
//! reachable from where the music plays and not only from the Artists
//! view's panel. It reads the same list and library check as that panel
//! (`Nokkvi.artist_recs`); this is their on-demand, keyboard-reachable face.
//!
//! The list state and modal chrome are the shared picker shell
//! (`widgets::picker_modal`). A row names the artist, MusicBrainz's note on
//! them, where they stand against the library and who listed them; pressing
//! it finds the artist in the library or opens their page on the service that
//! listed them, and the globe at its end opens that page either way.
//!
//! State lives on `Nokkvi.similar_artists_modal`; the handler is
//! `update/similar_artists_modal.rs`.

use iced::{
    Alignment, Border, Element, Length, Widget as _,
    font::Weight,
    widget::{Space, button, column, container, mouse_area, row, svg, text},
};
use nokkvi_data::types::artist_recs::{ArtistRec, LibraryMatch};

use crate::{
    embedded_svg, theme,
    widgets::picker_modal::{PickerChrome, PickerList, picker_modal},
};

/// `text_input` ID for the modal's search field, focused on open.
pub(crate) const SIMILAR_ARTISTS_SEARCH_INPUT_ID: &str = "similar_artists_modal_search";

/// Where the modal's list stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SimilarArtistsStatus {
    /// Asked; nothing has landed yet.
    Loading,
    /// The list landed (an empty one means no source knows of anyone
    /// similar).
    Ready,
    /// Both Similar Artists sources are off, so nothing is asked.
    Off,
    /// Neither the library nor the server's agents know the artist's
    /// MusicBrainz id, so ListenBrainz cannot be asked, and Deezer is off.
    NoMbid,
    /// No source could be reached, or a library lookup failed.
    Failed,
}

/// State for the Similar Artists modal.
#[derive(Debug, Clone)]
pub struct SimilarArtistsModalState {
    /// The library artist the list is about.
    pub artist_id: String,
    pub artist_name: String,
    pub(crate) list: PickerList<ArtistRec>,
    pub(crate) status: SimilarArtistsStatus,
}

impl SimilarArtistsModalState {
    pub(crate) fn new(
        artist_id: String,
        artist_name: String,
        status: SimilarArtistsStatus,
    ) -> Self {
        Self {
            artist_id,
            artist_name,
            list: PickerList::new(Vec::new()),
            status,
        }
    }

    /// The list landed: show it, keeping whatever was typed in the search.
    pub(crate) fn set_recs(&mut self, recs: Vec<ArtistRec>) {
        let query = std::mem::take(&mut self.list.search_query);
        self.list = PickerList::new(recs);
        self.list.search_query = query;
        self.refilter();
        self.status = SimilarArtistsStatus::Ready;
    }

    /// Filter by name or MusicBrainz note, case-insensitively.
    pub(crate) fn refilter(&mut self) {
        self.list.refilter(|rec, query| {
            query.is_empty()
                || rec.name.to_lowercase().contains(query)
                || rec
                    .comment
                    .as_deref()
                    .is_some_and(|comment| comment.to_lowercase().contains(query))
        });
    }
}

/// Modal messages.
#[derive(Debug, Clone)]
pub enum SimilarArtistsModalMessage {
    /// Open for this artist: a library row menu's Similar Artists entry.
    Open(crate::views::ArtistRef),
    /// Escape, the X, or a click on the backdrop.
    Close,
    SearchChanged(String),
    SlotListUp,
    SlotListDown,
    SlotListSetOffset(usize),
    /// A press on a row of the filtered list: find the artist in the
    /// library, or open their page on the service that listed them.
    ClickItem(usize),
    /// The same for the centered row (Enter).
    ActivateCenter,
    /// The row's globe: the artist's page on the service that listed them,
    /// library or not.
    OpenArtistPage(usize),
}

/// Render the modal, meant to be stacked on top of the app.
pub(crate) fn similar_artists_modal_overlay<'a>(
    state: &'a SimilarArtistsModalState,
    window_height: f32,
) -> Element<'a, SimilarArtistsModalMessage> {
    let empty_text = match state.status {
        SimilarArtistsStatus::Loading => "Looking for similar artists...",
        SimilarArtistsStatus::Ready => "No similar artists found for this one",
        SimilarArtistsStatus::Off => "Turned off in Settings: General > Similar Artists",
        SimilarArtistsStatus::NoMbid => {
            "No MusicBrainz id is known for this artist, so ListenBrainz cannot be asked"
        }
        SimilarArtistsStatus::Failed => {
            "Similar artists could not be fetched. Close and try again."
        }
    };
    let who = text(format!("to {}", state.artist_name))
        .size(13.0)
        .font(theme::ui_font())
        .color(theme::fg2())
        .wrapping(iced::widget::text::Wrapping::None);
    picker_modal(
        PickerChrome {
            title: "Similar Artists",
            after_title: Some(who.boxed()),
            before_close: None,
            search_placeholder: "Type to filter artists...",
            search_input_id: SIMILAR_ARTISTS_SEARCH_INPUT_ID,
            on_search: SimilarArtistsModalMessage::SearchChanged,
            on_close: SimilarArtistsModalMessage::Close,
            on_up: SimilarArtistsModalMessage::SlotListUp,
            on_down: SimilarArtistsModalMessage::SlotListDown,
            on_set_offset: SimilarArtistsModalMessage::SlotListSetOffset,
            empty_text,
            window_height,
        },
        &state.list,
        move |rec, ctx| render_row(rec, ctx.item_index, ctx.is_center, ctx.row_height),
    )
}

/// Where the artist stands against the library, in words.
fn standing_label(rec: &ArtistRec) -> &'static str {
    match &rec.library {
        LibraryMatch::Found(artist) if artist.album_artist => "In your library",
        LibraryMatch::Found(_) => "In your library, on other artists' albums",
        LibraryMatch::Missing => "Not in your library",
        LibraryMatch::Unknown => "Library lookup failed",
    }
}

/// Where the artist stands against the library, as the row says it: the
/// label, in the accent for an artist the library has.
fn standing(rec: &ArtistRec) -> (&'static str, iced::Color) {
    let color = if rec.library_id().is_some() {
        theme::accent()
    } else {
        theme::fg3()
    };
    (standing_label(rec), color)
}

/// What a similar artist's hover note says (the Artists panel's chips):
/// MusicBrainz's note on them when there is one, where they stand against
/// the library, then who listed them, the same words the modal's row uses.
pub(crate) fn similar_artist_note(rec: &ArtistRec) -> String {
    let standing = standing_label(rec);
    let source = rec.source.label();
    match &rec.comment {
        Some(comment) => format!("{comment} · {standing} · {source}"),
        None => format!("{standing} · {source}"),
    }
}

fn render_row<'a>(
    rec: &ArtistRec,
    item_index: usize,
    is_center: bool,
    row_height: f32,
) -> Element<'a, SimilarArtistsModalMessage> {
    let label_color = if is_center {
        theme::fg0()
    } else {
        theme::fg2()
    };
    let note_color = if is_center {
        theme::fg2()
    } else {
        theme::fg3()
    };
    let weight = if is_center {
        Weight::Bold
    } else {
        Weight::Medium
    };
    let title = text(rec.name.clone())
        .size(14.0)
        .font(theme::weighted_ui_font(weight))
        .color(label_color)
        .wrapping(iced::widget::text::Wrapping::None);

    let (standing_text, standing_color) = standing(rec);
    let mut subtitle = row![].spacing(0).align_y(Alignment::Center);
    if let Some(comment) = &rec.comment {
        subtitle = subtitle.push(
            text(format!("{comment} · "))
                .size(11.0)
                .font(theme::ui_font())
                .color(note_color)
                .wrapping(iced::widget::text::Wrapping::None)
                .boxed(),
        );
    }
    subtitle = subtitle.push(
        text(standing_text)
            .size(11.0)
            .font(theme::ui_font())
            .color(standing_color)
            .wrapping(iced::widget::text::Wrapping::None)
            .boxed(),
    );
    subtitle = subtitle.push(
        text(format!(" · {}", rec.source.label()))
            .size(11.0)
            .font(theme::ui_font())
            .color(note_color)
            .wrapping(iced::widget::text::Wrapping::None)
            .boxed(),
    );
    let text_col = column![title, subtitle].spacing(2);

    let globe_color = theme::fg3();
    let globe = button(
        embedded_svg::svg_widget("assets/icons/globe.svg")
            .width(Length::Fixed(14.0))
            .height(Length::Fixed(14.0))
            .style(move |_theme, _status| svg::Style {
                color: Some(globe_color),
            }),
    )
    .on_press(SimilarArtistsModalMessage::OpenArtistPage(item_index))
    .padding(6)
    .style(|_theme, status| {
        let hovered = matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );
        iced::widget::button::Style {
            background: hovered.then(|| theme::bg2().into()),
            border: Border {
                radius: theme::ui_radius_sm(),
                ..Default::default()
            },
            ..Default::default()
        }
    });

    let body = container(
        row![
            Space::new().width(Length::Fixed(16.0)),
            container(text_col).width(Length::Fill).clip(true),
            Space::new().width(Length::Fixed(8.0)),
            globe,
            Space::new().width(Length::Fixed(8.0)),
        ]
        .align_y(Alignment::Center)
        .height(Length::Fill),
    )
    .height(Length::Fixed(row_height))
    .width(Length::Fill)
    .style(move |_: &iced::Theme| container::Style {
        background: is_center.then(|| theme::bg1().into()),
        border: Border {
            radius: theme::ui_radius_sm(),
            ..Default::default()
        },
        ..Default::default()
    });

    // The globe is a button, so its press is captured before the row's.
    mouse_area(body)
        .on_press(SimilarArtistsModalMessage::ClickItem(item_index))
        .interaction(iced::mouse::Interaction::Pointer)
        .boxed()
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::artist_recs::{LibraryArtist, RecSource};

    use super::*;

    fn rec(comment: Option<&str>, library: LibraryMatch) -> ArtistRec {
        ArtistRec {
            source: RecSource::ListenBrainz { mbid: "m".into() },
            name: "Muse".into(),
            comment: comment.map(str::to_string),
            library,
        }
    }

    #[test]
    fn the_note_leads_with_musicbrainz_then_the_library_standing_then_the_source() {
        assert_eq!(
            similar_artist_note(&rec(Some("UK rock band"), LibraryMatch::Missing)),
            "UK rock band · Not in your library · ListenBrainz"
        );
        assert_eq!(
            similar_artist_note(&rec(
                None,
                LibraryMatch::Found(LibraryArtist {
                    id: "a".into(),
                    album_artist: false,
                })
            )),
            "In your library, on other artists' albums · ListenBrainz"
        );
        assert_eq!(
            similar_artist_note(&ArtistRec {
                source: RecSource::Deezer { id: 743 },
                ..rec(None, LibraryMatch::Unknown)
            }),
            "Library lookup failed · Deezer"
        );
    }
}
