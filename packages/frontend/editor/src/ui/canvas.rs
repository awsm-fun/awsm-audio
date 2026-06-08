//! The canvas: a clipping viewport plus a pan/zoomed "world" layer that holds
//! the wire overlay and every node. Background drag pans, the wheel zooms toward
//! the cursor, and a viewport-level pointer-move/up pair drives whatever gesture
//! the controller currently has in flight (node move, pan, or wire).

use dominator::{clone, events, html, Dom, EventOptions};
use futures_signals::map_ref;
use futures_signals::signal::SignalExt;
use futures_signals::signal_vec::SignalVecExt;
use wasm_bindgen::JsCast;

use super::{node, wire};
use crate::controller::{controller, DragState, EditorCommand};

/// True if `target` is (or sits inside) an element matching `selector`.
fn target_matches(target: Option<web_sys::EventTarget>, selector: &str) -> bool {
    target
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.closest(selector).ok().flatten())
        .is_some()
}

pub fn render() -> Dom {
    let ctrl = controller();

    html!("div", {
        .class("canvas-viewport")
        .style("position", "absolute")
        .style("inset", "0")
        .style("overflow", "hidden")
        .style("background-color", "var(--bg-0)")
        // Dotted grid that pans/zooms with the world.
        .style("background-image", "radial-gradient(var(--line) 1px, transparent 1.5px)")
        .style_signal("background-size", ctrl.zoom.signal().map(|z| {
            let s = 26.0 * z;
            format!("{s}px {s}px")
        }))
        .style_signal("background-position", ctrl.pan.signal().map(|p| format!("{}px {}px", p.0, p.1)))
        .style("touch-action", "none")
        .style("cursor", "grab")
        // Stash the element for client→world coordinate conversion.
        .after_inserted(clone!(ctrl => move |el| {
            ctrl.set_viewport(el.unchecked_into());
        }))
        // Empty-space press: Shift starts a box-select; otherwise a pan.
        .event(clone!(ctrl => move |e: events::PointerDown| {
            // Only the empty canvas pans / box-selects. A press on a node or its
            // ports is handled by that element (move the node, or drag a wire);
            // this viewport handler ALSO fires for it in this dominator build
            // (the inner `stop_propagation` isn't honored), so bail to avoid
            // hijacking the gesture with a pan.
            if target_matches(e.target(), ".node") {
                return;
            }
            if e.shift_key() {
                let (wx, wy) = ctrl.client_to_world(e.x(), e.y());
                ctrl.dispatch(EditorCommand::ClearSelection);
                ctrl.box_select.set(Some((wx, wy, wx, wy)));
                *ctrl.drag.borrow_mut() = Some(DragState::Box { start_x: wx, start_y: wy });
            } else {
                let (px, py) = ctrl.pan.get();
                ctrl.dispatch(EditorCommand::ClearSelection);
                *ctrl.drag.borrow_mut() = Some(DragState::Pan {
                    start_cx: e.x(),
                    start_cy: e.y(),
                    start_px: px,
                    start_py: py,
                });
            }
        }))
        // Drive the in-flight gesture.
        .event(clone!(ctrl => move |e: events::PointerMove| {
            let (cx, cy) = (e.x(), e.y());
            let drag = ctrl.drag.borrow().clone();
            match drag {
                Some(DragState::Pan { start_cx, start_cy, start_px, start_py }) => {
                    ctrl.dispatch(EditorCommand::SetCamera {
                        pan_x: start_px + (cx - start_cx),
                        pan_y: start_py + (cy - start_cy),
                        zoom: ctrl.zoom.get(),
                    });
                }
                Some(DragState::Node { items }) => {
                    let (wx, wy) = ctrl.client_to_world(cx, cy);
                    for (id, grab_x, grab_y) in items {
                        ctrl.dispatch(EditorCommand::MoveNode { id, x: wx - grab_x, y: wy - grab_y });
                    }
                }
                Some(DragState::Box { start_x, start_y }) => {
                    let (wx, wy) = ctrl.client_to_world(cx, cy);
                    ctrl.box_select.set(Some((start_x, start_y, wx, wy)));
                }
                None => {}
            }
            if ctrl.pending.lock_ref().is_some() {
                let w = ctrl.client_to_world(cx, cy);
                ctrl.update_wire(w);
            }
        }))
        // Release: finalize a box-select, then end any drag / drop a stray wire.
        .event(clone!(ctrl => move |e: events::PointerUp| {
            if let Some((x0, y0, x1, y1)) = ctrl.box_select.get() {
                ctrl.select_in_box(x0, y0, x1, y1);
                ctrl.box_select.set(None);
            }
            *ctrl.drag.borrow_mut() = None;
            // Drop a stray wire — UNLESS the release landed on an input port,
            // whose own pointer-up commits the wire. That handler fires *after*
            // this one in this dominator build, so cancelling here would clear
            // the pending wire before it can connect.
            if !target_matches(e.target(), ".node-port-in") {
                ctrl.cancel_wire();
            }
        }))
        // Wheel: zoom toward the cursor.
        .event_with_options(&EventOptions::preventable(), clone!(ctrl => move |e: events::Wheel| {
            e.prevent_default();
            let factor = if e.delta_y() < 0.0 { 1.1 } else { 1.0 / 1.1 };
            ctrl.zoom_at(e.x(), e.y(), factor);
        }))
        // Accept palette drag-and-drop: dragover must preventDefault to allow a
        // drop; drop places the dragged node at the cursor.
        .event_with_options(&EventOptions::preventable(), |e: events::DragOver| {
            e.prevent_default();
        })
        .event_with_options(&EventOptions::preventable(), clone!(ctrl => move |e: events::Drop| {
            e.prevent_default();
            ctrl.drop_palette_item(e.x(), e.y());
        }))
        // The pan/zoomed world layer.
        .child(html!("div", {
            .class("canvas-world")
            .style("position", "absolute")
            .style("left", "0")
            .style("top", "0")
            .style("transform-origin", "0 0")
            .style_signal("transform", map_ref! {
                let pan = ctrl.pan.signal(), let zoom = ctrl.zoom.signal() =>
                    format!("translate({}px, {}px) scale({})", pan.0, pan.1, zoom)
            })
            .child(wire::render())
            .children_signal_vec(ctrl.nodes.signal_vec_cloned().map(node::render))
            // Rubber-band box-select overlay.
            .child_signal(ctrl.box_select.signal().map(|b| b.map(|(x0, y0, x1, y1)| {
                let (l, t) = (x0.min(x1), y0.min(y1));
                let (w, h) = ((x0 - x1).abs(), (y0 - y1).abs());
                html!("div", {
                    .style("position", "absolute")
                    .style("pointer-events", "none")
                    .style("left", &format!("{l}px"))
                    .style("top", &format!("{t}px"))
                    .style("width", &format!("{w}px"))
                    .style("height", &format!("{h}px"))
                    .style("border", "1px solid var(--accent-bright)")
                    .style("background", "var(--accent-ghost)")
                })
            })))
        }))
    })
}
