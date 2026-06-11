//! The top-left product mark: a gradient rounded-square holding a musical-note
//! glyph, followed by the "Awsm" + muted "Audio" wordmark. The audio sibling of
//! the awsm-renderer brand, so the two tools read as one family.

use dominator::{html, Dom};

use super::icon::Icon;
use crate::theme::ACCENT_FG;

pub fn brand() -> Dom {
    html!("a", {
        // Links home to awsm.fun — in a new tab so it never navigates away from
        // unsaved editor work.
        .attr("href", "https://awsm.fun")
        .attr("target", "_blank")
        .attr("rel", "noopener noreferrer")
        .attr("title", "awsm.fun")
        .style("text-decoration", "none")
        .style("color", "inherit")
        .style("cursor", "pointer")
        .style("display", "flex")
        .style("align-items", "center")
        .style("gap", "9px")
        .style("user-select", "none")
        // Gradient chip with the note glyph.
        .child(html!("div", {
            .style("width", "26px")
            .style("height", "26px")
            .style("border-radius", "7px")
            .style("position", "relative")
            .style("flex", "0 0 auto")
            .style("background", "linear-gradient(145deg, var(--accent-bright), var(--accent-dim))")
            .style("box-shadow", "inset 0 1px 0 oklch(1 0 0 / .25), var(--shadow-1)")
            .child(html!("div", {
                .style("position", "absolute")
                .style("inset", "0")
                .style("display", "flex")
                .style("align-items", "center")
                .style("justify-content", "center")
                .child(Icon::new("note").size(15.0).stroke_width(1.6).color(ACCENT_FG).render())
            }))
        }))
        // Wordmark: bold "Awsm" + muted "Audio".
        .child(html!("span", {
            .style("font-size", "13px")
            .style("font-weight", "680")
            .style("letter-spacing", "-0.01em")
            .style("color", "var(--text-0)")
            .text("Awsm")
            .child(html!("span", {
                .style("color", "var(--text-2)")
                .style("font-weight", "500")
                .text("Audio")
            }))
        }))
    })
}
