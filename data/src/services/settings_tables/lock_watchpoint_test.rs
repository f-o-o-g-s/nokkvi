//! Regression test: keep every `SettingsManager::set_*` setter synchronous so
//! the strangler-fig dispatcher in `src/update/settings.rs` can keep calling
//! it under `blocking_lock()` without janking the iced UI thread.

#[cfg(test)]
mod tests {
    /// Source of `SettingsManager`. Embedded at compile time; the test reads
    /// the file as bytes via `include_str!`.
    const SETTINGS_SRC: &str = include_str!("../settings.rs");

    #[test]
    fn sync_setters_only_under_blocking_lock() {
        // Walk every line; flag `pub async fn set_` (with optional `(crate)` /
        // `(super)` visibility). The dispatcher in
        // `src/update/settings.rs::handle_settings_general` calls these under
        // `blocking_lock()`; an async setter would jank the UI thread.
        let mut offending: Vec<(usize, &str)> = Vec::new();
        for (idx, line) in SETTINGS_SRC.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("pub async fn set_")
                || trimmed.starts_with("pub(crate) async fn set_")
                || trimmed.starts_with("pub(super) async fn set_")
            {
                offending.push((idx + 1, line));
            }
        }
        assert!(
            offending.is_empty(),
            "Found async `set_*` setter(s) on `SettingsManager`. The strangler-fig \
             dispatcher in `src/update/settings.rs` calls these under `blocking_lock()` \
             on the iced UI thread; an async setter would jank the UI for the duration \
             of its `.await`. Either keep the setter sync or revisit the dispatcher \
             before adding `.await`. Offending lines:\n{:#?}",
            offending
        );
    }
}
