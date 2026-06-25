// Copyright 2024 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! A read-only, mouse-selectable text widget that supports per-range colors.
//!
//! Unlike [`TextArea`](crate::widgets::TextArea) (which is built on parley's
//! `PlainEditor` and therefore only supports a single global text color), this
//! widget builds a ranged parley [`Layout`] directly, so different byte ranges
//! can use different brushes (e.g. syntax highlighting), while still letting the
//! user drag-select and copy the whole block. Selection is driven by parley's
//! [`Selection`] running over the layout.

use std::any::TypeId;
use std::ops::Range;

use accesskit::{Node, Role};
use tracing::{Span, trace_span};

use crate::core::keyboard::{Key, KeyState};
use crate::core::{
    AccessCtx, ArcStr, BrushIndex, ChildrenIds, CursorIcon, EventCtx, LayoutCtx, MeasureCtx,
    NoAction, PaintCtx, PointerButton, PointerEvent, PropertiesMut, PropertiesRef, QueryCtx,
    RegisterCtx, StyleProperty, TextEvent, Update, UpdateCtx, Widget, WidgetId, WidgetMut,
    render_text,
};
use crate::imaging::Painter;
use crate::kurbo::{Affine, Axis, Point, Rect, Size};
use crate::layout::{AsUnit, LenReq, Length};
use crate::parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout, LayoutContext,
    Selection,
};
use crate::peniko::{Brush, Color};

/// One byte range of the text painted with a given color.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSpan {
    /// Byte range into the widget's text.
    pub range: Range<usize>,
    /// Color used for that range.
    pub color: Color,
}

/// A selectable, per-range-colored, read-only text view.
pub struct ColoredText {
    text: ArcStr,
    spans: Vec<ColorSpan>,
    default_color: Color,
    font: FontFamily<'static>,
    font_size: f32,
    weight: FontWeight,
    hint: bool,

    /// Built lazily from the inputs above; `None` means "rebuild needed".
    layout: Option<Layout<BrushIndex>>,
    /// Brush palette indexed by `BrushIndex`. Index 0 is the default color.
    palette: Vec<Brush>,
    selection: Option<Selection>,
}

impl ColoredText {
    /// Creates a selectable, per-range-colored text widget.
    pub fn new(
        text: impl Into<ArcStr>,
        spans: Vec<ColorSpan>,
        default_color: Color,
        font: FontFamily<'static>,
        font_size: f32,
        weight: FontWeight,
    ) -> Self {
        Self {
            text: text.into(),
            spans,
            default_color,
            font,
            font_size,
            weight,
            hint: true,
            layout: None,
            palette: Vec::new(),
            selection: None,
        }
    }

    /// Replaces the text and colored spans, forcing a layout rebuild.
    pub fn set_text(&mut self, text: impl Into<ArcStr>, spans: Vec<ColorSpan>) {
        self.text = text.into();
        self.spans = spans;
        self.layout = None;
        self.selection = None;
    }

    /// Updates the text + colored spans from a [`WidgetMut`] and requests layout.
    pub fn set_text_mut(
        this: &mut WidgetMut<'_, Self>,
        text: impl Into<ArcStr>,
        spans: Vec<ColorSpan>,
    ) {
        this.widget.set_text(text, spans);
        this.ctx.request_layout();
    }

    /// Rebuilds the parley layout + brush palette if needed.
    fn ensure_layout(&mut self, fctx: &mut FontContext, lctx: &mut LayoutContext<BrushIndex>) {
        if self.layout.is_some() {
            return;
        }

        // Build the palette: index 0 is the default color, then one entry per
        // distinct span color.
        let mut palette: Vec<Brush> = vec![self.default_color.into()];
        let index_for = |color: Color, palette: &mut Vec<Brush>| -> usize {
            let brush: Brush = color.into();
            if let Some(found) = palette.iter().position(|existing| *existing == brush) {
                found
            } else {
                palette.push(brush);
                palette.len() - 1
            }
        };

        let mut builder = lctx.ranged_builder(fctx, &self.text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(self.font_size));
        builder.push_default(StyleProperty::FontWeight(self.weight));
        builder.push_default(StyleProperty::FontFamily(self.font.clone()));
        builder.push_default(StyleProperty::Brush(BrushIndex(0)));

        for span in &self.spans {
            if span.range.start >= span.range.end || span.range.end > self.text.len() {
                continue;
            }
            let idx = index_for(span.color, &mut palette);
            builder.push(StyleProperty::Brush(BrushIndex(idx)), span.range.clone());
        }

        let mut layout = Layout::new();
        builder.build_into(&mut layout, &self.text);
        // No wrapping: code scrolls horizontally inside its portal.
        layout.break_all_lines(None);
        layout.align(None, Alignment::Start, AlignmentOptions::default());

        self.palette = palette;
        self.layout = Some(layout);
    }

    fn copy_selection(&self, ctx: &mut EventCtx<'_>) {
        if let Some(selection) = &self.selection {
            let range = selection.text_range();
            if range.start < range.end {
                if let Some(text) = self.text.get(range) {
                    ctx.set_clipboard(text.to_string());
                }
            }
        }
    }
}

impl Widget for ColoredText {
    type Action = NoAction;

    fn accepts_pointer_interaction(&self) -> bool {
        true
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accepts_text_input(&self) -> bool {
        false
    }

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn property_changed(&mut self, _ctx: &mut UpdateCtx<'_>, _property_type: TypeId) {}

    fn update(&mut self, ctx: &mut UpdateCtx<'_>, _props: &mut PropertiesMut<'_>, event: &Update) {
        if let Update::FontsChanged = event {
            self.layout = None;
            ctx.request_layout();
        }
    }

    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        let Some(layout) = &self.layout else {
            return;
        };
        match event {
            PointerEvent::Down(button_event)
                if matches!(
                    button_event.button,
                    None | Some(PointerButton::Primary)
                ) =>
            {
                let pos = ctx.local_position(button_event.state.position);
                let (x, y) = (pos.x as f32, pos.y as f32);
                self.selection = Some(match button_event.state.count {
                    2 => Selection::word_from_point(layout, x, y),
                    3 => Selection::line_from_point(layout, x, y),
                    _ => Selection::from_point(layout, x, y),
                });
                ctx.request_focus();
                ctx.capture_pointer();
                ctx.request_render();
            }
            PointerEvent::Move(update) if ctx.is_active() => {
                let pos = ctx.local_position(update.current.position);
                if let Some(selection) = &self.selection {
                    self.selection =
                        Some(selection.extend_to_point(layout, pos.x as f32, pos.y as f32));
                    ctx.request_render();
                }
            }
            _ => {}
        }
    }

    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        if let TextEvent::Keyboard(key_event) = event {
            if key_event.state != KeyState::Down {
                return;
            }
            let action_mod = if cfg!(target_os = "macos") {
                key_event.modifiers.meta()
            } else {
                key_event.modifiers.ctrl()
            };
            if let Key::Character(c) = &key_event.key {
                if action_mod && c.as_str().eq_ignore_ascii_case("c") {
                    self.copy_selection(ctx);
                } else if action_mod && c.as_str().eq_ignore_ascii_case("a") {
                    if let Some(layout) = &self.layout {
                        self.selection = Some(
                            Selection::from_point(layout, 0.0, 0.0)
                                .extend_to_point(layout, f32::MAX, f32::MAX),
                        );
                        ctx.request_render();
                    }
                }
            }
        }
    }

    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        _len_req: LenReq,
        _cross_length: Option<Length>,
    ) -> Length {
        let (fctx, lctx) = ctx.text_contexts();
        self.ensure_layout(fctx, lctx);
        let layout = self.layout.as_ref().unwrap();
        let length = if axis == Axis::Horizontal {
            layout.width()
        } else {
            layout.height()
        };
        length.px()
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, _size: Size) {
        let (fctx, lctx) = ctx.text_contexts();
        self.ensure_layout(fctx, lctx);
        let layout = self.layout.as_ref().unwrap();

        let line_count = layout.len();
        if line_count > 0 {
            let first = layout.get(0).unwrap();
            let last = layout.get(line_count - 1).unwrap();
            ctx.set_baselines(
                first.metrics().baseline as f64,
                last.metrics().baseline as f64,
            );
        } else {
            ctx.clear_baselines();
        }
    }

    fn paint(
        &mut self,
        ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        painter: &mut Painter<'_>,
    ) {
        let Some(layout) = &self.layout else {
            return;
        };

        if ctx.is_focus_target() {
            if let Some(selection) = &self.selection {
                let highlight = Color::from_rgba8(0x4f, 0x46, 0xe5, 0x66);
                for (bb, _) in selection.geometry(layout) {
                    let rect = Rect::new(bb.x0, bb.y0, bb.x1, bb.y1);
                    painter.fill(rect, highlight).draw();
                }
            }
        }

        render_text(
            painter,
            Affine::IDENTITY,
            layout,
            &self.palette,
            self.hint,
        );
    }

    fn get_cursor(&self, _ctx: &QueryCtx<'_>, _pos: Point) -> CursorIcon {
        CursorIcon::Text
    }

    fn accessibility_role(&self) -> Role {
        Role::Document
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.set_value(self.text.to_string());
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("ColoredText", id = id.trace())
    }

    fn get_debug_text(&self) -> Option<String> {
        Some(self.text.to_string())
    }
}
