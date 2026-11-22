//! Discord Rich Presence: the playback tick tells the presence worker what plays.

use std::time::{SystemTime, UNIX_EPOCH};

use iced::Task;
use nokkvi_data::services::discord_rpc::{Presence, usable_cover_url};
use tracing::debug;

use crate::{
    Nokkvi, app_message::Message, state::CoverAnswer, update::components::session_expired_message,
};

/// A song plays this long before its album's cover is asked for, so skipping
/// through a queue asks the server about none of the albums passed.
const COVER_ASK_AFTER_SECS: u32 = 3;

/// The current unix second, as the presence worker and the cover asks count time.
pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// What Discord should show for a tick, or `None` when nothing plays (which
/// clears the activity). `duration_secs` is `None` for a radio stream, which
/// has no length and so no progress bar.
pub(crate) fn presence_for(
    title: &str,
    artist: &str,
    album: &str,
    duration_secs: Option<u32>,
    position_secs: u32,
    playing: bool,
    paused: bool,
) -> Option<Presence> {
    if !(playing || paused) || title.trim().is_empty() {
        return None;
    }
    Some(Presence {
        title: title.to_owned(),
        artist: artist.to_owned(),
        album: album.to_owned(),
        duration_secs: duration_secs.filter(|d| *d > 0),
        position_secs,
        playing: playing && !paused,
        cover_url: None,
    })
}

impl Nokkvi {
    /// Push this tick's now-playing state to Discord. A stale tick waits for
    /// the next one: `position` is `None` when a seek moved the playhead after
    /// the tick read the engine (so a seek never flashes the old time), and
    /// `item_current` is false when the tick was built for the item before a
    /// station or queue/radio switch (`tick_matches_active_item`, as MPRIS
    /// checks). Returns the request for the album's public cover link when
    /// one is due.
    #[expect(
        clippy::too_many_arguments,
        reason = "the playback tick's fields, passed as the MPRIS push takes them"
    )]
    pub(crate) fn sync_discord_presence(
        &mut self,
        song_id: Option<&str>,
        album: &str,
        duration_secs: u32,
        position: Option<u32>,
        item_current: bool,
        playing: bool,
        paused: bool,
    ) -> Option<Task<Message>> {
        if !self.settings.discord_presence {
            // Off (the default): stop any worker and build nothing.
            self.discord.sync(false, "", None, 0);
            return None;
        }
        let position = position.filter(|_| item_current)?;
        // Radio shows the ICY artist and track, like MPRIS does.
        let (title, artist, is_radio) =
            if let crate::state::ActivePlayback::Radio(ref state) = self.active_playback {
                (
                    state.icy_title.as_deref().unwrap_or(&self.playback.title),
                    state.icy_artist.as_deref().unwrap_or(&self.playback.artist),
                    true,
                )
            } else {
                (
                    self.playback.title.as_str(),
                    self.playback.artist.as_str(),
                    false,
                )
            };
        let mut presence = presence_for(
            title,
            artist,
            album,
            (!is_radio).then_some(duration_secs),
            position,
            playing,
            paused,
        );

        let now = unix_now();
        // The cover is the server's public link for the song's album; a
        // station has none.
        let mut cover_request = None;
        if let (Some(presence), Some(song_id), false) = (presence.as_mut(), song_id, is_radio)
            && self.discord.is_running()
        {
            let queue = &self.library.queue_songs;
            let album_id = self
                .discord
                .album_of(song_id, || {
                    queue
                        .iter()
                        .find(|s| s.id == song_id)
                        .map(|s| s.album_id.clone())
                        .filter(|id| !id.is_empty())
                })
                .map(str::to_owned);
            if let Some(album_id) = album_id {
                presence.cover_url = self.discord.cover_of(&album_id).map(str::to_owned);
                if position >= COVER_ASK_AFTER_SECS && self.discord.claim_cover(&album_id, now) {
                    cover_request = Some(album_id);
                }
            }
        }

        self.discord.sync(
            self.settings.discord_presence,
            &self.settings.discord_app_id,
            presence,
            now,
        );
        cover_request.map(|album_id| self.discord_cover_task(album_id))
    }

    /// Ask the server for `album_id`'s public cover link. Only a link
    /// [`usable_cover_url`] vets reaches the presence; any other answer (none,
    /// a credentialed or LAN-only link, an error) leaves the default icon.
    fn discord_cover_task(&self, album_id: String) -> Task<Message> {
        self.shell_task(
            {
                let album_id = album_id.clone();
                move |shell| async move {
                    shell
                        .album_info_api()
                        .await?
                        .get_album_image_url(&album_id)
                        .await
                }
            },
            move |result| match result {
                Ok(url) => {
                    let answer = match (url.as_deref().and_then(usable_cover_url), url) {
                        (Some(usable), _) => CoverAnswer::Link(usable),
                        (None, Some(_)) => {
                            debug!(
                                "Discord cover for {album_id} is not shareable; using the icon \
                                 and asking about no more covers this session"
                            );
                            CoverAnswer::PrivateLink
                        }
                        (None, None) => CoverAnswer::NoLink,
                    };
                    Message::DiscordCover { album_id, answer }
                }
                Err(e) => session_expired_message(&e).unwrap_or_else(|| {
                    debug!("Discord cover lookup for {album_id} failed: {e}");
                    Message::DiscordCover {
                        album_id,
                        answer: CoverAnswer::Failed,
                    }
                }),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_playing_clears_the_presence() {
        assert!(presence_for("Song", "A", "B", Some(200), 0, false, false).is_none());
        assert!(presence_for("", "A", "B", Some(200), 0, true, false).is_none());
    }

    #[test]
    fn a_song_carries_its_duration_and_a_station_does_not() {
        let song = presence_for("Song", "A", "B", Some(200), 12, true, false).unwrap();
        assert_eq!(song.duration_secs, Some(200));
        assert!(song.playing);
        let radio = presence_for("Song", "A", "B", None, 12, true, false).unwrap();
        assert_eq!(radio.duration_secs, None);
    }

    #[test]
    fn paused_still_shows_but_not_as_playing() {
        let p = presence_for("Song", "A", "B", Some(200), 12, false, true).unwrap();
        assert!(!p.playing);
    }
}
