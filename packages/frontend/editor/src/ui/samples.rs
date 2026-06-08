//! The sample tab strip: one tab per sample in the project. Click to switch
//! (the canvas commits the current sample first), `+` adds a sample, `★` marks
//! the play root, `×` deletes, double-click renames.

use dominator::{clone, events, html, Dom};
use futures_signals::signal::SignalExt;

use awsm_audio_schema::SampleKind;

use crate::controller::{controller, SampleTab};
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
        .children(controller().sample_tabs().into_iter().map(tab))
        .child(IconBtn::new("plus")
            .title("New sample")
            .on_click(|| controller().add_sample())
            .render())
    })
}

fn tab(t: SampleTab) -> Dom {
    let id = t.id;
    let can_delete = controller().sample_tabs().len() > 1;
    html!("div", {
        .class("t")
        .style("display", "flex")
        .style("align-items", "center")
        .style("gap", "5px")
        .style("height", "28px")
        .style("padding", "0 7px 0 10px")
        .style("border-radius", "var(--r2)")
        .style("font-size", "12px")
        .style("white-space", "nowrap")
        .style("cursor", "pointer")
        .style("border", if t.is_active {
            "1px solid var(--accent-line)"
        } else {
            "1px solid var(--line)"
        })
        .style("color", if t.is_active { "var(--text-0)" } else { "var(--text-1)" })
        .style("background", if t.is_active { "var(--accent-ghost)" } else { "var(--bg-3)" })
        // Click the body switches to this sample.
        .event(clone!(id => move |_: events::Click| controller().switch_sample(id)))
        // Right-click: context menu (Clone).
        .event_with_options(&dominator::EventOptions::preventable(), clone!(id => move |e: events::ContextMenu| {
            e.prevent_default();
            controller().open_sample_tab_menu(id, e.x(), e.y());
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
        // Name (double-click to rename).
        .child(html!("span", {
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
        // Delete (only if more than one sample).
        .apply(move |b| if can_delete {
            b.child(html!("span", {
                .attr("title", "Delete sample")
                .style("cursor", "pointer")
                .style("opacity", "0.55")
                .style("padding", "0 2px")
                .text("×")
                .event(clone!(id => move |e: events::Click| {
                    e.stop_propagation();
                    controller().delete_sample(id);
                }))
            }))
        } else { b })
    })
}
