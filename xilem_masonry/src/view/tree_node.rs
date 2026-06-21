// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

use std::marker::PhantomData;

use crate::core::{MessageCtx, MessageResult, Mut, View, ViewId, ViewMarker, ViewPathTracker};
use crate::masonry::widgets;
use crate::{Pod, ViewCtx, WidgetView};

/// Create a tree node with the given content and child nodes.
/// Pass an empty `Vec` for a leaf.
pub fn tree_node<State, Action, V>(
    content: V,
    children: Vec<TreeNode<State, Action, V>>,
) -> TreeNode<State, Action, V>
where
    State: 'static,
    Action: 'static,
    V: WidgetView<State, Action>,
{
    TreeNode {
        content,
        children,
        phantom: PhantomData,
    }
}

/// A node in a tree view: a content row plus indented child nodes.
#[must_use = "View values do nothing unless provided to Xilem."]
pub struct TreeNode<State, Action, V> {
    content: V,
    children: Vec<TreeNode<State, Action, V>>,
    phantom: PhantomData<fn(State) -> Action>,
}

/// Random 32-bit id, so a wrong message path is caught instead of silently misrouted.

const TREE_NODE_CONTENT_VIEW_ID: ViewId = ViewId::new(0x5A3F_1C02);

impl<State, Action, V> ViewMarker for TreeNode<State, Action, V>{}
impl<State, Action, V> View<State, Action, ViewCtx> for TreeNode<State, Action, V>
where
    State: 'static,
    Action: 'static,
    V: WidgetView<State, Action>,
{
    type Element = Pod<widgets::TreeNode>;
    type ViewState = TreeNodeViewState<V::ViewState>;

    fn build(&self, ctx: &mut ViewCtx, app_state: &mut State) -> (Self::Element, Self::ViewState) {
        // 1. Build the content row (tagged with the constant content id).
        let (content, content_state) = ctx.with_id(TREE_NODE_CONTENT_VIEW_ID, |ctx| {
            View::<State, Action, _>::build(&self.content, ctx, app_state)
        });

        // 2. Build each child, tagged by its index (0, 1, 2, ...).
        let mut child_widgets = Vec::with_capacity(self.children.len());
        let mut child_states = Vec::with_capacity(self.children.len());
        for (i, child) in self.children.iter().enumerate() {
            let (child_el, child_state) = ctx.with_id(ViewId::new(i as u64), |ctx| {
                View::<State, Action, _>::build(child, ctx, app_state)
            });
            child_widgets.push(child_el.new_widget);
            child_states.push(child_state);
        }

        // 3. Assemble the masonry widget: leaf if no children, branch otherwise.
        let widget = if child_widgets.is_empty() {
            widgets::TreeNode::new(content.new_widget)
        } else {
            widgets::TreeNode::branch(content.new_widget, child_widgets)
        };
        let pod = ctx.create_pod(widget);

        // 4. Return the element plus the recursive state.
        (
            pod,
            TreeNodeViewState {
                content: content_state,
                children: child_states,
            },
        )
    }

    fn rebuild(
        &self,
        prev: &Self,
        state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) {
        // 1. Rebuild the content row (uses the content's slice of the state).
        ctx.with_id(TREE_NODE_CONTENT_VIEW_ID, |ctx| {
            View::<State, Action, _>::rebuild(
                &self.content,
                &prev.content,
                &mut state.content,
                ctx,
                widgets::TreeNode::content_mut(&mut element).downcast(),
                app_state,
            );
        });

        let prev_len = prev.children.len();
        let new_len = self.children.len();
        let common = prev_len.min(new_len);

        // 2. Children present in BOTH old and new: rebuild in place.
        for i in 0..common {
            ctx.with_id(ViewId::new(i as u64), |ctx| {
                View::<State, Action, _>::rebuild(
                    &self.children[i],
                    &prev.children[i],
                    &mut state.children[i],
                    ctx,
                    widgets::TreeNode::child_mut(&mut element, i),
                    app_state,
                );
            });
        }
        // 3. Children that were REMOVED (new is shorter): tear down + remove, from the tail.
        for i in (new_len..prev_len).rev() {
            ctx.with_id(ViewId::new(i as u64), |ctx| {
                View::<State, Action, _>::teardown(
                    &prev.children[i],
                    &mut state.children[i],
                    ctx,
                    widgets::TreeNode::child_mut(&mut element, i),
                );
            });
            widgets::TreeNode::remove_child(&mut element, i);
            state.children.pop();
        }

        // 4. Children that were ADDED (new is longer): build + add.
        for i in prev_len..new_len {
            let (child_el, child_state) = ctx.with_id(ViewId::new(i as u64), |ctx| {
                View::<State, Action, _>::build(&self.children[i], ctx, app_state)
            });
            widgets::TreeNode::add_child(&mut element, child_el.new_widget);
            state.children.push(child_state);
        }
    }

    fn teardown(
        &self,
        state: &mut Self::ViewState,
        ctx: &mut ViewCtx,
        mut element: Mut<'_, Self::Element>,
    ) {
        ctx.with_id(TREE_NODE_CONTENT_VIEW_ID, |ctx| {
            View::<State, Action, _>::teardown(
                &self.content,
                &mut state.content,
                ctx,
                widgets::TreeNode::content_mut(&mut element).downcast(),
            );
        });
        for i in 0..self.children.len() {
            ctx.with_id(ViewId::new(i as u64), |ctx| {
                View::<State, Action, _>::teardown(
                    &self.children[i],
                    &mut state.children[i],
                    ctx,
                    widgets::TreeNode::child_mut(&mut element, i),
                );
            });
        }
    }

    fn message(
        &self,
        state: &mut Self::ViewState,
        message: &mut MessageCtx,
        mut element: Mut<'_, Self::Element>,
        app_state: &mut State,
    ) -> MessageResult<Action> {
        match message.take_first() {
            // The constant id → it's for the content.
            Some(TREE_NODE_CONTENT_VIEW_ID) => self.content.message(
                &mut state.content,
                message,
                widgets::TreeNode::content_mut(&mut element).downcast(),
                app_state,
            ),
            // Any other id → it's a child; the id IS the index.
            Some(other) => {
                let index = other.routing_id() as usize;
                self.children[index].message(
                    &mut state.children[index],
                    message,
                    widgets::TreeNode::child_mut(&mut element, index),
                    app_state,
                )
            }
            // Empty path → meant for this view itself; we emit nothing.
            None => MessageResult::Stale,
        }
    }
}

/// Retained state for a `TreeNode` view: the content's state plus each child's state.
pub struct TreeNodeViewState<ContentState> {
    content: ContentState,
    children: Vec<TreeNodeViewState<ContentState>>,
}