---
name: macos-desktop-integration
description: macOS replacements for nokkvi's Linux desktop integration — Now Playing + media keys instead of MPRIS, a menu-bar status item instead of the ksni tray, UNUserNotificationCenter instead of D-Bus notifications, window hide/Dock reopen instead of Wayland close-and-reopen, Cmd shortcuts, `open` instead of xdg-open, rfd file panels, hostname. Use when touching src/services/{mpris,tray,notifications,mpris_art_writer}.rs, src/update/{mpris,tray,window,notifications,about_modal}.rs, src/hotkeys/, main.rs subscriptions/window settings, or data/src/services/api_key.rs host_name. Load macos-port first for the general rules.
---

# macOS desktop integration

`.claude/rules/gotchas/desktop.md` and `ui-window.md` auto-inject on these files and describe the Linux behaviour; their *intent* (one item per tick, a service ends with its subscription, the launch lock before the socket probe) carries over, the D-Bus mechanics don't.

## The shape to keep

Each Linux service is an iced subscription `run() -> impl Sipper<Never, XEvent>` that first emits `XEvent::Connected(XConnection)`; the app keeps the connection and pushes `XCommand`s through it; `src/update/x.rs` turns events into messages. On macOS, `src/main.rs` runs `services::mpris_macos::run` (Now Playing + media keys through `souvlaki`, same `MprisEvent`/`MprisCommand` types) and swaps the tray and notifications for `Subscription::none()`.

Port each service by giving it a macOS backend **with the same event/command/connection types**, so `src/update/{mpris,tray,notifications}.rs` and their tests in `src/update/tests/` stay shared. Rename a type only if its name becomes misleading (e.g. `MprisEvent` → `MediaControlEvent`), in its own refactor commit, before the macOS backend lands.

## Main-thread rule

AppKit and MediaPlayer objects (`NSStatusItem`, `NSMenu`, `NSApplication`, `MPRemoteCommandCenter` handlers, `NSOpenPanel`) must be created and touched on the **main thread**, which winit owns inside `iced::daemon`. A Linux-style "own thread + std mpsc" service will crash or silently do nothing on macOS. Options, in order of preference:

1. Do the AppKit work in code iced already runs on the main thread (the `update` path), and keep the background part (art fetching, channels) in the subscription.
2. Dispatch onto the main queue (`dispatch2`'s main queue / `MainThreadMarker`-checked calls via objc2) from the subscription.

Callbacks from AppKit (menu item clicked, remote command fired) come back on the main thread — forward them into the subscription through a channel, never call into iced state directly.

Check before writing: `objc2`, `objc2-foundation`, `objc2-app-kit` 0.3.x are already in the tree through winit/arboard (`cargo tree -i objc2-app-kit`). Reusing those versions is the cheapest path; MediaPlayer needs `objc2-media-player`, UserNotifications needs `objc2-user-notifications` — **new crates need the owner's OK** (CLAUDE.md). An alternative is one cross-platform crate per concern (e.g. `souvlaki` for media controls on Linux+macOS, `tray-icon` for the tray); weigh it against the existing, tuned Linux code (the MPRIS gotchas encode real bugs) — replacing the Linux backend is out of scope unless the owner asks.

## Per subsystem

**Now Playing / media keys (MPRIS).** `MPNowPlayingInfoCenter.defaultCenter().nowPlayingInfo` takes title/artist/album/duration/elapsed/rate/artwork (`MPMediaItemArtwork`, from image bytes — not a `file://` URL as on MPRIS); `playbackState` mirrors `MprisCommand::SetPlaybackStatus`. `MPRemoteCommandCenter` commands map onto existing `MprisEvent`s: play/pause/togglePlayPause/next/previous/stop, `changePlaybackPositionCommand` → `SetPosition`, `skipForward/Backward` → `Seek`, `changeRepeatMode` / `changeShuffleMode` → `SetLoopStatus` / `SetShuffle`. Keep the "one item per tick" check (`tick_matches_active_item`) and judge seeks against the published track (`PublishedTrack::set_position_target`); a stale Control Center scrubber must not seek another track. Media keys only route to the app while it is the current Now Playing app — it has to publish `nowPlayingInfo` and enable at least one command. Art cache: use `paths::get_cache_dir()` (`~/Library/Caches/nokkvi`), not the hand-rolled XDG path in `mpris_art_writer.rs`.

**Tray → menu-bar extra.** `NSStatusItem` with an `NSMenu` (Show/Hide, Play/Pause, Next, Previous, Quit → the existing `TrayEvent`s; `TrayCommand::SetPlayingState` updates the Play/Pause title and the tooltip). Use a template image (monochrome PDF/PNG with `setTemplate(true)`) so it follows light/dark menu bars. The "service ends with its subscription" rule still applies: turning Show Tray Icon off must remove the status item (`NSStatusBar.removeStatusItem`). Setting text (`ShowTrayIcon` label "Show Tray Icon", doc comment about SNI/waybar in `data/src/types/settings.rs`) should read naturally on macOS ("Show in Menu Bar") — the label comes from `data/src/services/settings_tables/general.rs`.

**Notifications.** `UNUserNotificationCenter` needs the process to be an app bundle with a `CFBundleIdentifier`; from a bare `target/release/nokkvi` it errors. Ask for authorization once (`requestAuthorizationWithOptions`) when the user enables a notification setting, not at launch. Coalesce-replace = reuse the same request identifier per `NotificationCommand` kind. A refused authorization is the macOS form of "missing notification daemon": log at `warn!`, idle, never crash (same as the Linux service). See `macos-bundle` for running from a bundle.

**Window: close-to-tray, hide, reopen.** Linux destroys and reopens the window because Wayland can't hide a surface (`src/update/tray.rs` module docs). On macOS hiding works (`window::set_mode(Hidden)` / `NSApp.hide`), and the native expectations differ:
- Red close button with close-to-tray (or by default on macOS conventions) hides the window; the app keeps playing; clicking the **Dock icon** must bring it back (`applicationShouldHandleReopen` — winit doesn't forward it today, so this needs a delegate hook or a workaround; verify what the pinned iced/winit exposes before designing).
- Cmd+Q quits, Cmd+W closes/hides the window, Cmd+H hides the app, Cmd+M minimizes.
- `Nokkvi::show_window` stays the single entry point; on macOS `window::gain_focus` works, so the `request_user_attention` fallback is Linux-only.
- `main_window_settings()` gates `PlatformSpecific { application_id }` to Linux; macOS's `PlatformSpecific` has its own fields (title bar style, `fullsize_content_view`, etc.) — check the pinned iced rev's `iced::window::settings::macos` before using any.

**Keyboard.** `KeyCombo` carries a `logo` flag (Cmd on macOS, Super elsewhere; config text `"Cmd + E"` / `"Super + E"`, every spelling parses everywhere), so ⌘ chords are ordinary bindings and ⌘C never fires a bare `c` binding (exact-match lookup). Shortcut-style defaults use `KeyCombo::primary` (⌘ on macOS, Ctrl elsewhere). The Cmd chords macOS and every text field own (⌘Q/W/H/M, ⌘C/V/X/A/Z, ⇧⌘Z, ⌘Tab, ⌘\`, ⌘Space) are `KeyCombo::is_os_reserved`: the capture UI refuses them and `HotkeyConfig::lookup_combo` never resolves them, so a text field still gets its Copy/Paste. Cmd+, opens Settings as a fallback in `resolve_action` unless the user bound it. Mouse multi-select and the rules editor's ⌘Enter read `modifiers.command()` (Ctrl+click is a right click on macOS) — prefer `command()` over `control()` / `logo()` for any new "primary modifier" check. `KeyCombo::display()` renders Apple's glyph order (⌃⌥⇧⌘) on macOS.

**Open a URL.** `src/update/about_modal.rs` spawns `xdg-open`; on macOS it is `open <url>`. Put the choice in one small helper (`#[cfg]` on the command name) and use it from every call site (`grep -rn 'xdg-open' src`).

**File picker.** `rfd` uses `NSOpenPanel` on macOS regardless of the `xdg-portal` feature (that feature only selects the Linux backend). Verify: the async dialog opens while called from an iced `Task` (rfd dispatches to the main thread itself; confirm it doesn't deadlock), and the case-sensitive glob filters in `custom_artwork.rs` (written for the portal) match `.JPG`/`.png` on macOS, which filters by extension.

**Hostname.** `data/src/services/api_key.rs::host_name` reads `/proc/sys/kernel/hostname` then `/etc/hostname`; neither exists on macOS, so the player name loses its host suffix. Use `libc::gethostname` (libc is already a dependency of the root crate; `data/` would need it added → ask) or `std::process::Command::new("scutil").args(["--get", "ComputerName"])`, behind `#[cfg(target_os = "macos")]`, keeping the 64-char trim.

## Testing

- Handler tests in `src/update/tests/{mpris,window,notifications}.rs` assert state and must pass on macOS untouched; a macOS-only state path (Dock reopen, hide instead of destroy) gets its own `#[cfg(target_os = "macos")]` test there.
- Things the owner must check in the running app (say so in the handoff): media keys and Control Center scrubber, the menu-bar icon in light and dark menu bars, a notification appearing (bundle only), close → Dock click → window back, Cmd+Q/W/H/M, Cmd+C/V in the search field.
