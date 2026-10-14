---
paths:
  - "src/views/**"
  - "src/widgets/**"
  - "src/app_view.rs"
  - "src/update/theater.rs"
  - "src/state/theater.rs"
---

# Gotchas: Widget Tree & Focus

- **Widget tree stability**: changing the root widget type (Row→Column) destroys `text_input` focus. Use `base_slot_list_empty_state` for consistent structure. The same holds for any wrapper on the path to the input: a view-header cell whose divider can flip between renders goes through `divided_header_cell` (the divider collapses to zero width), because the Queue's search-driven count label once hid the count cell mid-type and blurred the field after one keystroke (`search_input_survives_the_count_cell_hiding`).
- **Search input ID collisions**: each view needs a unique search input ID constant.
- **Focus a text input through `widgets::focus_text_input`** (Tab: `focus_adjacent_text_input`): iced 0.15's `operation::focus` leaves the caret where the input's state has it, and an input the view rebuilt with text in it (a view switch, a fresh login screen) has it at the front, so `/` and then typing landed before the query. The helpers chain `move_cursor_to_end`. `text_inputs_take_focus_with_the_caret_at_the_end` fails on a bare focus call anywhere else; the two left are `unfocus_all` and the rules editor's JSON `text_editor`, whose caret lives in app state.
- **`Length::Fill` stripe in unconstrained Row**: `container(Space).height(Fill)` in a row without explicit height expands to fill column space. Set `height(Shrink)` on the wrapper row.
- **Theater Mode entry and exit are unmount edges**: entering clears everything the unmounted widgets' `on_exit` / `on_close` would have cleared (`enter_theater` is the inventory); leaving clears the bar/corner hover flags and `open_menu`. Keep the theater `Stack` at a fixed layer count (`Space::new()` placeholders; `Stack::push` drops `void()` / `None` children), because `Stack` diffs by position. A `Column` spacer cannot translate a `Fixed`-height child (the child is squashed, and `pin` squashes too): use `OverflowPin`. Never give a moving element's `mouse_area` an `on_move`: it publishes whenever the BOUNDS change under a parked cursor, and a per-frame slide then loops iced's redraw ("3 consecutive RedrawRequested events produced invalidation").
