//! A box to type text in: one line that grows into several as the text wraps or is given
//! line breaks.

use std::{
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla,
    InspectorElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, Point, ScrollHandle, SharedString, Style, Task, TextAlign,
    TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill, point,
    prelude::*, relative, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::theme::ActiveTheme;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteToLineStart,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        Start,
        End,
        SelectStart,
        SelectEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        /// Start a new line.
        Newline,
        /// Hand the text over: Enter.
        Submit,
        ShowCharacterPalette,
        /// While the owner offers choices for what is being typed: move among them, take
        /// one, or put them away.
        PreviousChoice,
        NextChoice,
        AcceptChoice,
        DismissChoices,
    ]
);

const CONTEXT: &str = "TextInput";
/// Added to the key context while the owner offers choices.
const CHOOSING: &str = "choosing";
/// How long the caret shows and hides for as it blinks.
const BLINK: Duration = Duration::from_millis(530);
/// Typing with pauses shorter than this is undone in one step.
const UNDO_TOGETHER: Duration = Duration::from_secs(1);
/// The most steps kept to undo.
const UNDO_STEPS: usize = 200;

pub(super) fn bind_keys(cx: &mut App) {
    let choosing = format!("{CONTEXT} && {CHOOSING}");
    let choosing = Some(choosing.as_str());
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("alt-backspace", DeleteWordLeft, Some(CONTEXT)),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, Some(CONTEXT)),
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("up", Up, Some(CONTEXT)),
        KeyBinding::new("down", Down, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("alt-left", WordLeft, Some(CONTEXT)),
        KeyBinding::new("alt-right", WordRight, Some(CONTEXT)),
        KeyBinding::new("alt-shift-left", SelectWordLeft, Some(CONTEXT)),
        KeyBinding::new("alt-shift-right", SelectWordRight, Some(CONTEXT)),
        KeyBinding::new("cmd-left", LineStart, Some(CONTEXT)),
        KeyBinding::new("cmd-right", LineEnd, Some(CONTEXT)),
        KeyBinding::new("home", LineStart, Some(CONTEXT)),
        KeyBinding::new("end", LineEnd, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-left", SelectLineStart, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, Some(CONTEXT)),
        KeyBinding::new("cmd-up", Start, Some(CONTEXT)),
        KeyBinding::new("cmd-down", End, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-up", SelectStart, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-down", SelectEnd, Some(CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-x", Cut, Some(CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        KeyBinding::new("cmd-z", Undo, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-z", Redo, Some(CONTEXT)),
        KeyBinding::new("shift-enter", Newline, Some(CONTEXT)),
        KeyBinding::new("alt-enter", Newline, Some(CONTEXT)),
        KeyBinding::new("enter", Submit, Some(CONTEXT)),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some(CONTEXT)),
        KeyBinding::new("up", PreviousChoice, choosing),
        KeyBinding::new("down", NextChoice, choosing),
        KeyBinding::new("enter", AcceptChoice, choosing),
        KeyBinding::new("tab", AcceptChoice, choosing),
        KeyBinding::new("escape", DismissChoices, choosing),
    ]);
}

/// What a [`TextInput`] tells whoever owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The text or the place typing goes changed.
    Changed,
    /// Enter was pressed.
    Submitted,
    /// A key meant for the choices its owner is offering.
    Choice(Choice),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Previous,
    Next,
    Accept,
    Dismiss,
}

/// The text and selection at one moment, to go back to.
#[derive(Clone, Debug, PartialEq)]
struct Moment {
    text: String,
    selection: Range<usize>,
}

/// The text as last drawn: its lines, and where they are.
struct Drawn {
    text: String,
    /// One for each line of the text, which wraps into rows.
    lines: Rc<Vec<WrappedLine>>,
    line_height: Pixels,
    bounds: Bounds<Pixels>,
}

/// One row of a line as it wraps: the part of the line it holds, and how far along the
/// unwrapped line it starts.
struct Row {
    range: Range<usize>,
    start_x: Pixels,
}

fn rows(line: &WrappedLine) -> Vec<Row> {
    let breaks = line.wrap_boundaries().iter().map(|boundary| {
        let glyph = &line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix];
        (glyph.index, glyph.position.x)
    });
    let mut rows = Vec::new();
    let mut start = (0, Pixels::ZERO);
    for next in breaks.chain([(line.len(), Pixels::ZERO)]) {
        rows.push(Row {
            range: start.0..next.0,
            start_x: start.1,
        });
        start = next;
    }
    rows
}

impl Drawn {
    /// Each line with where in the text it starts and how far down it is drawn.
    fn lines(&self) -> impl Iterator<Item = (&WrappedLine, usize, Pixels)> {
        let mut start = 0;
        let mut top = Pixels::ZERO;
        self.lines.iter().map(move |line| {
            let placed = (line, start, top);
            start += line.len() + 1;
            top += line.size(self.line_height).height;
            placed
        })
    }

    fn height(&self) -> Pixels {
        let lines = self.lines.iter();
        lines.map(|line| line.size(self.line_height).height).sum()
    }

    /// Where `index` is drawn, from the top left of the text.
    fn position(&self, index: usize) -> Point<Pixels> {
        let placed = self.lines().take_while(|(_, start, _)| *start <= index);
        let Some((line, start, top)) = placed.last() else {
            return Point::default();
        };
        let within = (index - start).min(line.len());
        let place = line.position_for_index(within, self.line_height);
        let place = place.unwrap_or_default();
        point(place.x, place.y + top)
    }

    /// The place in the text nearest to `position`, from the top left of the text.
    fn index(&self, position: Point<Pixels>) -> usize {
        if position.y < Pixels::ZERO {
            return 0;
        }
        for (line, start, top) in self.lines() {
            let bottom = top + line.size(self.line_height).height;
            if position.y < bottom {
                let within = point(position.x.max(Pixels::ZERO), position.y - top);
                let index = line.closest_index_for_position(within, self.line_height);
                return start + index.unwrap_or_else(|index| index);
            }
        }
        self.text.len()
    }

    /// The rectangles behind `range`, one for each row it touches.
    fn rectangles(&self, range: &Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut rectangles = Vec::new();
        for (line, start, top) in self.lines() {
            for (number, row) in rows(line).into_iter().enumerate() {
                let (from, to) = (start + row.range.start, start + row.range.end);
                if range.end < from || range.start > to {
                    continue;
                }
                let x = |index: usize| {
                    line.unwrapped_layout
                        .x_for_index(index.clamp(from, to) - start)
                        - row.start_x
                };
                let y = top + self.line_height * number as f32;
                rectangles.push(Bounds::from_corners(
                    point(x(range.start), y),
                    point(x(range.end), y + self.line_height),
                ));
            }
        }
        rectangles
    }
}

/// A box to type text in. It grows with its text, and scrolls once it is `most_rows` tall.
pub struct TextInput {
    text: String,
    /// What is selected; empty where typing goes.
    selection: Range<usize>,
    /// Whether the selection grows from its end towards its start.
    reversed: bool,
    /// Text an input method is still composing.
    marked: Option<Range<usize>>,
    /// Parts drawn in the accent colour, such as the names of people mentioned.
    accented: Vec<Range<usize>>,
    placeholder: SharedString,
    most_rows: usize,
    focus_handle: FocusHandle,
    scroll: ScrollHandle,
    drawn: Option<Drawn>,
    /// Whether the pointer is dragging out a selection.
    selecting: bool,
    /// Whether the owner offers choices for what is being typed.
    choosing: bool,
    /// How far along its row the caret wants to be while moving up and down.
    column: Option<Pixels>,
    caret_shown: bool,
    blink: Task<()>,
    /// The caret is to be scrolled into view when next drawn.
    reveal_caret: bool,
    undo: Vec<Moment>,
    redo: Vec<Moment>,
    typed: Option<Instant>,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            selection: 0..0,
            reversed: false,
            marked: None,
            accented: Vec::new(),
            placeholder: placeholder.into(),
            most_rows: usize::MAX,
            focus_handle: cx.focus_handle().tab_index(0).tab_stop(true),
            scroll: ScrollHandle::new(),
            drawn: None,
            selecting: false,
            choosing: false,
            column: None,
            caret_shown: true,
            blink: Task::ready(()),
            reveal_caret: false,
            undo: Vec::new(),
            redo: Vec::new(),
            typed: None,
        }
    }

    /// Scrolls instead of growing past this many rows.
    pub fn most_rows(mut self, rows: usize) -> Self {
        self.most_rows = rows;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where typing goes: the moving end of the selection.
    pub fn caret(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    /// Replaces all the text, as when a message has been sent, with the caret at its end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = text.into();
        self.selection = self.text.len()..self.text.len();
        self.reversed = false;
        self.marked = None;
        self.accented.clear();
        self.undo.clear();
        self.redo.clear();
        self.changed(cx);
    }

    /// Replaces part of the text and puts the caret after what was put in.
    pub fn replace(&mut self, range: Range<usize>, with: &str, cx: &mut Context<Self>) {
        self.remember(false);
        self.splice(range, with);
        self.changed(cx);
    }

    /// Draws these parts of the text in the accent colour. They move with the text as it is
    /// edited, and one that is typed into is dropped.
    pub fn set_accented(&mut self, ranges: Vec<Range<usize>>, cx: &mut Context<Self>) {
        self.accented = ranges;
        cx.notify();
    }

    pub fn accented(&self) -> &[Range<usize>] {
        &self.accented
    }

    /// Says whether the owner offers choices for what is being typed. While it does, Up,
    /// Down, Enter, Tab and Escape go to it as [`TextInputEvent::Choice`].
    pub fn set_choosing(&mut self, choosing: bool, cx: &mut Context<Self>) {
        if self.choosing != choosing {
            self.choosing = choosing;
            cx.notify();
        }
    }

    /// Where the caret is drawn in the window, once the text has been drawn.
    pub fn caret_bounds(&self) -> Option<Bounds<Pixels>> {
        let drawn = self.drawn.as_ref()?;
        let place = drawn.position(self.caret().min(drawn.text.len()));
        Some(Bounds::new(
            drawn.bounds.origin + place,
            size(Pixels::ZERO, drawn.line_height),
        ))
    }

    /// Puts `with` in place of `range`, keeping the accented parts where they belong.
    fn splice(&mut self, range: Range<usize>, with: &str) {
        self.text.replace_range(range.clone(), with);
        let grew = with.len() as isize - range.len() as isize;
        self.accented.retain_mut(|accented| {
            if accented.end <= range.start {
                true
            } else if accented.start >= range.end {
                accented.start = accented.start.saturating_add_signed(grew);
                accented.end = accented.end.saturating_add_signed(grew);
                true
            } else {
                // Typed into: it no longer says what it did.
                false
            }
        });
        let caret = range.start + with.len();
        self.selection = caret..caret;
        self.reversed = false;
    }

    /// Keeps the text as it is now to undo back to. Typing in a run is kept as one step.
    fn remember(&mut self, typing: bool) {
        let now = Instant::now();
        let together = typing
            && self
                .typed
                .is_some_and(|typed| now.duration_since(typed) < UNDO_TOGETHER);
        self.typed = typing.then_some(now);
        self.redo.clear();
        if together {
            return;
        }
        let moment = self.moment();
        if self.undo.last() != Some(&moment) {
            self.undo.push(moment);
            if self.undo.len() > UNDO_STEPS {
                self.undo.remove(0);
            }
        }
    }

    fn moment(&self) -> Moment {
        Moment {
            text: self.text.clone(),
            selection: self.selection.clone(),
        }
    }

    fn restore(&mut self, moment: Moment, cx: &mut Context<Self>) {
        self.text = moment.text;
        self.selection = moment.selection;
        self.reversed = false;
        self.marked = None;
        // The accented parts were for other text.
        self.accented.clear();
        self.typed = None;
        self.changed(cx);
    }

    /// After the text changed.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.column = None;
        self.show_caret(cx);
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// Keeps the caret lit and in view after it moved, and starts its blinking over.
    fn show_caret(&mut self, cx: &mut Context<Self>) {
        self.caret_shown = true;
        self.reveal_caret = true;
        self.blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let blinked = this.update(cx, |this, cx| {
                    this.caret_shown = !this.caret_shown;
                    cx.notify();
                });
                if blinked.is_err() {
                    return;
                }
            }
        });
    }

    fn move_to(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selection = index..index;
        self.reversed = false;
        self.moved(cx);
    }

    fn select_to(&mut self, index: usize, cx: &mut Context<Self>) {
        let anchor = if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        self.reversed = index < anchor;
        self.selection = index.min(anchor)..index.max(anchor);
        self.moved(cx);
    }

    /// After the caret moved without the text changing.
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.typed = None;
        self.show_caret(cx);
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// Moves the caret to `index`, or the selection's end there when `select`.
    fn go(&mut self, index: usize, select: bool, cx: &mut Context<Self>) {
        self.column = None;
        if select {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn before(&self, index: usize) -> usize {
        let before = self.text[..index].grapheme_indices(true).next_back();
        before.map_or(0, |(start, _)| start)
    }

    fn after(&self, index: usize) -> usize {
        let after = self.text[index..].graphemes(true).next();
        index + after.map_or(0, str::len)
    }

    fn word_before(&self, index: usize) -> usize {
        let mut words = self.text[..index].unicode_word_indices();
        words.next_back().map_or(0, |(start, _)| start)
    }

    fn word_after(&self, index: usize) -> usize {
        let word = self.text[index..].unicode_word_indices().next();
        word.map_or(self.text.len(), |(start, word)| index + start + word.len())
    }

    /// The start and end of the row the caret is on, as drawn; of its line before that.
    fn row_ends(&self) -> (usize, usize) {
        let caret = self.caret();
        if let Some(drawn) = self.drawn.as_ref().filter(|drawn| drawn.text == self.text) {
            for (line, start, _) in drawn.lines() {
                if caret > start + line.len() {
                    continue;
                }
                let rows = rows(line);
                let row = rows.iter().find(|row| caret - start <= row.range.end);
                if let Some(row) = row {
                    return (start + row.range.start, start + row.range.end);
                }
            }
        }
        let start = self.text[..caret].rfind('\n').map_or(0, |at| at + 1);
        let end = self.text[caret..]
            .find('\n')
            .map_or(self.text.len(), |at| caret + at);
        (start, end)
    }

    /// The place one row up or down from the caret, keeping to its column.
    fn row_away(&mut self, down: bool) -> usize {
        let Some(drawn) = self.drawn.as_ref().filter(|drawn| drawn.text == self.text) else {
            return if down { self.text.len() } else { 0 };
        };
        let place = drawn.position(self.caret());
        let column = *self.column.get_or_insert(place.x);
        let step = if down {
            drawn.line_height
        } else {
            -drawn.line_height
        };
        let y = place.y + step + drawn.line_height / 2.;
        if y > drawn.height() {
            return self.text.len();
        }
        drawn.index(point(column, y))
    }

    fn vertically(&mut self, down: bool, select: bool, cx: &mut Context<Self>) {
        let index = self.row_away(down);
        let column = self.column;
        self.go(index, select, cx);
        self.column = column;
    }

    fn erase(&mut self, to: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            if to == self.caret() {
                return;
            }
            self.select_to(to, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn selected(&self) -> Option<ClipboardItem> {
        let selected = &self.text[self.selection.clone()];
        (!selected.is_empty()).then(|| ClipboardItem::new_string(selected.to_owned()))
    }

    fn index_at(&self, position: Point<Pixels>) -> usize {
        match &self.drawn {
            Some(drawn) if drawn.text == self.text => drawn.index(position - drawn.bounds.origin),
            _ => self.text.len(),
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let index = self.index_at(event.position);
        match event.click_count {
            1 => {
                self.selecting = true;
                self.go(index, event.modifiers.shift, cx);
            }
            2 => {
                let start = self.word_before(self.after(index).min(self.text.len()));
                self.move_to(start, cx);
                self.select_to(self.word_after(start), cx);
            }
            _ => {
                self.move_to(0, cx);
                self.select_to(self.text.len(), cx);
            }
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_at(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn utf16(&self, index: usize) -> usize {
        self.text[..index].encode_utf16().count()
    }

    fn index_of_utf16(&self, units: usize) -> usize {
        let mut seen = 0;
        for (index, character) in self.text.char_indices() {
            if seen >= units {
                return index;
            }
            seen += character.len_utf16();
        }
        self.text.len()
    }

    #[cfg(test)]
    fn length_utf16(&self) -> usize {
        self.utf16(self.text.len())
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.index_of_utf16(range.start)..self.index_of_utf16(range.end)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.utf16(range.start)..self.utf16(range.end));
        Some(self.text[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.utf16(self.selection.start)..self.utf16(self.selection.end),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let marked = self.marked.as_ref()?;
        Some(self.utf16(marked.start)..self.utf16(marked.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        if !range.is_empty() {
            // Typing over a selection starts a run of its own.
            self.typed = None;
        }
        self.remember(text.graphemes(true).count() == 1);
        self.splice(range, text);
        self.marked = None;
        self.changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.range_from_utf16(&range))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        if self.marked.is_none() {
            self.remember(false);
        }
        self.splice(range.clone(), text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        if let Some(selected) = selected_utf16 {
            let composed = &self.text[range.start..range.start + text.len()];
            let within = |units: usize| {
                let mut seen = 0;
                for (index, character) in composed.char_indices() {
                    if seen >= units {
                        return index;
                    }
                    seen += character.len_utf16();
                }
                composed.len()
            };
            self.selection =
                range.start + within(selected.start)..range.start + within(selected.end);
        }
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let drawn = self
            .drawn
            .as_ref()
            .filter(|drawn| drawn.text == self.text)?;
        let first = drawn.rectangles(&range).into_iter().next()?;
        Some(Bounds::new(first.origin + drawn.bounds.origin, first.size))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.utf16(self.index_at(point)))
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let most = theme.text.body.line_height * self.most_rows.min(1_000) as f32;
        let context = if self.choosing {
            format!("{CONTEXT} {CHOOSING}")
        } else {
            CONTEXT.to_owned()
        };
        let choice = |choice: Choice| {
            cx.listener(move |_, _: &dyn gpui::Action, _, cx| {
                cx.emit(TextInputEvent::Choice(choice));
            })
        };
        let focused = self.focus_handle.is_focused(window);
        if !focused {
            // Nothing blinks where nothing is typed.
            self.blink = Task::ready(());
            self.caret_shown = true;
        }

        div()
            .id("text-input")
            .key_context(context.as_str())
            .track_focus(&self.focus_handle)
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .max_h(most)
            .w_full()
            .cursor(CursorStyle::IBeam)
            .text_size(theme.text.body.size)
            .line_height(theme.text.body.line_height)
            .on_action(cx.listener(|this, _: &Backspace, window, cx| {
                this.erase(this.before(this.caret()), window, cx);
            }))
            .on_action(cx.listener(|this, _: &Delete, window, cx| {
                this.erase(this.after(this.caret()), window, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteWordLeft, window, cx| {
                this.erase(this.word_before(this.caret()), window, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteToLineStart, window, cx| {
                this.erase(this.row_ends().0, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Left, _, cx| {
                let to = match this.selection.is_empty() {
                    true => this.before(this.caret()),
                    false => this.selection.start,
                };
                this.go(to, false, cx);
            }))
            .on_action(cx.listener(|this, _: &Right, _, cx| {
                let to = match this.selection.is_empty() {
                    true => this.after(this.caret()),
                    false => this.selection.end,
                };
                this.go(to, false, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| {
                this.go(this.before(this.caret()), true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| {
                this.go(this.after(this.caret()), true, cx);
            }))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.vertically(false, false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.vertically(true, false, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.vertically(false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.vertically(true, true, cx)))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| {
                this.go(this.word_before(this.caret()), false, cx);
            }))
            .on_action(cx.listener(|this, _: &WordRight, _, cx| {
                this.go(this.word_after(this.caret()), false, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| {
                this.go(this.word_before(this.caret()), true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| {
                this.go(this.word_after(this.caret()), true, cx);
            }))
            .on_action(
                cx.listener(|this, _: &LineStart, _, cx| this.go(this.row_ends().0, false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &LineEnd, _, cx| this.go(this.row_ends().1, false, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| {
                this.go(this.row_ends().0, true, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| {
                this.go(this.row_ends().1, true, cx);
            }))
            .on_action(cx.listener(|this, _: &Start, _, cx| this.go(0, false, cx)))
            .on_action(cx.listener(|this, _: &End, _, cx| this.go(this.text.len(), false, cx)))
            .on_action(cx.listener(|this, _: &SelectStart, _, cx| this.go(0, true, cx)))
            .on_action(cx.listener(|this, _: &SelectEnd, _, cx| this.go(this.text.len(), true, cx)))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.move_to(0, cx);
                this.select_to(this.text.len(), cx);
            }))
            .on_action(cx.listener(|this, _: &Copy, _, cx| {
                if let Some(selected) = this.selected() {
                    cx.write_to_clipboard(selected);
                }
            }))
            .on_action(cx.listener(|this, _: &Cut, window, cx| {
                if let Some(selected) = this.selected() {
                    cx.write_to_clipboard(selected);
                    this.replace_text_in_range(None, "", window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Paste, window, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    this.replace_text_in_range(None, &text, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Undo, _, cx| {
                if let Some(moment) = this.undo.pop() {
                    this.redo.push(this.moment());
                    this.restore(moment, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Redo, _, cx| {
                if let Some(moment) = this.redo.pop() {
                    this.undo.push(this.moment());
                    this.restore(moment, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Newline, window, cx| {
                this.replace_text_in_range(None, "\n", window, cx);
            }))
            .on_action(cx.listener(|_, _: &Submit, _, cx| cx.emit(TextInputEvent::Submitted)))
            .on_action(cx.listener(|_, _: &ShowCharacterPalette, window, _| {
                window.show_character_palette();
            }))
            .on_action({
                let choice = choice(Choice::Previous);
                move |action: &PreviousChoice, window, cx| choice(action, window, cx)
            })
            .on_action({
                let choice = choice(Choice::Next);
                move |action: &NextChoice, window, cx| choice(action, window, cx)
            })
            .on_action({
                let choice = choice(Choice::Accept);
                move |action: &AcceptChoice, window, cx| choice(action, window, cx)
            })
            .on_action({
                let choice = choice(Choice::Dismiss);
                move |action: &DismissChoices, window, cx| choice(action, window, cx)
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(Text {
                input: cx.entity(),
                focused,
            })
    }
}

/// Draws the text of a [`TextInput`], its selection and its caret.
struct Text {
    input: Entity<TextInput>,
    focused: bool,
}

struct Prepared {
    lines: Rc<Vec<WrappedLine>>,
    selection: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
    caret_bounds: Bounds<Pixels>,
}

/// The text to draw and how: the placeholder when there is none.
fn shaped(
    input: &TextInput,
    width: Option<Pixels>,
    window: &mut Window,
    cx: &App,
) -> Vec<WrappedLine> {
    let colors = &cx.theme().colors;
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let run = |len: usize, color: Hsla, underlined: bool| TextRun {
        len,
        font: style.font(),
        color,
        background_color: None,
        underline: underlined.then(|| UnderlineStyle {
            color: Some(color),
            thickness: cx.theme().size.border,
            wavy: false,
        }),
        strikethrough: None,
    };
    let (text, runs) = if input.text.is_empty() {
        let placeholder = input.placeholder.clone();
        let runs = vec![run(placeholder.len(), colors.text_muted, false)];
        (placeholder, runs)
    } else {
        // Every place where the way the text is drawn changes.
        let mut edges = vec![0, input.text.len()];
        let marked = input.marked.clone().unwrap_or(0..0);
        edges.extend([marked.start, marked.end]);
        edges.extend(
            input
                .accented
                .iter()
                .flat_map(|range| [range.start, range.end]),
        );
        edges.sort_unstable();
        edges.dedup();
        let runs = edges
            .windows(2)
            .map(|edges| {
                let (start, end) = (edges[0], edges[1]);
                let within = |range: &Range<usize>| range.start <= start && end <= range.end;
                let color = match input.accented.iter().any(within) {
                    true => colors.text_accent,
                    false => colors.text,
                };
                run(end - start, color, within(&marked))
            })
            .collect();
        (SharedString::from(input.text.clone()), runs)
    };
    let lines = window
        .text_system()
        .shape_text(text, font_size, &runs, width, None);
    lines.map(|lines| lines.into_vec()).unwrap_or_default()
}

impl Element for Text {
    type RequestLayoutState = ();
    type PrepaintState = Prepared;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let input = self.input.clone();
        let line_height = window.line_height();
        let layout = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let lines = shaped(input.read(cx), width, window, cx);
            let height: Pixels = lines.iter().map(|line| line.size(line_height).height).sum();
            size(width.unwrap_or_default(), height.max(line_height))
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepared {
        let theme = cx.theme();
        let line_height = window.line_height();
        let input = self.input.read(cx);
        let lines = Rc::new(shaped(input, Some(bounds.size.width), window, cx));
        let drawn = Drawn {
            text: input.text.clone(),
            // With no text the lines are the placeholder's, and everything is at the start.
            lines: match input.text.is_empty() {
                true => Rc::default(),
                false => lines.clone(),
            },
            line_height,
            bounds,
        };
        let within = |rectangle: Bounds<Pixels>| {
            Bounds::new(rectangle.origin + bounds.origin, rectangle.size)
        };
        let selection = match input.selection.is_empty() {
            true => Vec::new(),
            false => drawn
                .rectangles(&input.selection)
                .into_iter()
                .map(|rectangle| fill(within(rectangle), theme.colors.selection))
                .collect(),
        };
        let caret_bounds = within(Bounds::new(
            drawn.position(input.caret()),
            size(theme.size.caret, line_height),
        ));
        let caret = (self.focused && input.caret_shown && input.selection.is_empty())
            .then(|| fill(caret_bounds, theme.colors.text));
        self.input.update(cx, |input, _| input.drawn = Some(drawn));
        Prepared {
            lines,
            selection,
            caret,
            caret_bounds,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepared: &mut Prepared,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepared.selection.drain(..) {
            window.paint_quad(selection);
        }
        let line_height = window.line_height();
        let mut origin = bounds.origin;
        for line in prepared.lines.iter() {
            // Text that cannot be drawn leaves a gap rather than stopping the window.
            line.paint(
                origin,
                line_height,
                TextAlign::Left,
                Some(bounds),
                window,
                cx,
            )
            .ok();
            origin.y += line.size(line_height).height;
        }
        if let Some(caret) = prepared.caret.take() {
            window.paint_quad(caret);
        }

        // After the caret moved, scroll it into view if it is outside what shows.
        let caret = prepared.caret_bounds;
        self.input.update(cx, |input, cx| {
            if !std::mem::take(&mut input.reveal_caret) {
                return;
            }
            let showing = input.scroll.bounds();
            let mut offset = input.scroll.offset();
            if caret.top() < showing.top() {
                offset.y += showing.top() - caret.top();
            } else if caret.bottom() > showing.bottom() {
                offset.y -= caret.bottom() - showing.bottom();
            } else {
                return;
            }
            input.scroll.set_offset(offset);
            cx.notify();
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::ui::test::{open, press};

    /// Opens a window with one input in it, focused, and keeps what it tells its owner.
    fn input(
        cx: &mut TestAppContext,
    ) -> (
        Entity<TextInput>,
        Rc<RefCell<Vec<TextInputEvent>>>,
        &mut VisualTestContext,
    ) {
        let (input, cx) = open(cx, |_, cx| TextInput::new("Message", cx).most_rows(3));
        let told = Rc::new(RefCell::new(Vec::new()));
        cx.update(|window, cx| {
            let told = told.clone();
            cx.subscribe(&input, move |_, event: &TextInputEvent, _| {
                if *event != TextInputEvent::Changed {
                    told.borrow_mut().push(*event);
                }
            })
            .detach();
            window.focus(&input.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (input, told, cx)
    }

    /// The text with `|` where the caret is, or `[` and `]` around what is selected.
    fn shown(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
        input.read_with(cx, |input, _| {
            let mut text = input.text.clone();
            if input.selection.is_empty() {
                text.insert(input.caret(), '|');
            } else {
                text.insert(input.selection.end, ']');
                text.insert(input.selection.start, '[');
            }
            text
        })
    }

    #[gpui::test]
    fn typing_goes_where_the_caret_is_and_keys_move_it(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);

        cx.simulate_input("hello world");
        assert_eq!(shown(&input, cx), "hello world|");

        press(cx, "left left left left left");
        cx.simulate_input("big ");
        assert_eq!(shown(&input, cx), "hello big |world");

        press(cx, "alt-left");
        assert_eq!(shown(&input, cx), "hello |big world");
        press(cx, "alt-right");
        assert_eq!(shown(&input, cx), "hello big| world");

        press(cx, "cmd-left");
        assert_eq!(shown(&input, cx), "|hello big world");
        press(cx, "cmd-right backspace");
        assert_eq!(shown(&input, cx), "hello big worl|");
        press(cx, "alt-backspace");
        assert_eq!(shown(&input, cx), "hello big |");
        press(cx, "cmd-backspace");
        assert_eq!(shown(&input, cx), "|");
    }

    #[gpui::test]
    fn what_is_selected_is_replaced_copied_and_cut(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);
        cx.simulate_input("one two");

        press(cx, "alt-shift-left");
        assert_eq!(shown(&input, cx), "one [two]");
        press(cx, "cmd-c left");
        assert_eq!(
            shown(&input, cx),
            "one |two",
            "Left goes to the selection's start"
        );

        press(cx, "cmd-a");
        assert_eq!(shown(&input, cx), "[one two]");
        press(cx, "cmd-x");
        assert_eq!(shown(&input, cx), "|");
        press(cx, "cmd-v cmd-v");
        assert_eq!(shown(&input, cx), "one twoone two|");

        press(cx, "shift-left shift-left shift-left");
        cx.simulate_input("x");
        assert_eq!(shown(&input, cx), "one twoone x|");
    }

    #[gpui::test]
    fn the_caret_moves_by_whole_characters_as_people_see_them(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);
        input.update(cx, |input, cx| input.set_text("a👩‍👩‍👧é", cx));

        press(cx, "left");
        assert_eq!(shown(&input, cx), "a👩‍👩‍👧|é");
        press(cx, "backspace");
        assert_eq!(shown(&input, cx), "a|é", "a family is one character");
    }

    #[gpui::test]
    fn enter_hands_the_text_over_and_shift_enter_starts_a_line(cx: &mut TestAppContext) {
        let (input, told, cx) = input(cx);
        cx.simulate_input("one");

        press(cx, "shift-enter");
        cx.simulate_input("two");
        assert_eq!(shown(&input, cx), "one\ntwo|");
        assert_eq!(*told.borrow(), []);

        press(cx, "enter");
        assert_eq!(*told.borrow(), [TextInputEvent::Submitted]);
        assert_eq!(
            shown(&input, cx),
            "one\ntwo|",
            "its owner decides what happens next"
        );
    }

    #[gpui::test]
    fn up_and_down_move_between_rows_keeping_the_column(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);
        input.update(cx, |input, cx| {
            input.set_text("first line\nx\nthird line", cx)
        });
        cx.run_until_parked();

        press(cx, "up");
        assert_eq!(shown(&input, cx), "first line\nx|\nthird line");
        press(cx, "up");
        assert_eq!(
            shown(&input, cx),
            "first line|\nx\nthird line",
            "back out to its column"
        );
        press(cx, "up");
        assert_eq!(
            shown(&input, cx),
            "|first line\nx\nthird line",
            "above the first row"
        );
        press(cx, "down down down");
        assert_eq!(
            shown(&input, cx),
            "first line\nx\nthird line|",
            "below the last row"
        );

        press(cx, "cmd-up shift-down");
        assert_eq!(shown(&input, cx), "[first line\n]x\nthird line");
    }

    #[gpui::test]
    fn typing_is_undone_a_run_at_a_time(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);
        cx.simulate_input("hello");
        press(cx, "cmd-a");
        cx.simulate_input("bye");
        assert_eq!(shown(&input, cx), "bye|");

        press(cx, "cmd-z");
        assert_eq!(shown(&input, cx), "[hello]");
        press(cx, "cmd-z");
        assert_eq!(shown(&input, cx), "|");
        press(cx, "cmd-shift-z cmd-shift-z");
        assert_eq!(shown(&input, cx), "bye|");
    }

    #[gpui::test]
    fn while_its_owner_offers_choices_their_keys_go_to_it(cx: &mut TestAppContext) {
        let (input, told, cx) = input(cx);
        cx.simulate_input("@a");
        input.update(cx, |input, cx| input.set_choosing(true, cx));
        cx.run_until_parked();

        press(cx, "down up enter tab escape");
        let choice = TextInputEvent::Choice;
        assert_eq!(
            *told.borrow(),
            [
                choice(Choice::Next),
                choice(Choice::Previous),
                choice(Choice::Accept),
                choice(Choice::Accept),
                choice(Choice::Dismiss)
            ]
        );
        assert_eq!(shown(&input, cx), "@a|");

        told.borrow_mut().clear();
        input.update(cx, |input, cx| input.set_choosing(false, cx));
        cx.run_until_parked();
        press(cx, "enter");
        assert_eq!(*told.borrow(), [TextInputEvent::Submitted]);
    }

    #[gpui::test]
    fn accented_parts_move_with_the_text_and_go_when_typed_into(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);
        input.update(cx, |input, cx| {
            input.set_text("hi @Ada and @Grace", cx);
            input.set_accented(vec![3..7, 12..18], cx);
            input.replace(0..2, "hello", cx);
        });
        let accented = |cx: &mut VisualTestContext| {
            input.read_with(cx, |input, _| {
                let parts = input.accented().iter();
                parts
                    .map(|range| input.text()[range.clone()].to_owned())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(accented(cx), ["@Ada", "@Grace"]);

        input.update(cx, |input, cx| input.replace(8..9, "", cx));
        assert_eq!(
            accented(cx),
            ["@Grace"],
            "a name with a letter gone names no one"
        );
    }

    #[gpui::test]
    fn an_input_method_composes_text_in_place(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);
        cx.simulate_input("a😀z");
        press(cx, "left");

        input.update_in(cx, |input, window, cx| {
            assert_eq!(input.length_utf16(), 4);
            // Composing "に" then "日" in its place, then settling on "日本".
            input.replace_and_mark_text_in_range(None, "に", Some(1..1), window, cx);
            assert_eq!(input.marked_text_range(window, cx), Some(3..4));
            input.replace_and_mark_text_in_range(None, "日", Some(1..1), window, cx);
            input.replace_text_in_range(None, "日本", window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
        });
        assert_eq!(shown(&input, cx), "a😀日本|z");
    }

    #[gpui::test]
    fn it_grows_with_its_text_up_to_its_most_rows(cx: &mut TestAppContext) {
        let (input, _, cx) = input(cx);
        let height = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                input.read(cx).scroll.bounds().size.height
            })
        };
        let one_row = height(cx);

        input.update(cx, |input, cx| input.set_text("one\ntwo", cx));
        assert_eq!(height(cx), one_row * 2.);

        input.update(cx, |input, cx| input.set_text("1\n2\n3\n4\n5", cx));
        assert_eq!(height(cx), one_row * 3., "it scrolls past three rows");
        let caret = input.read_with(cx, |input, _| input.caret_bounds().unwrap());
        let showing = input.read_with(cx, |input, _| input.scroll.bounds());
        assert!(
            caret.bottom() <= showing.bottom() && caret.top() >= showing.top(),
            "the caret is scrolled into view: {caret:?} in {showing:?}"
        );
    }
}
