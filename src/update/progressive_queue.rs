//! Progressive queue append handler — chains paginated song fetches into the queue.

use iced::Task;
use nokkvi_data::types::library_query::LibraryQuery;
use tracing::debug;

use crate::{Nokkvi, app_message::Message};

impl Nokkvi {
    /// Chain-load songs page by page into the queue.
    ///
    /// Each invocation fetches one page, appends it to the queue, refreshes the UI,
    /// and (if more pages remain) emits the next `ProgressiveQueueAppendPage` message.
    /// A generation counter guards against stale chains from superseded play actions.
    // Mirrors the `ProgressiveQueueAppendPage` message fields 1:1 (sort/search/filter
    // + paging cursor + generation) — a params struct would just shuffle the same
    // values through an extra type for this single-caller internal handler.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_progressive_queue_append_page(
        &mut self,
        sort_mode: String,
        sort_order: String,
        search_query: Option<String>,
        filter: Option<nokkvi_data::types::filter::LibraryFilter>,
        offset: usize,
        total_count: usize,
        generation: u64,
    ) -> Task<Message> {
        // Stale generation check: if a newer play-from-songs has started,
        // this chain is obsolete — stop silently.
        if !self
            .library
            .progressive_queue_generation
            .is_current(generation)
        {
            debug!(
                "📄 Progressive queue: stale chain (gen {} vs current {}), cancelling",
                generation,
                self.library.progressive_queue_generation.current()
            );
            return Task::none();
        }

        let search_q = search_query.clone();
        let sort_m = sort_mode.clone();
        let sort_o = sort_order.clone();
        let filter_c = filter.clone();
        let page_size = self.settings.library_page_size.to_usize();
        let chain_generation = self.library.progressive_queue_generation.clone();
        let fetch_task = self.shell_task(
            move |shell| async move {
                let library_ids = shell.active_library_ids_vec();
                let songs = shell
                    .songs()
                    .load_raw_songs_page_with_libraries(
                        &LibraryQuery {
                            sort_mode: &sort_m,
                            sort_order: &sort_o,
                            search_query: search_q.as_deref(),
                            filter: filter_c.as_ref(),
                            library_ids: &library_ids,
                        },
                        offset,
                        page_size,
                    )
                    .await?;
                // A queue replacement during the fetch bumped the shared
                // generation: this page belongs to the replaced queue.
                if !chain_generation.is_current(generation) {
                    return Ok(None);
                }
                let count = songs.len();
                // Appending in shuffle mode invalidates the engine's
                // pre-buffered next-track decoder — discharge against
                // the engine immediately so the next gapless prep
                // picks the right song from the freshly-extended order.
                let effect = shell.queue().add_songs(songs).await?;
                effect.apply_to(&shell.audio_engine()).await;
                Ok(Some(count))
            },
            move |result: Result<Option<usize>, anyhow::Error>| match result {
                Ok(None) => {
                    debug!("📄 Progressive queue: dropped a page fetched for a replaced queue");
                    Message::NoOp
                }
                Ok(Some(count)) => {
                    let new_offset = offset + count;
                    debug!(
                        "📄 Progressive queue: appended {} songs ({}→{} of {})",
                        count, offset, new_offset, total_count
                    );
                    if count == 0 || new_offset >= total_count {
                        // Done — clear progressive loading target, then refresh queue UI
                        Message::ProgressiveQueueDone { generation }
                    } else {
                        // Chain next page fetch (LoadQueue fires first via batch)
                        Message::ProgressiveQueueAppendPage {
                            sort_mode,
                            sort_order,
                            search_query,
                            filter,
                            offset: new_offset,
                            total_count,
                            generation,
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(" Progressive queue failed: {}", e);
                    // Clear target and refresh what we have
                    Message::ProgressiveQueueDone { generation }
                }
            },
        );
        // Refresh queue UI with what's been loaded so far, then fetch next page
        Task::batch(vec![Task::done(Message::LoadQueue), fetch_task])
    }
}
