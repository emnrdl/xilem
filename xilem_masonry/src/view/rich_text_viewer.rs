// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

use std::marker::PhantomData;

use masonry::core::ArcStr;
use masonry::parley::style::FontWeight;
use masonry::properties::LineBreaking;

use crate::core::{MessageCtx, MessageResult, Mut, View, ViewMarker};
use crate::view::{Portal, Prose, portal, prose};
use crate::{Pod, ViewCtx, WidgetView};

/// A scrollable, selectable rich text viewer.
///
/// This is intentionally built from one [`Prose`] child inside one [`Portal`]
/// so selection can span the entire document. It is best suited for generated
/// answers, logs, markdown-like text, and other read-only documents where users
/// need to scroll and copy arbitrary ranges.
pub fn rich_text_viewer<State, Action>(content: impl Into<ArcStr>) -> RichTextViewer<State, Action>
where
    State: 'static,
    Action: 'static,
{
    RichTextViewer {
        content: content.into(),
        text_size: masonry::theme::TEXT_SIZE_NORMAL,
        weight: FontWeight::NORMAL,
        line_break_mode: LineBreaking::WordWrap,
        constrain_horizontal: true,
        constrain_vertical: false,
        phantom: PhantomData,
    }
}

/// The view returned by [`rich_text_viewer`].
#[must_use = "View values do nothing unless provided to Xilem."]
pub struct RichTextViewer<State, Action> {
    content: ArcStr,
    text_size: f32,
    weight: FontWeight,
    line_break_mode: LineBreaking,
    constrain_horizontal: bool,
    constrain_vertical: bool,
    phantom: PhantomData<fn(State) -> Action>,
}

impl<State, Action> RichTextViewer<State, Action>
where
    State: 'static,
    Action: 'static,
{
    /// Set the font size used by the selectable document body.
    pub fn text_size(mut self, text_size: f32) -> Self {
        self.text_size = text_size;
        self
    }

    /// Set the font weight used by the selectable document body.
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Set how the document should wrap or overflow horizontally.
    pub fn line_break_mode(mut self, line_break_mode: LineBreaking) -> Self {
        self.line_break_mode = line_break_mode;
        self
    }

    /// Control whether the inner document receives the viewer width as a bound.
    pub fn constrain_horizontal(mut self, constrain_horizontal: bool) -> Self {
        self.constrain_horizontal = constrain_horizontal;
        self
    }

    /// Control whether the inner document receives the viewer height as a bound.
    pub fn constrain_vertical(mut self, constrain_vertical: bool) -> Self {
        self.constrain_vertical = constrain_vertical;
        self
    }
}

impl<State, Action> ViewMarker for RichTextViewer<State, Action> {}

impl<State, Action> View<State, Action, ViewCtx> for RichTextViewer<State, Action>
where
    State: 'static,
    Action: 'static,
{
    type Element =
        Pod<<Portal<Prose<State, Action>, State, Action> as WidgetView<State, Action>>::Widget>;
    type ViewState =
        <Portal<Prose<State, Action>, State, Action> as View<State, Action, ViewCtx>>::ViewState;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        self.inner().build(ctx, app_state)
    }

    fn rebuild(
        &self,
        prev: &Self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) {
        self.inner()
            .rebuild(&prev.inner(), view_state, ctx, element, app_state);
    }

    fn teardown(
        &self,
        view_state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        element: Mut<'_, Self::Element>,
    ) {
        self.inner().teardown(view_state, ctx, element);
    }

    fn message(
        &self,
        view_state: &mut Self::ViewState,
        message: &mut MessageCtx,
        element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> MessageResult<Action> {
        self.inner()
            .message(view_state, message, element, app_state)
    }
}

impl<State, Action> RichTextViewer<State, Action>
where
    State: 'static,
    Action: 'static,
{
    fn inner(&self) -> Portal<Prose<State, Action>, State, Action> {
        portal(
            prose(self.content.clone())
                .text_size(self.text_size)
                .weight(self.weight)
                .line_break_mode(self.line_break_mode),
        )
        .constrain_horizontal(self.constrain_horizontal)
        .constrain_vertical(self.constrain_vertical)
    }
}
