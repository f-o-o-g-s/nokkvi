//! A wrapper that gives its child a fresh widget state whenever its key
//! changes, and keeps the child's state while the key stays the same.
//!
//! Why: iced's `text_input` re-reads its `value` in `layout` and, when it
//! differs from the text its state last held, overwrites the editor buffer
//! with default text attributes (`Editor::overwrite`, no font family). The
//! font is only re-applied when the font itself changes, so a value replaced
//! from OUTSIDE the field while it stays mounted (a curve drag, a nudge, a
//! reload) is drawn in the system sans-serif until the field is rebuilt. A
//! fresh state shapes its first text with the app font. Bump the key at
//! every outside change, never for the user's own keystrokes (a fresh state
//! has no focus and no caret).
//!
//! `iced::widget::keyed::Column` does not do this for a single child: its
//! diff only consults the keys when the child count changes, so a one-child
//! keyed column with a new key keeps the old state (pinned below).

use iced::{
    Event, Length, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
};

/// The key the child's state was built under.
struct MountKey(u64);

/// See the module docs: `content`'s state lives as long as `key` does.
pub(crate) struct Remount<W> {
    key: u64,
    content: W,
}

/// Wrap `content` so a change of `key` rebuilds its widget state.
pub(crate) fn remount<W>(key: u64, content: W) -> Remount<W> {
    Remount { key, content }
}

impl<W: iced::advanced::widget::Meta> iced::advanced::widget::Meta for Remount<W> {
    fn is_void(&self) -> bool {
        self.content.is_void()
    }
}

impl<W, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Remount<W>
where
    W: Widget<Message, Theme, Renderer>,
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<MountKey>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(MountKey(self.key))
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mounted = tree.state.downcast_mut::<MountKey>();
        if mounted.0 != self.key {
            mounted.0 = self.key;
            // Dropped here, rebuilt from scratch by `diff_children`.
            tree.children.clear();
        }
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.size()
    }

    // The child fills our bounds exactly (zero translation, same size), so it
    // shares our `Layout` everywhere below.
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content.layout(&mut tree.children[0], renderer, limits);
        tree.size = tree.children[0].size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .operate(&mut tree.children[0], layout, viewport, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

#[cfg(test)]
mod tests {
    use iced::advanced::widget::{Tree, tree};

    use super::*;

    /// A leaf whose state is a number the test writes, so a kept state and a
    /// fresh one tell apart.
    struct Probe;

    struct ProbeState(u32);

    impl iced::advanced::widget::Meta for Probe {}

    impl Widget<(), iced::Theme, iced::Renderer> for Probe {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<ProbeState>()
        }

        fn state(&self) -> tree::State {
            tree::State::new(ProbeState(0))
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Shrink, Length::Shrink)
        }

        fn layout(&mut self, _: &mut Tree, _: &iced::Renderer, _: &layout::Limits) {}

        fn draw(
            &self,
            _: &Tree,
            _: &mut iced::Renderer,
            _: &iced::Theme,
            _: &renderer::Style,
            _: Layout,
            _: mouse::Cursor,
            _: &Rectangle,
        ) {
        }
    }

    fn mark(child: &mut Tree, value: u32) {
        child.state.downcast_mut::<ProbeState>().0 = value;
    }

    fn mark_of(child: &Tree) -> u32 {
        child.state.downcast_ref::<ProbeState>().0
    }

    #[test]
    fn a_new_key_rebuilds_the_childs_state_and_the_same_key_keeps_it() {
        let mut first = remount(1, Probe);
        let mut tree = Tree::new(&first);
        tree.diff(&mut first);
        mark(&mut tree.children[0], 7);

        tree.diff(&mut remount(1, Probe));
        assert_eq!(mark_of(&tree.children[0]), 7, "same key: state kept");

        tree.diff(&mut remount(2, Probe));
        assert_eq!(mark_of(&tree.children[0]), 0, "new key: a fresh state");
        mark(&mut tree.children[0], 9);
        tree.diff(&mut remount(2, Probe));
        assert_eq!(mark_of(&tree.children[0]), 9, "and that one is kept");
    }

    /// Why `Remount` exists: at the pinned iced rev a one-child keyed column
    /// keeps its child's state across a key change (its diff searches by key
    /// only when the child count changes). If this starts failing, iced
    /// fixed it and a keyed column could do `Remount`'s job.
    #[test]
    fn a_one_child_keyed_column_keeps_its_childs_state_across_a_key_change() {
        let mut first = iced::widget::keyed_column([(1u64, Probe)]);
        let mut tree = Tree::new(&first);
        tree.diff(&mut first);
        mark(&mut tree.children[0], 7);

        tree.diff(&mut iced::widget::keyed_column([(2u64, Probe)]));
        assert_eq!(mark_of(&tree.children[0]), 7);
    }
}
