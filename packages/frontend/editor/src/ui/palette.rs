//! The palette: a left sidebar of node kinds grouped into sections. Each item
//! adds a node on click; its `?` opens the help modal for that kind.

use awsm_audio_schema::NodeKind;
use dominator::{clone, events, html, with_node, Dom};
use futures_signals::signal::{Mutable, SignalExt};

use crate::catalog::{self, Section};
use crate::controller::{controller, BoundaryPort, EditorCommand, PaletteDrag};
use crate::ports::kind_label;

/// Mark a palette button draggable and arm the controller's palette-drag on
/// `dragstart`, so dropping it on the canvas places the node at the cursor.
/// `make` is cloned per event (PaletteDrag isn't Copy).
fn draggable(
    builder: dominator::DomBuilder<web_sys::HtmlElement>,
    make: impl Fn() -> PaletteDrag + 'static,
) -> dominator::DomBuilder<web_sys::HtmlElement> {
    builder
        .attr("draggable", "true")
        .event(move |e: events::DragStart| {
            // Firefox only starts a drag if data is set on the transfer.
            if let Some(dt) = e.data_transfer() {
                let _ = dt.set_data("text/plain", "node");
                dt.set_effect_allowed("copy");
            }
            controller().begin_palette_drag(make());
        })
}

pub fn render() -> Dom {
    // Live text filter over node names (created once at mount, so it persists).
    let filter = Mutable::new(String::new());
    html!("div", {
        .style("width", "186px")
        .style("flex", "0 0 auto")
        .style("overflow-y", "auto")
        .style("padding", "10px")
        .style("box-sizing", "border-box")
        .style("background", "var(--bg-1)")
        .style("border-right", "1px solid var(--line)")
        .child(search_box(filter.clone()))
        // Re-render on filter text. One unified palette — every node is always
        // available; the typed port matrix governs what can wire to what.
        .child_signal(filter.signal_cloned().map(|q| Some(palette_list(&q))))
    })
}

/// The filter text field at the top of the palette: a search icon + input on a
/// recessed well.
fn search_box(filter: Mutable<String>) -> Dom {
    html!("div", {
        .style("display", "flex")
        .style("align-items", "center")
        .style("gap", "6px")
        .style("margin-bottom", "12px")
        .style("padding", "0 8px")
        .style("height", "30px")
        .style("border", "1px solid var(--line-soft)")
        .style("border-radius", "var(--r2)")
        .style("background", "var(--bg-3)")
        .style("color", "var(--text-3)")
        .child(crate::widgets::Icon::new("search").size(14.0).render())
        .child(html!("input" => web_sys::HtmlInputElement, {
            .attr("type", "search")
            .attr("placeholder", "Search nodes…")
            .style("flex", "1")
            .style("min-width", "0")
            .style("border", "0")
            .style("outline", "none")
            .style("background", "transparent")
            .style("font-size", "12px")
            .style("color", "var(--text-0)")
            .with_node!(input => {
                .event(clone!(input => move |_: events::Input| filter.set(input.value())))
            })
        }))
    })
}

/// The (filtered) palette body: matching node sections + the composition items,
/// plus a hint when the query looks like a non-node concept (e.g. "macro").
fn palette_list(query: &str) -> Dom {
    let q = query.trim().to_lowercase();
    let matches = |label: &str| q.is_empty() || label.to_lowercase().contains(&q);

    // Every section is always available — a Sound is any graph, so sequencing,
    // output, and source nodes all live in one palette.
    let sections: Vec<Section> = catalog::sections()
        .into_iter()
        .map(|s| Section {
            name: s.name,
            kinds: s
                .kinds
                .into_iter()
                .filter(|k| matches(kind_label(k)))
                .collect(),
        })
        .filter(|s| !s.kinds.is_empty())
        .collect();

    // Composition items: boundary ports (Input/Output) for reusable sub-sounds,
    // and a reference to another Sound.
    let comp: Vec<(&str, PaletteDrag)> = [
        ("Input", PaletteDrag::Inlet),
        ("Output", PaletteDrag::Outlet),
        ("Sound", PaletteDrag::SampleRef),
    ]
    .into_iter()
    .filter(|(l, _)| matches(l))
    .collect();

    let empty = sections.is_empty() && comp.is_empty();
    // Things people search for that aren't nodes — point them the right way.
    let conceptual = !q.is_empty()
        && [
            "macro",
            "param",
            "knob",
            "control",
            "expose",
            "automation",
            "envelope",
            "midi",
            "cc",
            "velocity",
            "modulat",
        ]
        .iter()
        .any(|k| q.contains(k));

    html!("div", {
        .apply(|b| if conceptual {
            b.child(hint("Exposed controls, modulation and automation aren’t nodes. To expose a parameter as a knob, drop an Input node and wire it to that parameter. To modulate, drag a node's output onto a small param dot. To automate over time, draw an envelope on the parameter (inspector) or wire a Control Sequencer lane to it."))
        } else { b })
        .children(sections.into_iter().map(section))
        .apply(move |b| if comp.is_empty() { b } else { b.child(composition_section(comp)) })
        .apply(move |b| if empty && !conceptual {
            b.child(hint("No nodes match."))
        } else { b })
    })
}

/// A small muted note in the palette body.
fn hint(text: &str) -> Dom {
    html!("div", {
        .style("font-size", "11.5px")
        .style("line-height", "1.5")
        .style("opacity", "0.5")
        .style("margin-bottom", "12px")
        .text(text)
    })
}

/// The "Composition" section: boundary ports + a sample-reference node. These
/// aren't plain `NodeKind`s, so they call dedicated controller methods.
fn composition_section(items: Vec<(&'static str, PaletteDrag)>) -> Dom {
    html!("div", {
        .style("margin-bottom", "14px")
        .child(html!("div", {
            .class("kicker")
            .style("margin-bottom", "6px")
            .text("Composition")
        }))
        .children(items.into_iter().map(|(label, drag)| {
            comp_button(label, drag, move || match label {
                "Input" => controller().add_boundary(BoundaryPort::Inlet),
                "Output" => controller().add_boundary(BoundaryPort::Outlet),
                _ => controller().add_sample_ref(),
            })
        }))
    })
}

fn comp_button(label: &str, drag: PaletteDrag, on_click: impl Fn() + 'static) -> Dom {
    // PaletteDrag isn't Copy; clone into the per-event closure factory.
    let drag = std::rc::Rc::new(drag);
    html!("button", {
        .style("width", "100%")
        .style("text-align", "left")
        .style("padding", "5px 9px")
        .style("margin-bottom", "4px")
        .style("font-size", "12px")
        .style("border", "1px solid var(--line-strong)")
        .style("border-radius", "5px")
        .style("background", "var(--bg-2)")
        .style("color", "var(--text-0)")
        .style("cursor", "grab")
        .text(label)
        .apply(clone!(drag => move |b| draggable(b, move || (*drag).clone())))
        .event(move |_: events::Click| on_click())
    })
}

fn section(s: Section) -> Dom {
    html!("div", {
        .style("margin-bottom", "14px")
        .child(html!("div", {
            .class("kicker")
            .style("margin-bottom", "6px")
            .text(s.name)
        }))
        .children(s.kinds.into_iter().map(item))
    })
}

fn item(kind: NodeKind) -> Dom {
    let help_kind = kind.clone();
    html!("div", {
        .style("display", "flex")
        .style("align-items", "stretch")
        .style("gap", "4px")
        .style("margin-bottom", "4px")
        // Add-node button (most of the row).
        .child(html!("button", {
            .style("flex", "1")
            .style("text-align", "left")
            .style("padding", "5px 9px")
            .style("font-size", "12px")
            .style("border", "1px solid var(--line-strong)")
            .style("border-radius", "5px")
            .style("background", "var(--bg-2)")
            .style("color", "var(--text-0)")
            .style("cursor", "grab")
            .attr("title", "Click to add at center, or drag onto the canvas to place it")
            .text(kind_label(&kind))
            // Drag onto the canvas to drop the node at the cursor.
            .apply(clone!(kind => move |b| draggable(b, clone!(kind => move || PaletteDrag::Node(Box::new(kind.clone()))))))
            // Click still adds at the viewport center (with a small cascade so
            // repeats don't stack exactly on top of one another).
            .event(clone!(kind => move |_: events::Click| {
                let ctrl = controller();
                let n = ctrl.nodes.lock_ref().len() as f64;
                let (cx, cy) = ctrl.world_center();
                let step = (n % 6.0) * 22.0;
                let x = cx - 86.0 + step; // ~half node width, to center it
                let y = cy - 36.0 + step;
                ctrl.dispatch(EditorCommand::AddNode { kind: kind.clone(), x, y });
            }))
        }))
        // Help button.
        .child(html!("button", {
            .attr("title", "What does this node do?")
            .style("flex", "0 0 auto")
            .style("width", "26px")
            .style("border", "1px solid var(--line-strong)")
            .style("border-radius", "5px")
            .style("background", "var(--bg-3)")
            .style("color", "var(--text-2)")
            .style("cursor", "pointer")
            .style("font-weight", "700")
            .text("?")
            .event(clone!(help_kind => move |e: events::Click| {
                e.stop_propagation();
                controller().show_help(catalog::doc(&help_kind));
            }))
        }))
    })
}
