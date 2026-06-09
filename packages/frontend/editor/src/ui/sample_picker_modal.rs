//! The sample picker modal — the scalable replacement for the inline sample tab
//! strip. The strip shows only the active sample as a button; clicking it opens
//! this modal, a filterable list of every sample in the current view (Sounds or
//! Arrangements) that grows indefinitely. Each row carries the same affordances
//! the old tabs had: click to switch, ★ toggles the play root, × deletes,
//! double-click renames; a `+ New` header button adds one.
//!
//! Mounted once in [`crate::ui`]; visible whenever the controller's
//! `sample_picker_open` is set.

use dominator::{clone, events, html, with_node, Dom};
use futures_signals::map_ref;
use futures_signals::signal::{Mutable, SignalExt};

use awsm_audio_schema::SampleKind;

use crate::controller::{controller, SampleTab};

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .child_signal(ctrl.sample_picker_open.signal().map(|open| if open { Some(view()) } else { None }))
    })
}

fn view() -> Dom {
    // Per-open filter state; resets each time the modal is re-created.
    let filter = Mutable::new(String::new());
    // Same backdrop / centering-layer split as the other modals: the click-to-close
    // backdrop sits behind the panel so row clicks never bubble into the close
    // handler (dominator's `Click` propagation doesn't honor stop_propagation here).
    html!("div", {
        .style("position", "fixed")
        .style("inset", "0")
        .style("z-index", "1000")
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .style("background", "oklch(0 0 0 / 0.62)")
            .style("backdrop-filter", "blur(2px)")
            .style_unchecked("-webkit-backdrop-filter", "blur(2px)")
            .event(|_: events::Click| controller().close_sample_picker())
        }))
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .style("display", "flex")
            .style("align-items", "center")
            .style("justify-content", "center")
            .style("pointer-events", "none")
            .child(html!("div", {
                .style("pointer-events", "auto")
                .style("width", "min(460px, 92vw)")
                .style("max-height", "78vh")
                .style("display", "flex")
                .style("flex-direction", "column")
                .style("margin", "0 16px")
                .style("padding", "18px 20px")
                .style("border-radius", "12px")
                .style("background", "var(--bg-2)")
                .style("border", "1px solid var(--line-strong)")
                .style("box-shadow", "0 24px 70px oklch(0 0 0 / 0.6)")
                .child(header())
                .child(filter_input(filter.clone()))
                // The list reacts to the filter text, the sample set, and the view.
                .child(html!("div", {
                    .style("overflow-y", "auto")
                    .style("margin-top", "10px")
                    .style("display", "flex")
                    .style("flex-direction", "column")
                    .style("gap", "3px")
                    .child_signal(map_ref! {
                        let needle = filter.signal_cloned(),
                        let _rev = controller().samples_rev.signal(),
                        let _view = controller().view.signal() =>
                        Some(list(needle.clone()))
                    })
                }))
            }))
        }))
    })
}

fn header() -> Dom {
    html!("div", {
        .style("display", "flex")
        .style("align-items", "center")
        .style("justify-content", "space-between")
        .style("margin-bottom", "12px")
        .child(html!("h2", {
            .style("margin", "0")
            .style("font-size", "15px")
            .style("font-weight", "650")
            .text_signal(controller().view.signal().map(|v| match v {
                SampleKind::Sound => "Sounds",
                SampleKind::Arrangement => "Arrangements",
            }))
        }))
        .child(html!("div", {
            .style("display", "flex")
            .style("align-items", "center")
            .style("gap", "8px")
            .child(html!("button", {
                .class("t")
                .style("display", "inline-flex")
                .style("align-items", "center")
                .style("gap", "5px")
                .style("height", "26px")
                .style("padding", "0 10px")
                .style("border", "1px solid var(--line)")
                .style("border-radius", "var(--r2)")
                .style("background", "var(--bg-3)")
                .style("color", "var(--text-1)")
                .style("font-size", "12px")
                .style("cursor", "pointer")
                .text("+ New")
                .event(|_: events::Click| controller().add_sample())
            }))
            .child(html!("button", {
                .style_unchecked("border", "none")
                .style("background", "transparent")
                .style("color", "var(--text-2)")
                .style("font-size", "18px")
                .style("cursor", "pointer")
                .style("line-height", "1")
                .text("×")
                .event(|_: events::Click| controller().close_sample_picker())
            }))
        }))
    })
}

fn filter_input(filter: Mutable<String>) -> Dom {
    html!("input" => web_sys::HtmlInputElement, {
        .style("width", "100%")
        .style("box-sizing", "border-box")
        .style("padding", "8px 10px")
        .style("font-size", "13px")
        .style("border-radius", "8px")
        .style("border", "1px solid var(--line)")
        .style("background", "var(--bg-1)")
        .style("color", "var(--text-1)")
        .attr("type", "text")
        .attr("spellcheck", "false")
        .attr("placeholder", "Filter…")
        .with_node!(input => {
            .event(clone!(filter => move |_: events::Input| {
                filter.set(input.value());
            }))
        })
    })
}

/// Build the filtered list of rows (or an empty-state line).
fn list(needle: String) -> Dom {
    let needle = needle.trim().to_lowercase();
    let tabs = controller().sample_tabs();
    let can_delete = tabs.len() > 1;
    let matched: Vec<SampleTab> = tabs
        .into_iter()
        .filter(|t| needle.is_empty() || t.name.to_lowercase().contains(&needle))
        .collect();
    if matched.is_empty() {
        return html!("div", {
            .style("padding", "14px 4px")
            .style("font-size", "13px")
            .style("color", "var(--text-3)")
            .text("No matches")
        });
    }
    html!("div", {
        .style("display", "flex")
        .style("flex-direction", "column")
        .style("gap", "3px")
        .children(matched.into_iter().map(move |t| row(t, can_delete)))
    })
}

fn row(t: SampleTab, can_delete: bool) -> Dom {
    let id = t.id;
    html!("div", {
        .class("t")
        .style("display", "flex")
        .style("align-items", "center")
        .style("gap", "8px")
        .style("height", "32px")
        .style("padding", "0 8px 0 10px")
        .style("border-radius", "var(--r2)")
        .style("font-size", "13px")
        .style("white-space", "nowrap")
        .style("cursor", "pointer")
        .style("border", if t.is_active {
            "1px solid var(--accent-line)"
        } else {
            "1px solid transparent"
        })
        .style("color", if t.is_active { "var(--text-0)" } else { "var(--text-1)" })
        .style("background", if t.is_active { "var(--accent-ghost)" } else { "transparent" })
        // Click the row: switch to this sample and close the picker.
        .event(clone!(id => move |_: events::Click| {
            controller().switch_sample(id);
            controller().close_sample_picker();
        }))
        // Root toggle (★ filled = root).
        .child(html!("span", {
            .attr("title", "Set as play root")
            .style("cursor", "pointer")
            .style("color", if t.is_root { "var(--warn)" } else { "var(--text-3)" })
            .text(if t.is_root { "★" } else { "☆" })
            .event_with_options(&dominator::EventOptions::bubbles(), clone!(id => move |e: events::Click| {
                e.stop_propagation();
                controller().set_root(id);
            }))
        }))
        // Name (double-click to rename); fills the row.
        .child(html!("span", {
            .style("flex", "1")
            .style("overflow", "hidden")
            .style("text-overflow", "ellipsis")
            .text(&t.name)
            .event(clone!(id => move |e: events::DoubleClick| {
                e.stop_propagation();
                if let Some(win) = web_sys::window() {
                    if let Ok(Some(name)) = win.prompt_with_message("Rename sample") {
                        if !name.trim().is_empty() {
                            controller().rename_sample(id, name);
                        }
                    }
                }
            }))
        }))
        // Clone (the old tab right-click affordance, inlined so it stays reachable
        // — the shared context menu renders below the modal).
        .child(html!("span", {
            .attr("title", "Clone sample")
            .style("cursor", "pointer")
            .style("opacity", "0.55")
            .style("padding", "0 4px")
            .style("font-size", "12px")
            .text("⧉")
            .event(clone!(id => move |e: events::Click| {
                e.stop_propagation();
                controller().clone_sample(id);
            }))
        }))
        // Delete (only if more than one sample remains).
        .apply(move |b| if can_delete {
            b.child(html!("span", {
                .attr("title", "Delete sample")
                .style("cursor", "pointer")
                .style("opacity", "0.55")
                .style("padding", "0 4px")
                .text("×")
                .event(clone!(id => move |e: events::Click| {
                    e.stop_propagation();
                    controller().delete_sample(id);
                }))
            }))
        } else { b })
    })
}
