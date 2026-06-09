//! The sample tab strip: one tab per sample in the project. Click to switch
//! (the canvas commits the current sample first), `+` adds a sample, `★` marks
//! the play root, `×` deletes, double-click renames.

use dominator::{events, html, Dom};
use futures_signals::signal::SignalExt;

use awsm_audio_schema::SampleKind;

use crate::controller::controller;
use crate::theme::ACCENT_FG;
use crate::widgets::{Icon, IconBtn};

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .style("flex", "0 0 auto")
        .style("display", "flex")
        .style("align-items", "center")
        .style("gap", "8px")
        .style("padding", "6px 12px")
        .style("background", "var(--bg-1)")
        .style("border-bottom", "1px solid var(--line)")
        .style("overflow-x", "auto")
        // Re-render the whole strip whenever the sample set changes.
        .child_signal(ctrl.samples_rev.signal().map(|_| Some(strip())))
    })
}

/// The top-level Sounds ⇄ Arrange segmented toggle. Drives + reflects the
/// canonical `controller().view` so it stays in sync with the body switch.
fn view_toggle() -> Dom {
    let seg = |label: &str, icon: &str, kind: SampleKind| {
        let label = label.to_string();
        let icon = icon.to_string();
        html!("button", {
            .class("t")
            .style("display", "inline-flex")
            .style("align-items", "center")
            .style("gap", "6px")
            .style("height", "26px")
            .style("padding", "0 12px")
            .style("border", "0")
            .style("border-radius", "var(--r1)")
            .style("cursor", "pointer")
            .style("font-size", "12px")
            .style("white-space", "nowrap")
            .style_signal("font-weight", controller().view.signal().map(move |v| {
                if v == kind { "600" } else { "520" }
            }))
            .style_signal("background", controller().view.signal().map(move |v| {
                if v == kind { "var(--accent)" } else { "transparent" }
            }))
            .style_signal("color", controller().view.signal().map(move |v| {
                if v == kind { ACCENT_FG } else { "var(--text-2)" }
            }))
            .child(Icon::new(icon).size(14.0).render())
            .child(html!("span", { .text(&label) }))
            .event(move |_: events::Click| controller().switch_view(kind))
        })
    };
    html!("div", {
        .style("display", "inline-flex")
        .style("align-items", "center")
        .style("gap", "2px")
        .style("padding", "2px")
        .style("border-radius", "var(--r2)")
        .style("background", "var(--bg-3)")
        .style("border", "1px solid var(--line-soft)")
        .style("margin-right", "2px")
        .child(seg("Sounds", "speaker", SampleKind::Sound))
        .child(seg("Arrange", "wave", SampleKind::Arrangement))
    })
}

fn strip() -> Dom {
    html!("div", {
        .style("display", "flex")
        .style("align-items", "center")
        .style("gap", "6px")
        .child(view_toggle())
        .child(html!("div", {
            .style("width", "1px")
            .style("height", "22px")
            .style("background", "var(--line)")
            .style("margin", "0 2px")
        }))
        .child(picker_button())
        .child(IconBtn::new("plus")
            .title("New sample")
            .on_click(|| controller().add_sample())
            .render())
    })
}

/// The current-selection button: shows the active sample's name plus a count of
/// how many samples are in this view, and opens the filterable picker modal. This
/// replaces the inline horizontal tab list, which didn't scale.
fn picker_button() -> Dom {
    let tabs = controller().sample_tabs();
    let count = tabs.len();
    let active_name = tabs
        .iter()
        .find(|t| t.is_active)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "—".to_string());
    html!("button", {
        .class("t")
        .style("display", "inline-flex")
        .style("align-items", "center")
        .style("gap", "7px")
        .style("height", "28px")
        .style("padding", "0 9px 0 11px")
        .style("border", "1px solid var(--accent-line)")
        .style("border-radius", "var(--r2)")
        .style("background", "var(--accent-ghost)")
        .style("color", "var(--text-0)")
        .style("font-size", "12px")
        .style("white-space", "nowrap")
        .style("cursor", "pointer")
        .attr("title", "Switch sample")
        .child(html!("span", {
            .style("max-width", "180px")
            .style("overflow", "hidden")
            .style("text-overflow", "ellipsis")
            .text(&active_name)
        }))
        .child(html!("span", {
            .style("font-size", "11px")
            .style("padding", "1px 6px")
            .style("border-radius", "999px")
            .style("background", "var(--bg-3)")
            .style("color", "var(--text-2)")
            .text(&count.to_string())
        }))
        .child(html!("span", {
            .style("font-size", "10px")
            .style("opacity", "0.7")
            .text("▾")
        }))
        .event(|_: events::Click| controller().open_sample_picker())
    })
}
