//! Progressive queue append handler — chains paginated song fetches into the queue.

use iced::Task;
use tracing::debug;

use crate::{Nokkvi, app_message::Message};

impl Nokkvi {
    /// Chain-load songs page by page into the queue.
    ///
    /// Each invocation fetches one page, appends it to the queue, refreshes the UI,
    /// and (if more pages remain) emits the next `ProgressiveQueueAppendPage` message.
    /// A generation counter guards against stale chains from superseded play actions.
    pub(crate) fn handle_progressive_queue_append_page(
        &mut self,
        sort_mode: String,
        sort_order: String,
        search_query: Option<String>,
        offset: usize,
        total_count: usize,
        generation: u64,
    ) -> Task<Message> {
        // Stale generation check: if a newer play-from-songs has started,
        // this chain is obsolete — stop silently.
        if generation != self.library.progressive_queue_generation {
            debug!(
                "📄 Progressive queue: stale chain (gen {} vs current {}), cancelling",
                generation, self.library.progressive_queue_generation
            );
            return Task::none();
        }

        let search_q = search_query.clone();
        let sort_m = sort_mode.clone();
        let sort_o = sort_order.clone();
        let page_size = self.settings.library_page_size.to_usize();
        let fetch_task = self.shell_task(
            move |shell| async move {
                let songs = shell
                    .songs()
                    .load_raw_songs_page(
                        Some(&sort_m),
                        Some(&sort_o),
                        search_q.as_deref(),
                        None,
                        offset,
                        page_size,
                    )
                    .await?;
                let count = songs.len();
                shell.queue().add_songs(songs).await?;
                Ok(count)
            },
            move |result: Result<usize, anyhow::Error>| match result {
                Ok(count) => {
                    let new_offset = offset + count;
                    debug!(
                        "📄 Progressive queue: appended {} songs ({}→{} of {})",
                        count, offset, new_offset, total_count
                    );
                    if count == 0 || new_offset >= total_count {
                        // Done — clear progressive loading target, then refresh queue UI
                        Message::ProgressiveQueueDone
                    } else {
                        // Chain next page fetch (LoadQueue fires first via batch)
                        Message::ProgressiveQueueAppendPage {
                            sort_mode,
                            sort_order,
                            search_query,
                            offset: new_offset,
                            total_count,
                            generation,
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(" Progressive queue failed: {}", e);
                    Message::ProgressiveQueueDone // Clear target and refresh what we have
                }
            },
        );
        // Refresh queue UI with what's been loaded so far, then fetch next page
        Task::batch(vec![Task::done(Message::LoadQueue), fetch_task])
    }
}
