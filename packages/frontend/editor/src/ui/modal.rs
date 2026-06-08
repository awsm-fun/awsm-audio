//! The node-help modal. Mounted once; visible whenever the controller's `help`
//! holds a `(title, body)`. Clicking the backdrop or the close button hides it.

use dominator::{events, html, Dom};
use futures_signals::signal::SignalExt;

use crate::controller::controller;

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .child_signal(ctrl.help.signal_cloned().map(|doc| doc.map(view)))
    })
}

fn view(doc: crate::catalog::NodeDoc) -> Dom {
    let crate::catalog::NodeDoc { title, body, mdn } = doc;
    // Two siblings: a click-to-close backdrop behind, and a pointer-events
    // transparent centering layer holding the panel. The backdrop stays out of
    // the panel's ancestor chain so panel clicks (e.g. the MDN link) never
    // bubble into the close handler — dominator's `Click` propagation does not
    // honor `stop_propagation` reliably here.
    html!("div", {
        .style("position", "fixed")
        .style("inset", "0")
        .style("z-index", "1000")
        // Backdrop: closes when its empty margin is clicked.
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .style("background", "oklch(0 0 0 / 0.62)")
            // `backdrop-filter` validates; the vendor-prefixed alias does NOT
            // (dominator panics on unknown style names), so set it unchecked.
            .style("backdrop-filter", "blur(2px)")
            .style_unchecked("-webkit-backdrop-filter", "blur(2px)")
            .event(|_: events::Click| controller().close_help())
        }))
        // Centering layer (transparent to pointer events; the panel re-enables).
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .style("display", "flex")
            .style("align-items", "center")
            .style("justify-content", "center")
            .style("pointer-events", "none")
            .child(html!("div", {
                .style("pointer-events", "auto")
                .style("max-width", "470px")
                .style("margin", "0 20px")
                .style("padding", "22px 24px")
                .style("border-radius", "12px")
                .style("background", "var(--bg-2)")
                .style("border", "1px solid var(--accent)")
                .style("box-shadow", "0 24px 70px oklch(0 0 0 / 0.6)")
                .child(html!("div", {
                    .style("display", "flex")
                    .style("align-items", "center")
                    .style("justify-content", "space-between")
                    .style("margin-bottom", "12px")
                    .child(html!("h2", {
                        .style("margin", "0")
                        .style("font-size", "17px")
                        .style("font-weight", "650")
                        .text(title)
                    }))
                    .child(html!("button", {
                        .style_unchecked("border", "none")
                        .style("background", "transparent")
                        .style("color", "var(--text-2)")
                        .style("font-size", "18px")
                        .style("cursor", "pointer")
                        .style("line-height", "1")
                        .text("×")
                        .event(|_: events::Click| controller().close_help())
                    }))
                }))
                .child(html!("p", {
                    .style("margin", "0 0 16px")
                    .style("font-size", "14px")
                    .style("line-height", "1.6")
                    .style("color", "var(--text-1)")
                    .text(body)
                }))
                .child(html!("a", {
                    .attr("href", mdn)
                    .attr("target", "_blank")
                    .attr("rel", "noopener noreferrer")
                    .style("font-size", "13.5px")
                    .style("font-weight", "550")
                    .style("color", "var(--accent-bright)")
                    .style("text-decoration", "none")
                    .text("View on MDN ↗")
                }))
            }))
        }))
    })
}
