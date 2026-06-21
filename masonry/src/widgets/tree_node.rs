// Copyright 2018 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

use crate::core::{
    AccessCtx, ChildrenIds, LayoutCtx, MeasureCtx, NewWidget, NoAction, PaintCtx, PropertiesRef,
    RegisterCtx, Widget, WidgetMut, WidgetPod,
};

use crate::accesskit;
use crate::imaging::Painter;
use crate::kurbo::{Axis, Point, Size};
use crate::layout::{LayoutSize, LenDef, LenReq, Length, SizeDef};
use crate::widgets::DisclosureButton;

/// How far each level is indented from its parent.
const INDENT: Length = Length::const_px(16.);
/// Size of the disclosure triangle (a square).
const BUTTON_LENGTH: Length = Length::const_px(16.);
/// Gap between the triangle and the content.
const GAP: Length = Length::const_px(4.);

/// A single node in a tree view.
///
/// A leaf node ([`TreeNode::new`]) shows only its content. A branch node
/// ([`TreeNode::branch`]) also shows a disclosure triangle and child nodes,
/// indented below it, which can be expanded or collapsed.
pub struct TreeNode {
    disclosure_button: Option<WidgetPod<DisclosureButton>>,
    content: WidgetPod<dyn Widget>,
    children: Vec<WidgetPod<Self>>,
}

impl TreeNode {
    /// Create a node that displays the given widget.
    pub fn new(content: NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            disclosure_button: None,
            content: content.erased().to_pod(),
            children: Vec::new(),
        }
    }

    /// Create a branch node: a content row plus child nodes, with a disclosure
    /// triangle to expand or collapse them.
    pub fn branch(
        content: NewWidget<impl Widget + ?Sized>,
        children: Vec<NewWidget<Self>>,
    ) -> Self {
        Self {
            disclosure_button: Some(DisclosureButton::new(true).prepare().to_pod()),
            content: content.erased().to_pod(),
            children: children.into_iter().map(NewWidget::to_pod).collect(),
        }
    }

    /// Expand or collapse this node (no effect on a leaf).
    pub fn set_expanded(this: &mut WidgetMut<'_, Self>, expanded: bool) {
        if let Some(btn) = this.widget.disclosure_button.as_mut() {
            let mut btn = this.ctx.get_mut(btn);
            DisclosureButton::set_disclosed(&mut btn, expanded);
        }
        this.ctx.request_layout();
    }

    /// Append a child node. If this was a leaf, it becomes a branch (gains a triangle).
    pub fn add_child(this: &mut WidgetMut<'_, Self>, child: NewWidget<Self>) {
        if this.widget.disclosure_button.is_none() {
            this.widget.disclosure_button = Some(DisclosureButton::new(true).prepare().to_pod());
        }
        this.widget.children.push(child.to_pod());
        this.ctx.children_changed();
    }

    /// Remove the `index`th child node. If this was the last child, it becomes a leaf (loses its triangle).
    pub fn remove_child(this: &mut WidgetMut<'_, Self>, index: usize) {
        let pod = this.widget.children.remove(index);
        this.ctx.remove_child(pod);

        // If that was the last child, this node is a leaf again: drop the triangle.
        if this.widget.children.is_empty()
            && let Some(btn) = this.widget.disclosure_button.take()
        {
            this.ctx.remove_child(btn);
        }

        this.ctx.children_changed();
    }

    /// Edit the content widget in place.
    pub fn content_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.content)
    }

    ///edit the `index`th child node in place.
    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>, index: usize) -> WidgetMut<'t, Self> {
        this.ctx.get_mut(&mut this.widget.children[index])
    }
}

impl Widget for TreeNode {
    type Action = NoAction;

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        if let Some(btn) = &mut self.disclosure_button {
            ctx.register_child(btn);
        }
        ctx.register_child(&mut self.content);
        for child in &mut self.children {
            ctx.register_child(child);
        }
    }
    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        len_req: LenReq,
        cross_length: Option<Length>,
    ) -> Length {
        // FitContent means "measure naturally, then clamp to this space".
        // Min/MaxContent have no clamp.
        let fit_space = match len_req {
            LenReq::FitContent(space) => Some(space),
            _ => None,
        };
        // Children/content are measured at their natural (min) size.
        let space: LenDef = match len_req {
            LenReq::FitContent(_) => LenReq::MinContent.into(),
            other => other.into(),
        };

        let cross = axis.cross();
        let context = LayoutSize::maybe(cross, cross_length);

        // Space the triangle takes on the row (0 for a leaf).
        let button_main = match axis {
            Axis::Horizontal => BUTTON_LENGTH.saturating_add(GAP),
            Axis::Vertical => BUTTON_LENGTH,
        };

        // Content sits after the triangle on the horizontal axis.
        let content_auto = match axis {
            Axis::Horizontal => space.reduce(button_main),
            Axis::Vertical => space,
        };

        let content_length =
            ctx.compute_length(&mut self.content, content_auto, context, axis, cross_length);

        let row_length = match axis {
            Axis::Horizontal => button_main.saturating_add(content_length),
            Axis::Vertical => button_main.max(content_length),
        };
        // Ask the triangle whether we are expanded.
        let expanded = match &mut self.disclosure_button {
            Some(btn) => ctx.get_raw(btn).0.is_disclosed(),
            None => false,
        };

        let mut children_length = Length::ZERO;
        if expanded {
            for child in &mut self.children {
                let child_auto = match axis {
                    Axis::Horizontal => space.reduce(INDENT),
                    Axis::Vertical => space,
                };
                let child_length =
                    ctx.compute_length(child, child_auto, context, axis, cross_length);
                match axis {
                    Axis::Horizontal => {
                        children_length = children_length.max(INDENT.saturating_add(child_length));
                    }
                    Axis::Vertical => {
                        children_length = children_length.saturating_add(child_length);
                    }
                }
            }
        }

        let total = match axis {
            Axis::Horizontal => row_length.max(children_length),
            Axis::Vertical => row_length.saturating_add(children_length),
        };

        // The one line that was wrong: take what we need, never more than offered.
        match fit_space {
            Some(space) => total.min(space),
            None => total,
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, size: Size) {
        let gap = GAP.get();
        let button_len = BUTTON_LENGTH.get();
        let button_main = button_len + gap;

        let button_h = if let Some(btn) = &mut self.disclosure_button {
            ctx.run_layout(btn, Size::new(button_len, button_len));
            button_len
        } else {
            0.0
        };

        let content_avail = Size::new((size.width - button_main).max(0.0), size.height);
        let content_size = ctx.compute_size(
            &mut self.content,
            SizeDef::fit(content_avail),
            content_avail.into(),
        );
        ctx.run_layout(&mut self.content, content_size);

        let row_height = content_size.height.max(button_h);

        if let Some(btn) = &mut self.disclosure_button {
            ctx.place_child(btn, Point::new(0.0, (row_height - button_h) * 0.5));
        }
        ctx.place_child(
            &mut self.content,
            Point::new(button_main, (row_height - content_size.height) * 0.5),
        );

        // Expanded? Ask the triangle.
        let expanded = match &mut self.disclosure_button {
            Some(btn) => ctx.get_raw(btn).0.is_disclosed(),
            None => false,
        };

        // Each child below the previous one, pushed right by INDENT.
        let indent = INDENT.get();
        let mut y = row_height;
        for child in &mut self.children {
            ctx.set_stashed(child, !expanded); //hide children when collapsed
            if expanded {
                let avail = Size::new((size.width - indent).max(0.0), (size.height - y).max(0.0));
                let child_size = ctx.compute_size(child, SizeDef::fit(avail), avail.into());
                ctx.run_layout(child, child_size);
                ctx.place_child(child, Point::new(indent, y));
                y += child_size.height;
            }
        }
        ctx.derive_baselines(&self.content);
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {
    }

    fn accessibility_role(&self) -> accesskit::Role {
        accesskit::Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut accesskit::Node,
    ) {
    }

    fn children_ids(&self) -> ChildrenIds {
        let mut ids = Vec::new();
        if let Some(btn) = &self.disclosure_button {
            ids.push(btn.id());
        }
        ids.push(self.content.id());
        ids.extend(self.children.iter().map(|c| c.id()));
        ChildrenIds::from_slice(&ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TestHarness, assert_render_snapshot};
    use crate::theme::test_property_set;
    use crate::widgets::Label;

    #[test]
    fn single_leaf() {
        // content = a Label; node = TreeNode wrapping it; root = the node.
        let widget = NewWidget::new(TreeNode::new(NewWidget::new(Label::new("Leaf"))));
        let mut harness = TestHarness::create_with_size(test_property_set(), widget, (200, 100));
        assert_render_snapshot!(harness, "tree_node_single_leaf");
    }

    #[test]
    fn with_children() {
        let widget = NewWidget::new(TreeNode::branch(
            NewWidget::new(Label::new("Root")),
            vec![
                NewWidget::new(TreeNode::new(NewWidget::new(Label::new("Child A")))),
                NewWidget::new(TreeNode::branch(
                    NewWidget::new(Label::new("Child B")),
                    vec![NewWidget::new(TreeNode::new(NewWidget::new(Label::new(
                        "Grandchild",
                    ))))],
                )),
                NewWidget::new(TreeNode::new(NewWidget::new(Label::new("Child C")))),
            ],
        ));
        let mut harness = TestHarness::create_with_size(test_property_set(), widget, (240, 160));
        assert_render_snapshot!(harness, "tree_node_with_children");
    }

    #[test]
    fn collapse_via_click() {
        let widget = NewWidget::new(TreeNode::branch(
            NewWidget::new(Label::new("Root")),
            vec![
                NewWidget::new(TreeNode::new(NewWidget::new(Label::new("Child A")))),
                NewWidget::new(TreeNode::new(NewWidget::new(Label::new("Child B")))),
            ],
        ));
        let mut harness = TestHarness::create_with_size(test_property_set(), widget, (240, 160));
        assert_render_snapshot!(harness, "tree_node_branch_expanded");

        // The triangle is the first child of the root node.
        let triangle = harness.root_widget().children()[0].id();
        harness.mouse_click_on(triangle, None);
        assert_render_snapshot!(harness, "tree_node_branch_collapsed");
    }

    #[test]
    fn edit_collapses() {
        let widget = NewWidget::new(TreeNode::branch(
            NewWidget::new(Label::new("Root")),
            vec![NewWidget::new(TreeNode::new(NewWidget::new(Label::new(
                "Child",
            ))))],
        ));
        let mut harness = TestHarness::create_with_size(test_property_set(), widget, (200, 120));

        // Collapse it programmatically (not by clicking the triangle).
        harness.edit_root_widget(|mut root| TreeNode::set_expanded(&mut root, false));

        assert_render_snapshot!(harness, "tree_node_edit_collapsed");

        harness.edit_root_widget(|mut root| TreeNode::set_expanded(&mut root, true));

        assert_render_snapshot!(harness, "tree_node_edit_expanded");
    }
}
