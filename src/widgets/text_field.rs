//! `text_field`: iced's `text_input` that keeps its font when the app
//! replaces its text.
//!
//! Why: at the pinned iced rev, `text_input::layout` overwrites the editor
//! buffer whenever the `value` prop differs from what its state holds, and
//! `Editor::overwrite` writes the new text with DEFAULT attributes (no font
//! family). The font is re-applied only when the font itself changes, so a
//! value the app sets while the field stays mounted (a search cleared by its
//! X button, a nudge, a reload, a normalize on Enter) is drawn in the system
//! sans-serif from then on, and so is everything typed after it. A value the
//! user types never takes that path: the app echoes it back unchanged.
//!
//! The fix here: right before the input lays out, ask it for the text it
//! holds. When that is not the `value` it is about to be handed, an overwrite
//! is coming, so the input lays out once in another font and then in its own.
//! Both passes change the font, which makes iced re-apply it to the buffer
//! the overwrite just reset. The input's state is never rebuilt, so focus,
//! caret, selection and an IME composition all stay.
//!
//! Every text input in nokkvi is built through [`text_field`]
//! (`every_text_input_is_a_text_field` scans the sources for a bare one).
//!
//! Upstream fixed the cause in iced-rs/iced cd04b74c (2026-10-21, "Keep
//! `Font` on `Editor::overwrite`"). Once the pinned rev has it,
//! `a_bare_text_input_loses_its_font_to_an_outside_value` fails: delete this
//! wrapper's two-pass layout then (or the whole wrapper).

use std::borrow::Cow;

use iced::{
    Event, Font, Length, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer, text,
        widget::{
            Id, Operation, Tree,
            operation::{self},
            tree,
        },
    },
    widget::{TextInput, text_input},
};

/// A `text_input` in `font`: see the module docs. `configure` sets everything
/// else (id, handlers, size, padding, style); the font is the wrapper's, so a
/// `.font(..)` inside `configure` is overridden.
pub(crate) fn text_field<'a, Message, Theme>(
    placeholder: impl text::IntoFragment<'a>,
    value: impl text::IntoFragment<'a>,
    font: Font,
    configure: impl FnOnce(TextInput<'a, Message, Theme>) -> TextInput<'a, Message, Theme>,
) -> TextField<'a, Message, Theme>
where
    Message: Clone,
    Theme: text_input::Catalog,
{
    let placeholder = placeholder.into_fragment();
    let value = value.into_fragment();
    let input = configure(TextInput::new(placeholder.clone(), value.clone())).font(font);
    TextField {
        input: Some(input),
        font,
        value,
        placeholder,
    }
}

/// See [`text_field`].
pub(crate) struct TextField<'a, Message, Theme = iced::Theme>
where
    Theme: text_input::Catalog,
{
    /// `None` only inside `layout`, while the input changes fonts.
    input: Option<TextInput<'a, Message, Theme>>,
    font: Font,
    value: Cow<'a, str>,
    placeholder: Cow<'a, str>,
}

/// The value the input was last laid out with.
#[derive(Default)]
struct Laid {
    value: String,
}

/// Reads the text a `text_input` holds. An empty input reports its
/// placeholder (`operation::TextInput::text`).
struct ReadText(Option<String>);

impl Operation for ReadText {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text_input(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        state: &mut dyn operation::TextInput,
    ) {
        self.0 = Some(state.text().into_owned());
    }
}

/// Whether the input is about to overwrite its buffer: the text it holds is
/// not the `value` it is being handed. `held` is what the input reports (its
/// placeholder while empty), `laid` the value of the previous layout.
///
/// An empty `value` can't be told from the report alone (an empty buffer and
/// one holding the placeholder's own text read the same), so a `value` that
/// just became empty counts too. A false "yes" only costs two cheap layouts.
fn overwrite_pending(held: &str, placeholder: &str, value: &str, laid: &str) -> bool {
    if value.is_empty() {
        !laid.is_empty() || (!held.is_empty() && held != placeholder)
    } else {
        held != value
    }
}

/// A font that is not `font`, for the first of the two layout passes.
fn other_font(font: Font) -> Font {
    if font == Font::MONOSPACE {
        Font::DEFAULT
    } else {
        Font::MONOSPACE
    }
}

impl<Message, Theme> iced::advanced::widget::Meta for TextField<'_, Message, Theme> where
    Theme: text_input::Catalog
{
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for TextField<'_, Message, Theme>
where
    Message: Clone,
    Theme: text_input::Catalog,
    Renderer: text::Renderer + 'static,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Laid>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Laid::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        if let Some(input) = self.input.as_mut() {
            tree.diff_children::<Message, Theme, Renderer>(std::slice::from_mut(input));
        }
    }

    fn size(&self) -> Size<Length> {
        self.input.as_ref().map_or(
            Size::new(Length::Shrink, Length::Shrink),
            Widget::<Message, Theme, Renderer>::size,
        )
    }

    // The input fills our bounds exactly (zero translation, same size), so it
    // shares our `Layout` everywhere below.
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let Some(mut input) = self.input.take() else {
            return;
        };
        let child = &mut tree.children[0];

        let mut held = ReadText(None);
        Widget::<Message, Theme, Renderer>::operate(
            &mut input,
            child,
            Layout::new(child.size),
            &Rectangle::with_size(child.size),
            renderer,
            &mut held,
        );
        let laid = tree.state.downcast_mut::<Laid>();
        let pending = held.0.as_deref().is_some_and(|held| {
            overwrite_pending(held, &self.placeholder, &self.value, &laid.value)
        });
        if laid.value != self.value {
            laid.value.clear();
            laid.value.push_str(&self.value);
        }

        if pending {
            // The overwrite happens inside this pass and resets the buffer's
            // attributes; the font differing from the editor's own makes iced
            // write it back, and the pass below does the same with ours.
            input = input.font(other_font(self.font));
            Widget::<Message, Theme, Renderer>::layout(&mut input, child, renderer, limits);
            input = input.font(self.font);
        }
        Widget::<Message, Theme, Renderer>::layout(&mut input, child, renderer, limits);
        tree.size = child.size;
        self.input = Some(input);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if let Some(input) = self.input.as_mut() {
            Widget::<Message, Theme, Renderer>::operate(
                input,
                &mut tree.children[0],
                layout,
                viewport,
                renderer,
                operation,
            );
        }
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
        if let Some(input) = self.input.as_mut() {
            Widget::<Message, Theme, Renderer>::update(
                input,
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                shell,
                viewport,
            );
        }
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
        if let Some(input) = self.input.as_ref() {
            Widget::<Message, Theme, Renderer>::draw(
                input,
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.input
            .as_ref()
            .map_or_else(mouse::Interaction::default, |input| {
                Widget::<Message, Theme, Renderer>::mouse_interaction(
                    input,
                    &tree.children[0],
                    layout,
                    cursor,
                    viewport,
                    renderer,
                )
            })
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
        match self.input.as_mut() {
            Some(input) => Widget::<Message, Theme, Renderer>::overlay(
                input,
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
                window,
            ),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use iced::{
        Background, Color, Point, Transformation,
        advanced::{image, text::Text},
        font::Family,
    };

    use super::*;

    /// The editor and paragraph the real renderer draws text inputs with.
    type RealEditor = <iced::Renderer as text::Renderer>::Editor;
    type RealParagraph = <iced::Renderer as text::Renderer>::Paragraph;

    /// A renderer that draws nothing and notes, for every editor it is asked
    /// to fill, the font family its first line carries. It shapes text with
    /// the real editor, so what it notes is what the real renderer would draw.
    #[derive(Default)]
    struct FamilyProbe {
        families: RefCell<Vec<String>>,
    }

    impl FamilyProbe {
        /// The family of the last editor drawn.
        fn last(&self) -> String {
            self.families
                .borrow()
                .last()
                .cloned()
                .expect("an editor was drawn")
        }
    }

    impl iced::advanced::Renderer for FamilyProbe {
        fn start_layer(&mut self, _bounds: Rectangle) {}

        fn end_layer(&mut self) {}

        fn start_transformation(&mut self, _transformation: Transformation) {}

        fn end_transformation(&mut self) {}

        fn fill_quad(&mut self, _quad: renderer::Quad, _background: impl Into<Background>) {}

        fn allocate_image(
            &self,
            _handle: &image::Handle,
            callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
        ) {
            callback(Err(image::Error::Unsupported));
        }

        fn hint(&mut self, _scale: renderer::Scale) {}

        fn scale(&self) -> Option<renderer::Scale> {
            None
        }

        fn reset(&mut self, _new_bounds: Rectangle) {}

        fn settings(&self) -> renderer::Settings {
            renderer::Settings::default()
        }
    }

    impl text::Renderer for FamilyProbe {
        type Paragraph = RealParagraph;
        type Editor = RealEditor;

        const ICON_FONT: Font = Font::DEFAULT;
        const CHECKMARK_ICON: char = '0';
        const ARROW_DOWN_ICON: char = '0';
        const SCROLL_UP_ICON: char = '0';
        const SCROLL_DOWN_ICON: char = '0';
        const SCROLL_LEFT_ICON: char = '0';
        const SCROLL_RIGHT_ICON: char = '0';
        const ICED_LOGO: char = '0';

        fn fill_paragraph(
            &mut self,
            _paragraph: &Self::Paragraph,
            _position: Point,
            _color: Color,
            _clip_bounds: Rectangle,
        ) {
        }

        fn fill_editor(
            &mut self,
            editor: &Self::Editor,
            _position: Point,
            _color: Color,
            _clip_bounds: Rectangle,
        ) {
            if let Some(line) = editor.buffer().lines.first() {
                self.families
                    .borrow_mut()
                    .push(format!("{:?}", line.attrs_list().defaults().family));
            }
        }

        fn fill_text(
            &mut self,
            _text: Text,
            _position: Point,
            _color: Color,
            _clip_bounds: Rectangle,
        ) {
        }
    }

    const APP_FAMILY: &str = "Nokkvi Probe Mono";
    const APP_FONT: Font = Font {
        family: Family::Name(APP_FAMILY),
        ..Font::DEFAULT
    };
    /// How the editor's first line reads in the app font, and after iced
    /// reset it.
    const IN_APP_FONT: &str = "Name(\"Nokkvi Probe Mono\")";
    const IN_FALLBACK: &str = "SansSerif";

    type Field<'a> = dyn Widget<String, iced::Theme, FamilyProbe> + 'a;

    /// A mounted field: its widget tree plus the renderer that watches it.
    struct Mounted {
        tree: Tree,
        probe: FamilyProbe,
    }

    impl Mounted {
        fn new(first: &mut Field<'_>) -> Self {
            let tree = Tree {
                tag: first.tag(),
                state: first.state(),
                translation: Vector::ZERO,
                size: Size::ZERO,
                children: Vec::new(),
            };
            let mut mounted = Self {
                tree,
                probe: FamilyProbe::default(),
            };
            mounted.show(first);
            mounted
        }

        /// What one frame does to a view the app rebuilt: diff, lay out,
        /// draw. Returns the family the text was drawn in.
        fn show(&mut self, field: &mut Field<'_>) -> String {
            field.diff(&mut self.tree);
            let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 40.0));
            field.layout(&mut self.tree, &self.probe, &limits);
            field.draw(
                &self.tree,
                &mut self.probe,
                &iced::Theme::Dark,
                &renderer::Style::default(),
                Layout::new(self.tree.size),
                mouse::Cursor::Unavailable,
                &Rectangle::with_size(self.tree.size),
            );
            self.probe.last()
        }
    }

    fn bare(value: &str) -> TextInput<'_, String> {
        iced::widget::text_input("Search...", value)
            .on_input(|typed| typed)
            .font(APP_FONT)
    }

    fn field(value: &str) -> TextField<'_, String> {
        text_field("Search...", value, APP_FONT, |input| {
            input.on_input(|typed| typed)
        })
    }

    /// The iced behaviour this module works around, pinned: a value replaced
    /// while the input stays mounted is drawn in the fallback family. When
    /// this fails, the pinned iced has the upstream fix (cd04b74c) and the
    /// two-pass layout can go.
    #[test]
    fn a_bare_text_input_loses_its_font_to_an_outside_value() {
        let mut first = bare("Hail 10");
        let mut mounted = Mounted::new(&mut first);
        assert_eq!(mounted.probe.last(), IN_APP_FONT, "a fresh mount is fine");

        assert_eq!(mounted.show(&mut bare("Road 20")), IN_FALLBACK);
    }

    #[test]
    fn a_text_field_keeps_its_font_when_the_app_replaces_its_value() {
        let mut first = field("Hail 10");
        let mut mounted = Mounted::new(&mut first);
        assert_eq!(mounted.probe.last(), IN_APP_FONT);

        assert_eq!(mounted.show(&mut field("Road 20")), IN_APP_FONT);
        assert_eq!(
            mounted.show(&mut field("Road 20")),
            IN_APP_FONT,
            "and the next frame"
        );
    }

    /// The search bar's X button: the value goes to empty with the field
    /// still mounted, and what is typed next lands in the reset buffer.
    #[test]
    fn a_text_field_cleared_by_the_app_keeps_its_font_for_what_comes_next() {
        let mut first = field("Hail 10");
        let mut mounted = Mounted::new(&mut first);
        mounted.show(&mut field(""));

        // The buffer is empty now; the next outside value fills it again.
        assert_eq!(mounted.show(&mut field("Hail 10")), IN_APP_FONT);

        // Control: the bare input ends up in the fallback the same way.
        let mut first = bare("Hail 10");
        let mut mounted = Mounted::new(&mut first);
        mounted.show(&mut bare(""));
        assert_eq!(mounted.show(&mut bare("Hail 10")), IN_FALLBACK);
    }

    #[test]
    fn an_overwrite_is_pending_when_the_held_text_is_not_the_value() {
        // (held, placeholder, value, laid)
        assert!(!overwrite_pending("abc", "Search...", "abc", "abc"));
        assert!(overwrite_pending("abc", "Search...", "abd", "abc"));
        assert!(
            overwrite_pending("Search...", "Search...", "abc", ""),
            "an empty buffer reports its placeholder"
        );
        // An empty value.
        assert!(!overwrite_pending("Search...", "Search...", "", ""));
        assert!(!overwrite_pending("", "", "", ""));
        assert!(overwrite_pending("abc", "Search...", "", "abc"));
        assert!(
            overwrite_pending("abc", "Search...", "", ""),
            "text the app never echoed"
        );
        assert!(
            overwrite_pending("Search...", "Search...", "", "Search..."),
            "the placeholder's own text, typed and then cleared"
        );
    }

    /// A bare `text_input` anywhere else brings the fallback font back for
    /// that field. The one allowed line is a test that reads the input's
    /// widget tag.
    #[test]
    fn every_text_input_is_a_text_field() {
        const ALLOWED: [&str; 1] = [
            "Tree::new::<String, iced::Theme, iced::Renderer>(&iced::widget::text_input(\"\", \"\"))",
        ];
        // A constructor call: `text_input(` not reached through a method or
        // another name (`operation.text_input(`, `focus_text_input(`), or
        // the struct's own `new`.
        fn builds_a_text_input(code: &str) -> bool {
            let call = concat!("text_", "input(");
            let by_new = concat!("Text", "Input::new(");
            code.contains(by_new)
                || code.match_indices(call).any(|(at, _)| {
                    !code[..at].chars().next_back().is_some_and(|before| {
                        before == '.' || before == '_' || before.is_alphanumeric()
                    })
                })
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let src = root.join("src");
        let own = root.join(file!());
        let mut stray = Vec::new();
        let mut dirs = vec![src];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path == own {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (i, line) in text.lines().enumerate() {
                    let code = line.trim_start();
                    if code.starts_with("//")
                        || !builds_a_text_input(code)
                        || ALLOWED.iter().any(|allowed| code.contains(allowed))
                    {
                        continue;
                    }
                    stray.push(format!("{}:{}: {code}", path.display(), i + 1));
                }
            }
        }
        stray.sort();
        assert!(
            stray.is_empty(),
            "build text inputs through widgets::text_field (a bare one loses its font when \
             the app replaces its value):\n{}",
            stray.join("\n")
        );

        assert!(builds_a_text_input(
            "let input = text_input(placeholder, query)"
        ));
        assert!(builds_a_text_input("iced::widget::text_input(\"\", value)"));
        assert!(builds_a_text_input("TextInput::new(placeholder, value)"));
        assert!(!builds_a_text_input(
            "operation.text_input(id, bounds, state)"
        ));
        assert!(!builds_a_text_input("focus_text_input(SEARCH_INPUT_ID)"));
    }

    #[test]
    fn the_other_font_is_never_the_fields_own() {
        for font in [Font::DEFAULT, Font::MONOSPACE, APP_FONT] {
            assert_ne!(other_font(font), font);
        }
    }
}
