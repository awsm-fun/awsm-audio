//! The SVG wire overlay. One `<path>` per connection, its `d` derived live from
//! the endpoint nodes' position signals so wires track nodes with no manual
//! redraw. A pending (being-dragged) wire follows the cursor.

use std::rc::Rc;

use dominator::{clone, events, html, svg, Dom};
use futures_signals::map_ref;
use futures_signals::signal::SignalExt;
use futures_signals::signal_vec::SignalVecExt;

use crate::controller::{controller, EditorConnection, PendingWire};
use crate::ports::{self, PortSide};

/// The full overlay: pointer-transparent SVG layers hosting every wire. The
/// committed wires and the pending wire live in separate `<svg>` elements so the
/// `children_signal_vec` and `child_signal` don't share one parent. SVG elements
/// take CSS via the `style` attribute (dominator's `.style` is HtmlElement-only).
const SVG_STYLE: &str =
    "position:absolute; left:0; top:0; width:1px; height:1px; overflow:visible; pointer-events:none;";

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .style("position", "absolute")
        .style("left", "0")
        .style("top", "0")
        .style("width", "0")
        .style("height", "0")
        .style("overflow", "visible")
        .style("pointer-events", "none")
        .child(svg!("svg", {
            .attr("style", SVG_STYLE)
            .children_signal_vec(ctrl.connections.signal_vec_cloned().map(render_wire))
        }))
        .child(svg!("svg", {
            .attr("style", SVG_STYLE)
            .child_signal(ctrl.pending.signal_cloned().map(|p| p.map(render_pending)))
        }))
    })
}

/// A cubic bézier path string between two world points, bowed horizontally.
fn bezier(ax: f64, ay: f64, bx: f64, by: f64) -> String {
    let dx = ((bx - ax).abs() * 0.5).max(40.0);
    format!(
        "M {ax} {ay} C {} {ay}, {} {by}, {bx} {by}",
        ax + dx,
        bx - dx
    )
}

fn render_wire(conn: Rc<EditorConnection>) -> Dom {
    use crate::controller::ConnSink;
    let from = conn.from.clone();
    let to = conn.to.clone();
    let from_out = conn.from_output;
    let conn_id = conn.id;
    // Resolve the landing inlet index + color from the sink (fixed; position
    // comes from the live node signals below). Modulation wires are orange.
    let (to_index, stroke) = match &conn.sink {
        ConnSink::Input(i) => (*i, "oklch(0.72 0.13 230)"),
        ConnSink::Param(p) => (
            ports::param_inlet_index(&to.kind.borrow(), p).unwrap_or(0),
            "oklch(0.78 0.16 70)",
        ),
        // Trigger bindings land on the instrument-ref's trigger inlet (port 0)
        // and are drawn amber-green to read as "play this" rather than audio.
        ConnSink::Trigger => (0, "oklch(0.8 0.17 130)"),
    };

    let d = clone!(from, to => move || {
        map_ref! {
            let a = from.pos.signal(), let b = to.pos.signal() => {
                let (ox, oy) = ports::port_offset(PortSide::Out, from_out);
                let (ix, iy) = ports::port_offset(PortSide::In, to_index);
                bezier(a.0 + ox, a.1 + oy, b.0 + ix, b.1 + iy)
            }
        }
    });

    // A group: a wide transparent hit-path (right-click to delete) under the
    // visible thin wire. `pointer-events:stroke` re-enables hits even though the
    // parent SVG is pointer-transparent.
    svg!("g", {
        .child(svg!("path", {
            .class("wire-hit")
            .attr("fill", "none")
            .attr("stroke", "transparent")
            .attr("stroke-width", "14")
            .attr("style", "pointer-events:stroke; cursor:pointer;")
            .attr_signal("d", d())
            .attr("title", "right-click for menu")
            .event_with_options(&dominator::EventOptions::preventable(), move |e: events::ContextMenu| {
                e.prevent_default();
                e.stop_propagation();
                controller().open_wire_menu(conn_id, e.x(), e.y());
            })
        }))
        .child(svg!("path", {
            .class("wire-line")
            .attr("fill", "none")
            .attr("stroke", stroke)
            .attr("stroke-width", "2.5")
            .attr("style", "pointer-events:none;")
            .attr_signal("d", d())
        }))
    })
}

fn render_pending(pw: Rc<PendingWire>) -> Dom {
    let from = pw.from.clone();
    let from_out = pw.from_output;

    svg!("path", {
        .attr("fill", "none")
        .attr("stroke", "#5b8dd6")
        .attr("stroke-width", "2.5")
        .attr("stroke-dasharray", "6 4")
        .attr_signal("d", map_ref! {
            let a = from.pos.signal(), let c = pw.cursor.signal() => {
                let (ox, oy) = ports::port_offset(PortSide::Out, from_out);
                bezier(a.0 + ox, a.1 + oy, c.0, c.1)
            }
        })
    })
}
