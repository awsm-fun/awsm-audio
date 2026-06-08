//! The example browser modal. Opened from the transport's "Load example…"
//! button, it presents the built-in examples as cards tagged with the node
//! kinds each uses — with the sequenced songs and the WASM AudioWorklet examples
//! each grouped into their own section so they're easy to discover. Clicking a
//! card loads it.

use awsm_audio_schema::{examples, NodeKind, SampleLibrary};
use dominator::{clone, events, html, Dom};
use futures_signals::signal::SignalExt;

use crate::controller::controller;
use crate::ports::kind_label;

/// The "Audio Worklet" tag label (also the marker for the WASM section).
const WORKLET_TAG: &str = "Audio Worklet";

struct Card {
    key: String,
    title: String,
    tags: Vec<&'static str>,
    worklet: bool,
    song: bool,
}

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .child_signal(ctrl.examples_open.signal().map(|open| if open { Some(view()) } else { None }))
    })
}

fn view() -> Dom {
    let cards = cards();
    // Songs first (their own section), then worklets, then everything else.
    let (songs, rest): (Vec<_>, Vec<_>) = cards.into_iter().partition(|c| c.song);
    let (worklet, builtin): (Vec<_>, Vec<_>) = rest.into_iter().partition(|c| c.worklet);
    // Full-screen wrapper holding two siblings: the click-to-close backdrop
    // *behind* the panel, and a pointer-events-transparent centering layer that
    // holds the panel. Keeping the backdrop out of the panel's ancestor chain
    // means card clicks never bubble into the close handler (dominator's
    // `Click` propagation does not honor `stop_propagation` reliably here), so a
    // card click loads its example instead of just dismissing the modal.
    html!("div", {
        .style("position", "fixed")
        .style("inset", "0")
        .style("z-index", "1000")
        // Backdrop: closes when clicked (only reachable in the empty margin,
        // since the panel sits on top in the centering layer).
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .style("background", "oklch(0 0 0 / 0.62)")
            .style("backdrop-filter", "blur(2px)")
            .style_unchecked("-webkit-backdrop-filter", "blur(2px)")
            .event(|_: events::Click| controller().close_examples())
        }))
        // Centering layer: transparent to pointer events so clicks in the empty
        // area pass through to the backdrop; the panel re-enables them.
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .style("display", "flex")
            .style("align-items", "center")
            .style("justify-content", "center")
            .style("pointer-events", "none")
            .child(html!("div", {
                .style("pointer-events", "auto")
                .style("width", "min(720px, 92vw)")
                .style("max-height", "82vh")
                .style("overflow-y", "auto")
                .style("margin", "0 16px")
                .style("padding", "20px 22px")
                .style("border-radius", "12px")
                .style("background", "var(--bg-2)")
                .style("border", "1px solid var(--line-strong)")
                .style("box-shadow", "0 24px 70px oklch(0 0 0 / 0.6)")
                .child(header())
                .apply(|b| if songs.is_empty() { b } else {
                    b.child(section("Sequenced Songs", "Multi-track sequences played through instruments", songs))
                })
                .apply(|b| if worklet.is_empty() { b } else {
                    b.child(section("WASM AudioWorklet", "Custom DSP compiled to WebAssembly", worklet))
                })
                .child(section("Built-in", "Composed from native WebAudio nodes", builtin))
            }))
        }))
    })
}

fn header() -> Dom {
    html!("div", {
        .style("display", "flex")
        .style("align-items", "center")
        .style("justify-content", "space-between")
        .style("margin-bottom", "14px")
        .child(html!("h2", {
            .style("margin", "0")
            .style("font-size", "15px")
            .text("Examples")
        }))
        .child(html!("button", {
            .style_unchecked("border", "none")
            .style("background", "transparent")
            .style("color", "var(--text-2)")
            .style("font-size", "16px")
            .style("cursor", "pointer")
            .style("line-height", "1")
            .text("×")
            .event(|_: events::Click| controller().close_examples())
        }))
    })
}

fn section(title: &str, subtitle: &str, cards: Vec<Card>) -> Dom {
    html!("div", {
        .style("margin-bottom", "18px")
        .child(html!("div", {
            .style("display", "flex")
            .style("align-items", "baseline")
            .style("gap", "8px")
            .style("margin-bottom", "8px")
            .child(html!("div", {
                .style("font-size", "11.5px")
                .style("font-weight", "700")
                .style("letter-spacing", "0.1em")
                .style("text-transform", "uppercase")
                .style("opacity", "0.6")
                .text(title)
            }))
            .child(html!("div", {
                .style("font-size", "11.5px")
                .style("opacity", "0.4")
                .text(subtitle)
            }))
        }))
        .child(html!("div", {
            .style("display", "grid")
            .style("grid-template-columns", "repeat(auto-fill, minmax(150px, 1fr))")
            .style("gap", "10px")
            .children(cards.into_iter().map(card))
        }))
    })
}

fn card(c: Card) -> Dom {
    let key = c.key.clone();
    html!("div", {
        .class("ex-card")
        .style("padding", "10px 12px")
        .style("border-radius", "8px")
        .style("background", "var(--bg-2)")
        .style("cursor", "pointer")
        .style("border", if c.worklet {
            "1px solid oklch(0.6 0.13 70)"
        } else {
            "1px solid var(--line)"
        })
        .child(html!("div", {
            .style("font-weight", "600")
            .style("margin-bottom", "8px")
            .text(&c.title)
        }))
        .child(html!("div", {
            .style("display", "flex")
            .style("flex-wrap", "wrap")
            .style("gap", "4px")
            .children(c.tags.into_iter().map(tag_chip))
        }))
        .event(clone!(key => move |_: events::Click| {
            controller().load_example(&key);
        }))
    })
}

fn tag_chip(label: &'static str) -> Dom {
    let worklet = label == WORKLET_TAG;
    html!("span", {
        .style("font-size", "11px")
        .style("padding", "1px 6px")
        .style("border-radius", "999px")
        .style("white-space", "nowrap")
        .style("background", if worklet { "oklch(0.4 0.12 70)" } else { "var(--line)" })
        .style("color", if worklet { "oklch(0.95 0.04 70)" } else { "var(--text-1)" })
        .style("font-weight", if worklet { "700" } else { "400" })
        .text(label)
    })
}

/// Build the card list from the built-in examples, deriving tags from the node
/// kinds each graph uses.
fn cards() -> Vec<Card> {
    examples::all()
        .into_iter()
        .map(|(key, lib)| {
            let tags = node_tags(&lib);
            let title = display_name(&lib).unwrap_or_else(|| key.to_string());
            let worklet = tags.contains(&WORKLET_TAG);
            // A "song" = anything driven by a sequencer node (Melodic or Drum).
            // Detect by node kind, not label, so renamed labels can't break it.
            let song = has_sequencer(&lib);
            Card {
                key: key.to_string(),
                title,
                tags,
                worklet,
                song,
            }
        })
        .collect()
}

fn display_name(lib: &SampleLibrary) -> Option<String> {
    root_sample(lib).map(|s| s.name.clone())
}

/// Distinct node-kind labels used anywhere in the library (root + every
/// sub-sample), in first-seen order. Scanning all samples means a worklet or
/// other node nested behind a `SampleRef` still surfaces as a tag.
fn node_tags(lib: &SampleLibrary) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for sample in &lib.samples {
        for node in &sample.graph.nodes {
            let label = kind_label(&node.kind);
            if !seen.contains(&label) {
                seen.push(label);
            }
        }
    }
    seen
}

/// True if any sample in the library contains a sequencer node — the marker for
/// the "Sequenced Songs" section.
fn has_sequencer(lib: &SampleLibrary) -> bool {
    lib.samples.iter().any(|s| {
        s.graph
            .nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::NoteSequencer(_)))
    })
}

fn root_sample(lib: &SampleLibrary) -> Option<&awsm_audio_schema::Sample> {
    lib.root
        .and_then(|r| lib.sample(r))
        .or_else(|| lib.samples.first())
}
