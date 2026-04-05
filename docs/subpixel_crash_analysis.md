# Analysis: `iced` Sub-pixel Image Crash vs Nokkvi Resizing Logic

## Executive Summary
This document analyzes the root cause of `wgpu` buffer validation crashes occurring in Nokkvi when the application window is resized to extremely narrow widths. 

**Conclusion: This is an upstream `iced` framework bug that cannot be fully fixed on the Nokkvi side.** Nokkvi-side mitigations reduce the likelihood but cannot prevent the crash entirely because iced's rendering pipeline (`State::prepare`) executes independently of the application's message cycle.

## Part 1: The Upstream `iced` Bug (Commits `0fe99b19` & `1463ec84`)
The `iced` developers correctly recognized that sending 0-sized or microscopic image bounds to the `wgpu` atlas renderer causes crashes. To prevent this, they added bounding guards in `wgpu/src/image/mod.rs` (commits `0fe99b19` and `1463ec84`).

However, the implementation of that guard introduces a new logic bug:

```rust
pub fn prepare(...) {
    // ... setup ...
    for image in images {
        // The Guard -> The Bug
        if bounds.width < 1.0 || bounds.height < 1.0 {
            return; // <--- This is the flaw
        }
        
        // ... build instance arrays for the image ...
    }

    // --- Cleanup and GPU finalization ---
    layer.push(...);
    layer.prepare(...);
    
    // CRITICAL: Clear the buffer for the next frame
    self.nearest_instances.clear();
    self.linear_instances.clear();
}
```

### Why `continue` is the correct upstream fix
When a sub-pixel image triggers the `return` statement:
1. It immediately aborts the `State::prepare` function.
2. The finalization sequence (`layer.push()` and `layer.prepare()`) is bypassed.
3. Most critically, the buffer clearing (`self.nearest_instances.clear()`) is skipped.

On the next frame, the renderer appends *new* instances onto the stale leftovers from the aborted frame. This causes a length mismatch when writing the buffer, immediately triggering a `wgpu` validation panic.

The upstream PR ([#3292](https://github.com/iced-rs/iced/pull/3292), fixing [#3272](https://github.com/iced-rs/iced/issues/3272)) correctly changes `return` to `continue`. This successfully drops the problematic sub-pixel image (fulfilling the original author's intent) but allows the `for` loop to finish and the critical cleanup code to execute.

## Part 2: The Nokkvi Resizing Flaw
While the `iced` framework shouldn't crash when receiving bad sizes, Nokkvi's layout engine contributes by generating sub-pixel image dimensions inside `src/widgets/base_slot_list_layout.rs`.

Nokkvi relies on `iced::widget::responsive` to adapt its artwork columns dynamically based on the window's bounding box:

```rust
// For single images
let square_size = size.width.min(size.height).max(0.0);

// For collage images
let cell_size = square_size / 3.0; 
```

Because `size.width` shrinks incrementally as a floating-point number, `square_size` easily drops to sub-pixel values (e.g., `0.75`). In the case of collages, an available width of `2.0` pixels is divided by three, resulting in a `0.66` pixel `cell_size`. 

Nokkvi then feeds these dimensions blindly into `image().width(Length::Fixed(square_size))`, activating the upstream bug.

## Part 3: Why Nokkvi-Side Fixes Cannot Fully Prevent the Crash

### Attempted mitigations (applied but insufficient)

1. **Artwork panel guards** (`base_slot_list_layout.rs`): Return `Space` instead of `image()` when dimensions drop below 1px. Prevents sub-pixel artwork images but doesn't cover SVGs elsewhere.

2. **Root view fallback** (`app_view.rs`): When `self.window.width` or `height` drops below 200px, the full widget tree is replaced with an empty container. Prevents most SVG/image rendering at extreme sizes.

3. **`min_size` window hint**: Rejected — Hyprland (and tiling WMs generally) ignore Wayland `min_size` constraints when tiling windows.

### Why these mitigations still fail

The root view guard reads `self.window.width` from state that is updated via the subscription-based `Message::WindowResized` pipeline:

```
Wayland configure event
  → winit delivers Resized event
  → iced event::listen_with subscription  
  → Message::WindowResized(w, h) queued
  → update() sets self.window.width = w
  → view() called with updated state
```

However, iced's **internal rendering pipeline** (`State::prepare`) runs during the layout/render phase that occurs *in response to the Wayland configure*, potentially **before** the `WindowResized` message is dispatched through the application's subscription system. This means:

- The old `view()` tree (with full SVG buttons at non-sub-pixel sizes) is still active
- But iced is rendering it at the **new, smaller** physical dimensions  
- The `bounds * scale` calculation in `prepare()` produces sub-pixel values
- The `return` fires and strands GPU buffers

The application's `view()` function simply cannot react fast enough — the crash happens inside iced's own frame-drawing code, not in user-controlled widget construction.

## Conclusion and Recommendations

### The upstream PR must be merged
PR [#3292](https://github.com/iced-rs/iced/pull/3292) is the **only complete fix**. It corrects a real defect in iced's rendering pipeline where a skipped loop element (`return` instead of `continue`) orphans GPU instance buffers. This affects any iced application that has images or SVGs that shrink below 1px — not just Nokkvi.

### Nokkvi-side mitigations (keep, but they are best-effort)
The artwork panel guards and root view fallback remain useful:
- They prevent unnecessary sub-pixel rendering even when the upstream fix lands
- They provide graceful degradation at extreme window sizes
- They reduce (but do not eliminate) crash frequency before the upstream fix is merged

### Nokkvi is NOT using a patched iced
Nokkvi pins `iced` at upstream rev `12a01265`, which contains the buggy `return` statements. No `[patch]` section is used. The fix commit `8d69450c` exists only on the `fork/fix/image-prepare-continue` branch and is not consumed by Nokkvi's build.

### Summary
Keep the upstream `iced` PR alive — it is the only complete fix. The Nokkvi-side mitigations are defensive coding that reduce exposure but cannot prevent the crash on tiling window managers. The crash will persist until either:
1. The upstream PR is merged and Nokkvi bumps its iced pin, or
2. Nokkvi switches to a `[patch]` section pointing at the fork
