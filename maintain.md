# Code Good, But Some Heavy Rocks

Me look at code. Code compile clean. Clippy make no angry noise (0 warnings). Caveman happy!

But some rocks too heavy to carry. Need break rocks into smaller pebbles.

## Big Heavy Rocks (Split these!)
- **`src/update/tests.rs` (2409 lines)**: Test rock too big! Break into small test pebbles. Maybe `update/tests/playback.rs`, `update/tests/ui.rs`.
- **`data/src/audio/engine.rs` (1891 lines)**: Big sound machine. Too many moving parts. Separate state from logic.
- **`data/src/types/hotkey_config.rs` (1458 lines)**: Why button push config so fat? Try make thin. Use macro or split config parts.
- **`src/widgets/slot_list.rs` (1285 lines)**: List widget do too much. Extract row logic. Extract scroll logic.
- **View Rocks**: `artists.rs` (1254 lines), `settings/rendering.rs` (1238 lines), `settings/mod.rs` (1209 lines). View rocks too heavy. Split view into tiny widget parts.

## Unfinished Business
Me hunt `TODO`. Found 7 `TODO`s. 
Almost all in `data/src/services/task_manager.rs`. 
Need finish UI notification for task manager! Add JoinHandle, add status channel. Do it.

## Architecture Talk
TEA (The Elm Architecture) good. Keep state safe. But `update/mod.rs` and big update handlers grow fast. Make sure `components.rs` take shared logic so update handlers stay small.

Code good. Break big rocks. Caveman done.
