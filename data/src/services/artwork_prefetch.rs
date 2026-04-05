//! Background artwork pre-fetching service
//!
//! Downloads all album artwork (thumbnails + large) in the background after login
//! to build the cache without requiring manual scrolling.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures::stream::{self, StreamExt};
use tokio::sync::mpsc;
use tracing::debug;

use crate::{
    types::{album::Album, progress::ProgressHandle},
    utils::{artwork_url, cache::DiskCache},
};

/// Progress update from the prefetch task (sent on completion)
#[derive(Debug, Clone)]
pub struct PrefetchProgress;

/// Start background prefetching of all album artwork
/// Returns a channel that receives progress updates
pub fn start_prefetch(
    albums: Vec<Album>,
    server_url: String,
    subsonic_credential: String,
    disk_cache: Arc<Option<DiskCache>>,
    progress: Option<ProgressHandle>,
    high_res_size: Option<u32>,
) -> mpsc::Receiver<PrefetchProgress> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        prefetch_all_artwork(
            albums,
            server_url,
            subsonic_credential,
            disk_cache,
            tx,
            progress,
            high_res_size,
        )
        .await;
    });

    rx
}

async fn prefetch_all_artwork(
    albums: Vec<Album>,
    server_url: String,
    subsonic_credential: String,
    disk_cache: Arc<Option<DiskCache>>,
    progress_tx: mpsc::Sender<PrefetchProgress>,
    progress: Option<ProgressHandle>,
    high_res_size: Option<u32>,
) {
    let total_albums = albums.len();
    let client = reqwest::Client::new();

    // Set total: 2 phases × album count (thumbnails + large)
    if let Some(ref h) = progress {
        h.set_total(total_albums * 2);
    }

    // Phase 1: Thumbnails (80px)
    debug!(
        " [PREFETCH] Starting thumbnail prefetch for {} albums...",
        total_albums
    );

    let completed = Arc::new(AtomicUsize::new(0));
    let progress_clone = progress.clone();
    const MAX_CONCURRENT: usize = 4; // Conservative to avoid overwhelming server

    let thumbnail_tasks: Vec<_> = albums
        .iter()
        .filter_map(|album| {
            let art_id = album.cover_art.as_deref().unwrap_or(&album.id).to_string();
            let (post_url, post_body) = artwork_url::build_cover_art_post_params(
                &art_id,
                &server_url,
                &subsonic_credential,
                Some(artwork_url::THUMBNAIL_SIZE),
                album.updated_at.as_deref(),
            )?;
            let cache_key =
                artwork_url::build_cache_key(&art_id, Some(artwork_url::THUMBNAIL_SIZE));
            Some((post_url, post_body, cache_key))
        })
        .collect();

    let disk_cache_clone = disk_cache.clone();
    let client_clone = client.clone();

    let results: Vec<_> = stream::iter(thumbnail_tasks)
        .map(|(post_url, post_body, cache_key)| {
            let dc = disk_cache_clone.clone();
            let client = client_clone.clone();
            let completed = completed.clone();
            let progress_h = progress_clone.clone();
            let total = total_albums;

            async move {
                // Skip if already cached
                if let Some(ref cache) = *dc
                    && cache.contains(&cache_key)
                {
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref h) = progress_h {
                        h.set_completed(count);
                    }
                    if count.is_multiple_of(50) {
                        debug!(" [PREFETCH] Thumbnails: {}/{} (cached)", count, total);
                    }
                    return true; // Already cached
                }

                // Download via POST (credentials in body, not URL)
                if let Ok(response) = client
                    .post(&post_url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(post_body)
                    .send()
                    .await
                    && response.status().is_success()
                {
                    // Capture Last-Modified for proper cache timing
                    let last_modified = response
                        .headers()
                        .get("last-modified")
                        .and_then(|h| h.to_str().ok())
                        .and_then(|s| httpdate::parse_http_date(s).ok());

                    if let Ok(bytes) = response.bytes().await {
                        if let Some(ref cache) = *dc {
                            cache.insert(&cache_key, &bytes);
                            // Set file mtime to match server
                            if let Some(server_time) = last_modified {
                                let path = cache.get_path(&cache_key);
                                let _ = filetime::set_file_mtime(
                                    &path,
                                    filetime::FileTime::from_system_time(server_time),
                                );
                            }
                        }
                        let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(ref h) = progress_h {
                            h.set_completed(count);
                        }
                        if count.is_multiple_of(50) {
                            debug!(" [PREFETCH] Thumbnails: {}/{} (downloaded)", count, total);
                        }
                        return true;
                    }
                }
                let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref h) = progress_h {
                    h.set_completed(count);
                }
                false
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    let thumbnails_cached = results.iter().filter(|&&r| r).count();
    debug!(
        " [PREFETCH] Thumbnails complete: {}/{} cached",
        thumbnails_cached, total_albums
    );

    // Phase 2: Large artwork (1000px)
    debug!(
        " [PREFETCH] Starting large artwork prefetch for {} albums...",
        total_albums
    );

    let completed = Arc::new(AtomicUsize::new(0));
    let progress_clone = progress.clone();

    let large_tasks: Vec<_> = albums
        .iter()
        .filter_map(|album| {
            let art_id = album.cover_art.as_deref().unwrap_or(&album.id).to_string();
            let (post_url, post_body) = artwork_url::build_cover_art_post_params(
                &art_id,
                &server_url,
                &subsonic_credential,
                high_res_size,
                album.updated_at.as_deref(),
            )?;
            let cache_key = artwork_url::build_cache_key(&art_id, high_res_size);
            Some((post_url, post_body, cache_key))
        })
        .collect();

    let results: Vec<_> = stream::iter(large_tasks)
        .map(|(post_url, post_body, cache_key)| {
            let dc = disk_cache.clone();
            let client = client.clone();
            let completed = completed.clone();
            let progress_h = progress_clone.clone();
            let total = total_albums;

            async move {
                // Skip if already cached
                if let Some(ref cache) = *dc
                    && cache.contains(&cache_key)
                {
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref h) = progress_h {
                        h.set_completed(total + count); // Phase 2 offset
                    }
                    if count.is_multiple_of(50) {
                        debug!(" [PREFETCH] Large: {}/{} (cached)", count, total);
                    }
                    return true;
                }

                // Download via POST (credentials in body, not URL)
                if let Ok(response) = client
                    .post(&post_url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(post_body)
                    .send()
                    .await
                    && response.status().is_success()
                {
                    let last_modified = response
                        .headers()
                        .get("last-modified")
                        .and_then(|h| h.to_str().ok())
                        .and_then(|s| httpdate::parse_http_date(s).ok());

                    if let Ok(bytes) = response.bytes().await {
                        if let Some(ref cache) = *dc {
                            cache.insert(&cache_key, &bytes);
                            if let Some(server_time) = last_modified {
                                let path = cache.get_path(&cache_key);
                                let _ = filetime::set_file_mtime(
                                    &path,
                                    filetime::FileTime::from_system_time(server_time),
                                );
                            }
                        }
                        let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(ref h) = progress_h {
                            h.set_completed(total + count); // Phase 2 offset
                        }
                        if count.is_multiple_of(50) {
                            debug!(" [PREFETCH] Large: {}/{} (downloaded)", count, total);
                        }
                        return true;
                    }
                }
                let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref h) = progress_h {
                    h.set_completed(total + count);
                }
                false
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    let large_cached = results.iter().filter(|&&r| r).count();
    debug!(
        " [PREFETCH] Large artwork complete: {}/{} cached",
        large_cached, total_albums
    );

    // Send completion
    if let Some(ref h) = progress {
        h.mark_done();
    }
    let _ = progress_tx.send(PrefetchProgress).await;

    debug!(
        " [PREFETCH] ✅ Artwork prefetch complete! {} thumbnails, {} large images",
        thumbnails_cached, large_cached
    );
}

/// Check if cache appears incomplete (less than 80% of expected files)
pub fn is_cache_incomplete(disk_cache: &Option<DiskCache>, expected_count: usize) -> bool {
    if expected_count == 0 {
        return false;
    }

    let Some(cache) = disk_cache else {
        return true;
    };

    // Count files in cache directory
    let cache_path = cache.get_path("");
    let cache_dir = cache_path.parent().unwrap_or(&cache_path);

    let file_count = std::fs::read_dir(cache_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);

    // Expected: 2 files per album (thumbnail + large)
    let expected_files = expected_count * 2;
    let threshold = (expected_files as f64 * 0.8) as usize;

    file_count < threshold
}

/// Start background prefetching of all artist artwork
/// Returns a channel that receives progress updates
pub fn start_artist_prefetch(
    artist_ids: Vec<(String, String)>, // (id, name) pairs
    server_url: String,
    subsonic_credential: String,
    disk_cache: Option<DiskCache>,
    progress: Option<ProgressHandle>,
) -> mpsc::Receiver<PrefetchProgress> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        prefetch_all_artist_artwork(
            artist_ids,
            server_url,
            subsonic_credential,
            disk_cache,
            tx,
            progress,
        )
        .await;
    });

    rx
}

async fn prefetch_all_artist_artwork(
    artist_ids: Vec<(String, String)>,
    server_url: String,
    subsonic_credential: String,
    disk_cache: Option<DiskCache>,
    progress_tx: mpsc::Sender<PrefetchProgress>,
    progress: Option<ProgressHandle>,
) {
    let total_artists = artist_ids.len();
    let client = reqwest::Client::new();

    // Set total: 2 phases × artist count (mini + large)
    if let Some(ref h) = progress {
        h.set_total(total_artists * 2);
    }

    debug!(
        " [PREFETCH] Starting artist artwork prefetch for {} artists...",
        total_artists
    );

    let completed = Arc::new(AtomicUsize::new(0));
    let progress_clone = progress.clone();
    const MAX_CONCURRENT: usize = 4;

    // Build tasks: fetch getCoverArt for each artist
    let tasks: Vec<_> = artist_ids
        .iter()
        .filter_map(|(id, _name)| {
            let (post_url, post_body) = artwork_url::build_cover_art_post_params(
                &format!("ar-{id}"),
                &server_url,
                &subsonic_credential,
                Some(80),
                None,
            )?;
            let cache_key = format!("ar-{id}_80");
            Some((post_url, post_body, cache_key, id.clone()))
        })
        .collect();

    let disk_cache_arc = Arc::new(disk_cache);
    let cache_hits = Arc::new(AtomicUsize::new(0));
    let downloads = Arc::new(AtomicUsize::new(0));

    let results: Vec<_> = stream::iter(tasks)
        .map(|(post_url, post_body, cache_key, _artist_id)| {
            let dc = disk_cache_arc.clone();
            let client = client.clone();
            let completed = completed.clone();
            let cache_hits = cache_hits.clone();
            let downloads = downloads.clone();
            let progress_h = progress_clone.clone();
            let total = total_artists;

            async move {
                // Skip if already cached
                if let Some(ref cache) = *dc
                    && cache.contains(&cache_key)
                {
                    let hits = cache_hits.fetch_add(1, Ordering::Relaxed) + 1;
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref h) = progress_h {
                        h.set_completed(count);
                    }
                    if count.is_multiple_of(50) {
                        let dl = downloads.load(Ordering::Relaxed);
                        debug!(
                            " [PREFETCH] Artists: {}/{} ({} cached, {} downloaded)",
                            count, total, hits, dl
                        );
                    }
                    return true;
                }

                // Download via POST (credentials in body, not URL)
                if let Ok(response) = client
                    .post(&post_url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(post_body)
                    .send()
                    .await
                    && response.status().is_success()
                    && let Ok(bytes) = response.bytes().await
                    && bytes.len() > 100
                {
                    // Ignore tiny/empty responses
                    if let Some(ref cache) = *dc {
                        cache.insert(&cache_key, &bytes);
                    }
                    let dl = downloads.fetch_add(1, Ordering::Relaxed) + 1;
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref h) = progress_h {
                        h.set_completed(count);
                    }
                    if count.is_multiple_of(50) {
                        let hits = cache_hits.load(Ordering::Relaxed);
                        debug!(
                            " [PREFETCH] Artists: {}/{} ({} cached, {} downloaded)",
                            count, total, hits, dl
                        );
                    }
                    return true;
                }
                // Log progress even for failures (no artwork available)
                let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref h) = progress_h {
                    h.set_completed(count);
                }
                if count.is_multiple_of(50) {
                    let hits = cache_hits.load(Ordering::Relaxed);
                    let dl = downloads.load(Ordering::Relaxed);
                    debug!(
                        " [PREFETCH] Artists: {}/{} ({} cached, {} downloaded, {} no artwork)",
                        count,
                        total,
                        hits,
                        dl,
                        count - hits - dl
                    );
                }
                false
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    let artists_cached = results.iter().filter(|&&r| r).count();
    debug!(
        " [PREFETCH] ✅ Mini artwork complete: {}/{}",
        artists_cached, total_artists
    );

    // Phase 2: Large artwork (500px) for center slot display
    debug!(
        " [PREFETCH] Starting large artwork prefetch for {} artists...",
        total_artists
    );

    let completed = Arc::new(AtomicUsize::new(0));
    let cache_hits = Arc::new(AtomicUsize::new(0));
    let downloads = Arc::new(AtomicUsize::new(0));
    let progress_clone = progress.clone();

    let large_tasks: Vec<_> = artist_ids
        .iter()
        .filter_map(|(id, _name)| {
            let (post_url, post_body) = artwork_url::build_cover_art_post_params(
                &format!("ar-{id}"),
                &server_url,
                &subsonic_credential,
                Some(500),
                None,
            )?;
            let cache_key = format!("ar-{id}_500");
            Some((post_url, post_body, cache_key, id.clone()))
        })
        .collect();

    let large_results: Vec<_> = stream::iter(large_tasks)
        .map(|(post_url, post_body, cache_key, _artist_id)| {
            let dc = disk_cache_arc.clone();
            let client = client.clone();
            let completed = completed.clone();
            let cache_hits = cache_hits.clone();
            let downloads = downloads.clone();
            let progress_h = progress_clone.clone();
            let total = total_artists;

            async move {
                // Skip if already cached
                if let Some(ref cache) = *dc
                    && cache.contains(&cache_key)
                {
                    let hits = cache_hits.fetch_add(1, Ordering::Relaxed) + 1;
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref h) = progress_h {
                        h.set_completed(total + count); // Phase 2 offset
                    }
                    if count.is_multiple_of(50) {
                        let dl = downloads.load(Ordering::Relaxed);
                        debug!(
                            " [PREFETCH] Large: {}/{} ({} cached, {} downloaded)",
                            count, total, hits, dl
                        );
                    }
                    return true;
                }

                // Download via POST (credentials in body, not URL)
                if let Ok(response) = client
                    .post(&post_url)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(post_body)
                    .send()
                    .await
                    && response.status().is_success()
                    && let Ok(bytes) = response.bytes().await
                    && bytes.len() > 100
                {
                    if let Some(ref cache) = *dc {
                        cache.insert(&cache_key, &bytes);
                    }
                    let dl = downloads.fetch_add(1, Ordering::Relaxed) + 1;
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref h) = progress_h {
                        h.set_completed(total + count); // Phase 2 offset
                    }
                    if count.is_multiple_of(50) {
                        let hits = cache_hits.load(Ordering::Relaxed);
                        debug!(
                            " [PREFETCH] Large: {}/{} ({} cached, {} downloaded)",
                            count, total, hits, dl
                        );
                    }
                    return true;
                }
                let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref h) = progress_h {
                    h.set_completed(total + count);
                }
                if count.is_multiple_of(50) {
                    let hits = cache_hits.load(Ordering::Relaxed);
                    let dl = downloads.load(Ordering::Relaxed);
                    debug!(
                        " [PREFETCH] Large: {}/{} ({} cached, {} downloaded, {} no artwork)",
                        count,
                        total,
                        hits,
                        dl,
                        count - hits - dl
                    );
                }
                false
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    let large_cached = large_results.iter().filter(|&&r| r).count();
    debug!(
        " [PREFETCH] ✅ Large artwork complete: {}/{}",
        large_cached, total_artists
    );

    // Send completion
    if let Some(ref h) = progress {
        h.mark_done();
    }
    let _ = progress_tx.send(PrefetchProgress).await;
}
