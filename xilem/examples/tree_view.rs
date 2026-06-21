// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! A small tree view built from the `tree_node` view.

use winit::error::EventLoopError;
use xilem::view::{label, tree_node};
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};

fn app_logic(_data: &mut ()) -> impl WidgetView<()> + use<> {
    tree_node(
        label("Root"),
        vec![
            tree_node(label("Child A"), vec![]),
            tree_node(
                label("Child B"),
                vec![tree_node(label("Grandchild"), vec![
                    tree_node(label("Great-grandchild-A"), vec![]),
                    tree_node(label("Great-grandchild-B"), vec![]),
                ])],
            ),
            tree_node(label("Child C"), vec![]),
        ],
    )
}

fn main() -> Result<(), EventLoopError> {
    let app = Xilem::new_simple((), app_logic, WindowOptions::new("Tree View"));
    app.run_in(EventLoop::with_user_event())?;
    Ok(())
}