//! Right-side inspector: when a single node is selected, edit each of its
//! AudioParams' automation timeline (envelopes). Re-renders on selection or
//! `inspector_rev` changes; inputs commit on `change` (blur/enter) so a
//! re-render never steals focus mid-edit.
//!
//! Supports the three value+time event kinds (`set` / `linear` / `exp`) — enough
//! to author attack/decay/sweep envelopes. Other event kinds (set-target, value
//! curves) from loaded files are shown but only removable.

use std::cell::RefCell;
use std::rc::Rc;

use awsm_audio_schema::{AutomationEvent, NodeId, NodeKind, WaveShaperShape};
use dominator::{clone, events, html, svg, with_node, Dom};
use futures_signals::map_ref;
use futures_signals::signal::{Mutable, SignalExt};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::controller::{controller, ControlOp, EditorCommand, EditorNode, EnvDrag, SongOp};
use crate::fields;
use crate::ports;

/// A live `requestAnimationFrame` handle (id + kept-alive closure) for the
/// per-node Analyser scope; cancelled when the scope element is removed.
type RafHandle = Rc<RefCell<Option<(i32, Closure<dyn FnMut()>)>>>;

const EVENT_TYPES: &[&str] = &["set", "linear", "exp"];

// Envelope plot geometry.
const PW: f64 = 224.0;
const PH: f64 = 96.0;
const PAD: f64 = 10.0;

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .style("width", "250px")
        .style("flex", "0 0 auto")
        .style("overflow-y", "auto")
        .style("box-sizing", "border-box")
        .style("padding", "12px")
        .style("background", "var(--bg-1)")
        .style("border-left", "1px solid var(--line)")
        .style("font-size", "12px")
        .child_signal(map_ref! {
            let id = ctrl.inspected.signal(),
            let _rev = ctrl.inspector_rev.signal() =>
            Some(match id.and_then(|nid| controller().node_by_id(nid)) {
                Some(node) => panel(node),
                None => inputs_panel(),
            })
        })
    })
}

/// Shown when nothing is selected: the active sample's inputs (its inlets).
fn inputs_panel() -> Dom {
    let inputs = controller().active_inputs();
    html!("div", {
        .child(html!("div", {
            .style("font-weight", "700")
            .style("margin-bottom", "8px")
            .text("Sample inputs")
        }))
        .child(html!("div", {
            .style("opacity", "0.45")
            .style("line-height", "1.5")
            .style("margin-bottom", "10px")
            .text("Add an Input node and wire it to a parameter. Its value sets that parameter (and can be overridden per Sample instance, or automated by a Control Sequencer). A signal wired into the input instead adds to the parameter.")
        }))
        .apply(|b| if inputs.is_empty() {
            b.child(html!("div", { .style("opacity", "0.4").text("No inputs yet.") }))
        } else {
            b.children(inputs.into_iter().map(input_row))
        })
    })
}

/// One input (inlet): its name, value, set/add mode, MIDI chip, and what it
/// drives.
fn input_row(node: Rc<EditorNode>) -> Dom {
    let id = node.id;
    let raw = node.label.get_cloned();
    let name = if raw.trim().is_empty() {
        "in".to_string()
    } else {
        raw
    };
    let targets = controller().input_targets(id);
    let controls = if targets.is_empty() {
        "controls nothing".to_string()
    } else {
        format!("→ {}", targets.join(", "))
    };
    html!("div", {
        .style("margin-bottom", "9px")
        .child(html!("div", {
            .style("display", "flex")
            .style("align-items", "center")
            .style("justify-content", "space-between")
            .style("gap", "6px")
            .child(html!("span", { .style("font-weight", "600").text(&name) }))
            .child(html!("div", {
                .style("display", "flex")
                .style("align-items", "center")
                .style("gap", "4px")
                .child(num_input(node.default.get() as f64, move |v| {
                    controller().set_input_default(id, v as f32);
                }))
            }))
        }))
        .child(html!("div", {
            .style("font-size", "11px")
            .style("opacity", "0.5")
            .style("line-height", "1.4")
            .text(&controls)
        }))
    })
}

/// A "name" text field that renames the node (empty falls back to the type name).
fn rename_row(node: Rc<EditorNode>) -> Dom {
    let id = node.id;
    html!("input" => web_sys::HtmlInputElement, {
        .attr("type", "text")
        .attr("placeholder", "name…")
        .attr("value", &node.label.get_cloned())
        .style("width", "100%")
        .style("box-sizing", "border-box")
        .style("margin-bottom", "12px")
        .style("background", "var(--bg-2)")
        .style("color", "inherit")
        .style("border", "1px solid var(--line-strong)")
        .style("border-radius", "4px")
        .style("padding", "3px 6px")
        .style("font-size", "12px")
        .with_node!(input => {
            .event(clone!(input => move |_: events::Input| {
                controller().rename_node(id, input.value());
            }))
        })
    })
}

/// One input value on a Sample-ref node: editable per-instance value (defaults
/// to the referenced sample's input default until set).
fn input_value_row(node: Rc<EditorNode>, name: String, default: f32) -> Dom {
    let id = node.id;
    let current = controller().input_value(id, &name).unwrap_or(default);
    html!("div", {
        .style("display", "flex")
        .style("align-items", "center")
        .style("justify-content", "space-between")
        .style("gap", "6px")
        .style("margin-bottom", "5px")
        .child(html!("span", { .style("font-weight", "600").text(&name) }))
        .child(num_input(current as f64, clone!(name => move |v| {
            controller().set_input_value(id, &name, v as f32);
        })))
    })
}

/// A label + control row used throughout the sequencer panel.
fn labeled_row(label: &str, control: Dom) -> Dom {
    html!("div", {
        .style("display", "flex")
        .style("align-items", "center")
        .style("justify-content", "space-between")
        .style("gap", "6px")
        .style("margin-bottom", "5px")
        .child(html!("span", { .style("opacity", "0.7").text(label) }))
        .child(control)
    })
}

/// The `.mid` file picker for a sequencer node.
fn midi_file_button(id: NodeId) -> Dom {
    html!("input" => web_sys::HtmlInputElement, {
        .attr("type", "file")
        .attr("accept", ".mid,.midi,audio/midi")
        .style("width", "100%")
        .style("box-sizing", "border-box")
        .style("margin-bottom", "8px")
        .style("font-size", "11.5px")
        .with_node!(input => {
            .event(clone!(input => move |_: events::Change| {
                if let Some(f) = input.files().and_then(|fs| fs.get(0)) {
                    controller().load_midi_file(id, f);
                }
            }))
        })
    })
}

/// One sound output: its label, the track/note it plays, transpose and gain.
/// Wire the matching output port to an instrument on the canvas.
fn sound_row(id: NodeId, idx: usize, out: &awsm_audio_schema::SoundOut) -> Dom {
    let detail = match out.note {
        Some(n) => format!(
            "track {} \u{b7} {}",
            out.track + 1,
            awsm_audio_schema::gm_drum_name(n).unwrap_or("perc")
        ),
        None => format!("track {}", out.track + 1),
    };
    html!("div", {
        .style("border", "1px solid var(--line)")
        .style("border-radius", "5px")
        .style("padding", "6px")
        .style("margin-bottom", "6px")
        .child(html!("div", {
            .style("display", "flex")
            .style("align-items", "center")
            .style("justify-content", "space-between")
            .style("margin-bottom", "5px")
            .child(html!("span", {
                .style("font-weight", "700")
                .text(&format!("out {} \u{2192} {}", idx + 1, out.label))
            }))
            .child(html!("span", {
                .style("opacity", "0.5")
                .style("font-size", "11px")
                .text(&detail)
            }))
        }))
        .child(labeled_row("name", html!("input" => web_sys::HtmlInputElement, {
            .apply(small_input)
            .style("max-width", "150px")
            .attr("value", &out.label)
            .with_node!(input => {
                .event(clone!(input => move |_: events::Change| {
                    controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetOutputLabel { index: idx, label: input.value() } });
                }))
            })
        })))
        .child(labeled_row("transpose", num_input(out.transpose as f64, move |v| {
            controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetOutputTranspose { index: idx, semitones: v as i32 } });
        })))
        .child(labeled_row("gain", num_input(out.gain as f64, move |v| {
            controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetOutputGain { index: idx, gain: v as f32 } });
        })))
    })
}

/// The Note Sequencer inspector: load a song, set tempo/seek/loop, edit notes,
/// and tune each sound output \u{2014} then wire each output to an instrument.
fn midisong_panel(node: Rc<EditorNode>) -> Dom {
    let id = node.id;
    let Some(ms) = controller().song_node(id) else {
        return html!("div", {});
    };
    let has_song = !ms.song.tracks.is_empty();
    let looping = ms.looping;

    html!("div", {
        .child(html!("div", {
            .style("font-weight", "700")
            .style("margin-bottom", "8px")
            .text(if ms.mode.is_drum() { "Drum Sequencer" } else { "Melodic Sequencer" })
        }))
        .child(rename_row(node.clone()))
        .child(midi_file_button(id))
        .apply(|b| if has_song {
            b.child(labeled_row("tempo (BPM)", num_input(ms.song.bpm, move |v| controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetBpm(v) }))))
             .child(labeled_row("start (beats)", num_input(ms.start, move |v| controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetStart(v) }))))
             // Playback-window stop; 0 = play to the song's end.
             .child(labeled_row("stop (beats)", num_input(ms.end.unwrap_or(0.0), move |v| {
                let end = if v > 0.0 { Some(v) } else { None };
                controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetEnd(end) });
             })))
             // Authored grid length; 0 = auto-fit the notes.
             .child(labeled_row("length (beats)", num_input(ms.length, move |v| controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetLength(v) }))))
             .child(labeled_row("loop", html!("input" => web_sys::HtmlInputElement, {
                .attr("type", "checkbox")
                .apply(move |b| if looping { b.attr("checked", "") } else { b })
                .with_node!(cb => {
                    .event(clone!(cb => move |_: events::Change| controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::SetLooping(cb.checked()) })))
                })
             })))
        } else {
            b.child(html!("div", {
                .style("opacity", "0.45")
                .style("line-height", "1.5")
                .style("margin-bottom", "6px")
                .text("Load a .mid file, or add a track and draw notes in the piano roll.")
            }))
            .child(html!("button", {
                .style("width", "100%")
                .style("padding", "4px")
                .style("cursor", "pointer")
                .style("background", "var(--bg-2)")
                .style("color", "inherit")
                .style("border", "1px solid var(--line-strong)")
                .style("border-radius", "4px")
                .style("font-size", "11.5px")
                .text("+ Add track")
                .event(move |_: events::Click| controller().dispatch(EditorCommand::EditSong { node: id, op: SongOp::AddTrack }))
            }))
        })
        // One song-level piano-roll entry (its tabs switch between tracks).
        .apply(clone!(ms => move |b| if has_song {
            let first = ms.outputs.first().map(|o| o.track).unwrap_or(0);
            b.child(html!("button", {
                .style("width", "100%")
                .style("margin-top", "8px")
                .style("padding", "5px")
                .style("cursor", "pointer")
                .style("background", "var(--accent-dim)")
                .style("color", "inherit")
                .style("border", "1px solid var(--accent-dim)")
                .style("border-radius", "4px")
                .style("font-size", "12px")
                .text("Piano roll \u{266a}")
                .event(move |_: events::Click| controller().open_piano_roll(id, first))
            }))
        } else {
            b
        }))
        .apply(clone!(ms => move |b| if ms.outputs.is_empty() {
            b
        } else {
            b.child(html!("div", {
                .style("font-weight", "700")
                .style("margin", "10px 0 5px")
                .text("Sounds")
            }))
            .children(ms.outputs.iter().enumerate().map(|(i, o)| sound_row(id, i, o)).collect::<Vec<_>>())
            .child(html!("div", {
                .style("opacity", "0.45")
                .style("line-height", "1.5")
                .style("margin-top", "8px")
                .text("Each sound is an output port. Wire it to an instrument \u{2014} a Sample node, or any node (e.g. an Oscillator) \u{2014} and that sound plays it. Press play to perform the whole song.")
            }))
        }))
    })
}

/// A `.mid` file picker that imports controller-change automation as lanes.
fn cc_file_button(id: NodeId) -> Dom {
    html!("input" => web_sys::HtmlInputElement, {
        .attr("type", "file")
        .attr("accept", ".mid,.midi,audio/midi")
        .style("width", "100%")
        .style("box-sizing", "border-box")
        .style("margin-bottom", "8px")
        .style("font-size", "11.5px")
        .with_node!(input => {
            .event(clone!(input => move |_: events::Change| {
                if let Some(f) = input.files().and_then(|fs| fs.get(0)) {
                    controller().load_midi_cc(id, f);
                }
            }))
        })
    })
}

/// A compact automation plot for one control lane: click empty space to add a
/// breakpoint, click a dot to delete it. Auto-scales to the lane's points
/// (always covering at least 4 beats and the 0..1 value band).
fn lane_plot(id: NodeId, idx: usize, points: &[awsm_audio_schema::ControlPoint]) -> Dom {
    let tmax = points.iter().map(|p| p.beat).fold(4.0_f64, f64::max);
    let vmin = points.iter().map(|p| p.value as f64).fold(0.0_f64, f64::min);
    let vmax = points.iter().map(|p| p.value as f64).fold(1.0_f64, f64::max);
    let host: Rc<RefCell<Option<web_sys::HtmlElement>>> = Rc::new(RefCell::new(None));
    // Sorted-by-beat path through the breakpoints. Each segment is drawn in the
    // shape of the *target* point's curve, so the plot matches what plays back.
    let mut sorted: Vec<&awsm_audio_schema::ControlPoint> = points.iter().collect();
    sorted.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
    let d = curve_path(&sorted, tmax, vmin, vmax);

    html!("div" => web_sys::HtmlElement, {
        .style("position", "relative")
        .style("width", &format!("{PW}px"))
        .style("height", &format!("{PH}px"))
        .style("margin", "2px 0 6px")
        .style("touch-action", "none")
        .after_inserted(clone!(host => move |el| { *host.borrow_mut() = Some(el); }))
        .child(svg!("svg", {
            .attr("style", &format!("position:absolute; left:0; top:0; width:{PW}px; height:{PH}px;"))
            .child(svg!("rect", {
                .attr("x", "0.5")
                .attr("y", "0.5")
                .attr("width", &format!("{}", PW - 1.0))
                .attr("height", &format!("{}", PH - 1.0))
                .attr("rx", "6")
                .attr("fill", "oklch(0.155 0.006 255)")
                .attr("stroke", "oklch(0.315 0.008 255)")
                .attr("style", "cursor:crosshair;")
                .event(clone!(host => move |e: events::PointerDown| {
                    if let Some(el) = host.borrow().as_ref() {
                        if let Some((t, v)) = unmap(el, e.x(), e.y(), tmax, vmin, vmax) {
                            controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::AddPoint { lane: idx, beat: t, value: v } });
                        }
                    }
                }))
            }))
            .child(svg!("path", {
                .attr("fill", "none")
                .attr("stroke", "oklch(0.8 0.14 90)")
                .attr("stroke-width", "2")
                .attr("style", "pointer-events:none;")
                .attr("d", &d)
            }))
            .children(sorted.iter().enumerate().map(move |(i, p)| {
                // The point index is into the *sorted* order, which equals the
                // stored order because the controller keeps points beat-sorted.
                // Click cycles the incoming curve; Alt/⌘-click deletes the point.
                let curve = p.curve;
                svg!("circle", {
                    .attr("r", "5")
                    .attr("cx", &format!("{:.1}", mx(p.beat, tmax)))
                    .attr("cy", &format!("{:.1}", my(p.value as f64, vmin, vmax)))
                    .attr("fill", "oklch(0.86 0.15 90)")
                    .attr("stroke", "oklch(0.155 0.006 255)")
                    .attr("stroke-width", "2")
                    .attr("style", "cursor:pointer;")
                    .event(move |e: events::PointerDown| {
                        e.stop_propagation();
                        let op = if e.alt_key() || e.ctrl_key() {
                            ControlOp::RemovePoint { lane: idx, index: i }
                        } else {
                            ControlOp::SetPointCurve { lane: idx, index: i, curve: next_curve(curve) }
                        };
                        controller().dispatch(EditorCommand::EditControl { node: id, op });
                    })
                })
            }))
        }))
        .child(html!("div", {
            .style("font-size", "10px")
            .style("color", "var(--text-2)")
            .style("margin-top", "-2px")
            .text("click bg: add · click point: cycle curve · alt/⌘-click: delete")
        }))
    })
}

/// The Control Sequencer inspector: tempo/start/loop, `.mid` CC import, and a
/// per-lane automation plot. Each lane is an output port wired to a parameter.
fn control_panel(node: Rc<EditorNode>) -> Dom {
    let id = node.id;
    let Some(cs) = controller().control_node(id) else {
        return html!("div", {});
    };
    let looping = cs.looping;
    html!("div", {
        .child(html!("div", {
            .style("font-weight", "700")
            .style("margin-bottom", "8px")
            .text("Control Sequencer")
        }))
        .child(rename_row(node.clone()))
        .child(cc_file_button(id))
        .child(labeled_row("tempo (BPM)", num_input(cs.bpm, move |v| controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::SetBpm(v) }))))
        .child(labeled_row("start (beats)", num_input(cs.start, move |v| controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::SetStart(v) }))))
        .child(labeled_row("loop", html!("input" => web_sys::HtmlInputElement, {
            .attr("type", "checkbox")
            .apply(move |b| if looping { b.attr("checked", "") } else { b })
            .with_node!(cb => {
                .event(clone!(cb => move |_: events::Change| controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::SetLooping(cb.checked()) })))
            })
        })))
        .child(html!("div", {
            .style("font-weight", "700")
            .style("margin", "10px 0 5px")
            .text("Lanes")
        }))
        .children(cs.lanes.iter().enumerate().map(move |(i, lane)| {
            html!("div", {
                .style("border", "1px solid var(--line)")
                .style("border-radius", "5px")
                .style("padding", "6px")
                .style("margin-bottom", "6px")
                .child(html!("div", {
                    .style("display", "flex")
                    .style("align-items", "center")
                    .style("justify-content", "space-between")
                    .style("margin-bottom", "5px")
                    .child(html!("span", {
                        .style("font-weight", "700")
                        .text(&format!("out {} \u{2192} param", i + 1))
                    }))
                    .child(html!("div", {
                        .style("display", "flex")
                        .style("gap", "8px")
                        .style("align-items", "center")
                        .child(html!("span", {
                            .style("cursor", "pointer")
                            .style("opacity", "0.6")
                            .style("font-size", "11px")
                            .text("clear")
                            .event(move |_: events::Click| controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::SetPoints { lane: i, points: Vec::new() } }))
                        }))
                        .child(html!("span", {
                            .style("cursor", "pointer")
                            .style("opacity", "0.6")
                            .style("padding", "0 4px")
                            .text("\u{00d7}")
                            .event(move |_: events::Click| controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::RemoveLane { index: i } }))
                        }))
                    }))
                }))
                .child(labeled_row("name", html!("input" => web_sys::HtmlInputElement, {
                    .apply(small_input)
                    .style("max-width", "150px")
                    .attr("value", &lane.label)
                    .with_node!(input => {
                        .event(clone!(input => move |_: events::Change| {
                            controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::SetLaneLabel { index: i, label: input.value() } });
                        }))
                    })
                })))
                .child(lane_plot(id, i, &lane.points))
            })
        }).collect::<Vec<_>>())
        .child(html!("button", {
            .style("width", "100%")
            .style("margin-top", "2px")
            .style("padding", "4px")
            .style("cursor", "pointer")
            .style("background", "var(--bg-2)")
            .style("color", "inherit")
            .style("border", "1px solid var(--line-strong)")
            .style("border-radius", "4px")
            .style("font-size", "11.5px")
            .text("+ Add lane")
            .event(move |_: events::Click| controller().dispatch(EditorCommand::EditControl { node: id, op: ControlOp::AddLane }))
        }))
        .child(html!("div", {
            .style("opacity", "0.45")
            .style("line-height", "1.5")
            .style("margin-top", "8px")
            .text("Each lane is an output port. Drag from it onto a node's parameter dot to automate that parameter over time; click the plot to add points, a point to delete it.")
        }))
    })
}

fn panel(node: Rc<EditorNode>) -> Dom {
    use crate::controller::BoundaryPort;
    // Boundary ports: just edit the port name (the canvas header reflects it).
    if let Some(b) = node.boundary {
        let is_inlet = b == BoundaryPort::Inlet;
        return html!("div", {
            .child(html!("div", {
                .style("font-weight", "700")
                .style("margin-bottom", "8px")
                .text(if is_inlet { "Input" } else { "Output" })
            }))
            .child(rename_row(node.clone()))
            // Inlets are inputs: editable value + set/add mode + MIDI + targets.
            .apply(clone!(node => move |dom| if is_inlet {
                dom.child(input_row(node.clone()))
            } else {
                dom
            }))
            .child(html!("div", {
                .style("opacity", "0.45")
                .style("line-height", "1.5")
                .text(if is_inlet {
                    "The name is this input's handle in a parent patch. Wire it to a node's parameter: its value sets that param; a signal wired into it from outside modulates (adds)."
                } else {
                    "The name is this output's handle in a parent patch. Wire a node into it to route signal out."
                })
            }))
        });
    }
    // Sample reference: set per-instance values for the target sample's inputs.
    let sample_ref = if let awsm_audio_schema::NodeKind::Sample(sr) = &*node.kind.borrow() {
        Some(sr.sample)
    } else {
        None
    };
    if let Some(sample) = sample_ref {
        let inputs = controller().referenced_inputs(sample);
        return html!("div", {
            .child(html!("div", {
                .style("font-weight", "700")
                .style("margin-bottom", "8px")
                .text("Sample — inputs")
            }))
            .apply(clone!(node => move |b| if inputs.is_empty() {
                b.child(html!("div", {
                    .style("opacity", "0.45")
                    .style("line-height", "1.5")
                    .text("The referenced sample has no inputs. Open it, add an Input node, and wire it to a parameter to expose one.")
                }))
            } else {
                b.children(inputs.into_iter().map(clone!(node => move |(name, default)| input_value_row(node.clone(), name, default))))
            }))
        });
    }

    // The sequencer gets a fully custom panel (song + parts).
    if matches!(&*node.kind.borrow(), NodeKind::NoteSequencer(_)) {
        return midisong_panel(node);
    }
    if matches!(&*node.kind.borrow(), NodeKind::ControlSequencer(_)) {
        return control_panel(node);
    }

    let kind = node.kind.borrow();
    let title = ports::kind_label(&kind);
    let params = fields::audio_params(&kind);
    // Node-specific authoring/visualization extras (shown below the params).
    let harmonics = match &*kind {
        NodeKind::Oscillator(o)
            if o.oscillator_type == awsm_audio_schema::OscillatorType::Custom =>
        {
            Some(o.harmonics.clone())
        }
        _ => None,
    };
    let waveshaper = match &*kind {
        NodeKind::WaveShaper(w) => Some((w.shape, w.amount, w.curve.clone())),
        _ => None,
    };
    let iir = match &*kind {
        NodeKind::IirFilter(f) => Some((f.feedforward.clone(), f.feedback.clone())),
        _ => None,
    };
    let is_analyser = matches!(&*kind, NodeKind::Analyser(_));
    drop(kind);

    html!("div", {
        .child(html!("div", {
            .style("font-weight", "700")
            .style("margin-bottom", "8px")
            .text(&format!("{title} — envelopes"))
        }))
        // Rename: a label overriding the header's type name (empty = type name).
        .child(rename_row(node.clone()))
        .apply(|b| if params.is_empty() {
            b.child(html!("div", { .style("opacity", "0.45").text("No automatable parameters.") }))
        } else {
            b
        })
        .children(params.into_iter().map(clone!(node => move |p| param_section(node.clone(), p))))
        // Custom oscillator: a drawable harmonics (partial-amplitude) editor.
        .apply(clone!(node => move |b| match harmonics {
            Some(h) => b.child(harmonics_editor(node.clone(), h)),
            None => b,
        }))
        // WaveShaper: the transfer curve it applies — drawable when Custom, else
        // a read-only preview of the generated curve.
        .apply(clone!(node => move |b| match waveshaper {
            Some((WaveShaperShape::Custom, _, curve)) => b.child(curve_editor(node.clone(), curve)),
            Some((shape, amount, _)) => b.child(transfer_plot(shape, amount)),
            None => b,
        }))
        // IIR filter: a friendly designer + its magnitude frequency response.
        .apply(clone!(node => move |b| match iir {
            Some((ff, fb)) => b
                .child(iir_designer(node.clone()))
                .child(iir_response_plot(&ff, &fb)),
            None => b,
        }))
        // Analyser: a live oscilloscope of the signal passing through it.
        .apply(clone!(node => move |b| if is_analyser {
            b.child(analyser_scope(node.clone()))
        } else {
            b
        }))
    })
}

/// Compute a normalized biquad's IIR coefficients (RBJ cookbook) for a friendly
/// filter spec, assuming a 48 kHz context. Returns `(feedforward, feedback)`.
fn design_biquad(kind: &str, f0: f64, q: f64) -> (Vec<f64>, Vec<f64>) {
    let sr = 48_000.0;
    let w0 = 2.0 * std::f64::consts::PI * (f0.clamp(10.0, sr / 2.0)) / sr;
    let (cw, sw) = (w0.cos(), w0.sin());
    let alpha = sw / (2.0 * q.max(0.05));
    let (b0, b1, b2, a0, a1, a2) = match kind {
        "highpass" => {
            let b = (1.0 + cw) / 2.0;
            (b, -(1.0 + cw), b, 1.0 + alpha, -2.0 * cw, 1.0 - alpha)
        }
        "bandpass" => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
        "notch" => (1.0, -2.0 * cw, 1.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
        _ => {
            let b = 1.0 - cw;
            (b / 2.0, b, b / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha)
        }
    };
    (vec![b0 / a0, b1 / a0, b2 / a0], vec![1.0, a1 / a0, a2 / a0])
}

/// A friendly IIR designer: pick a response + cutoff + Q and "Apply" to compute
/// the feedforward/feedback coefficients (overwriting the raw lists above).
fn iir_designer(node: Rc<EditorNode>) -> Dom {
    use crate::fields::FieldValue;
    let ty = Mutable::new("lowpass".to_string());
    let freq = Mutable::new(1000.0_f64);
    let q = Mutable::new(0.707_f64);
    let apply = clone!(node, ty, freq, q => move || {
        let (ff, fb) = design_biquad(&ty.get_cloned(), freq.get(), q.get());
        let join = |v: &[f64]| v.iter().map(|x| format!("{x:.6}")).collect::<Vec<_>>().join(", ");
        controller().dispatch(EditorCommand::SetField {
            id: node.id, key: "feedforward".into(), value: FieldValue::Text(join(&ff)),
        });
        controller().dispatch(EditorCommand::SetField {
            id: node.id, key: "feedback".into(), value: FieldValue::Text(join(&fb)),
        });
    });
    html!("div", {
        .style("margin", "4px 0 6px")
        .style("padding-top", "5px")
        .style("border-top", "1px dashed var(--line)")
        .child(plot_caption("designer"))
        .child(labeled_row("response", html!("select" => web_sys::HtmlSelectElement, {
            .apply(small_input)
            .children(["lowpass","highpass","bandpass","notch"].iter().map(|o| html!("option", {
                .attr("value", o).text(o)
            })))
            .with_node!(sel => {
                .event(clone!(sel, ty => move |_: events::Change| ty.set(sel.value())))
            })
        })))
        .child(labeled_row("cutoff (Hz)", num_input(freq.get(), clone!(freq => move |v| freq.set(v.max(10.0))))))
        .child(labeled_row("Q", num_input(q.get(), clone!(q => move |v| q.set(v.max(0.05))))))
        .child(html!("button", {
            .style("width", "100%")
            .style("margin-top", "4px")
            .style("padding", "4px")
            .style("cursor", "pointer")
            .style("background", "var(--accent-dim)")
            .style("color", "inherit")
            .style("border", "1px solid var(--accent-dim)")
            .style("border-radius", "4px")
            .style("font-size", "11.5px")
            .text("Apply → coefficients")
            .event(move |_: events::Click| apply())
        }))
    })
}

/// A live oscilloscope for an Analyser node: an SVG trace polling the node's
/// time-domain data each animation frame while the graph plays.
fn analyser_scope(node: Rc<EditorNode>) -> Dom {
    let id = node.id;
    let path = Mutable::new(String::new());
    let raf: RafHandle = Rc::new(RefCell::new(None));
    html!("div", {
        .style("margin", "2px 0 6px")
        .child(plot_caption("scope (plays live)"))
        .child(svg!("svg", {
            .attr("style", &format!("width:{PW}px; height:{PH}px; background:var(--bg-0); border:1px solid var(--line); border-radius:6px;"))
            .child(svg!("path", {
                .attr("fill", "none")
                .attr("stroke", "oklch(0.82 0.13 150)")
                .attr("stroke-width", "1.5")
                .attr_signal("d", path.signal_cloned())
            }))
        }))
        // Drive an rAF loop that samples the node's analyser into the path.
        .after_inserted(clone!(path, raf => move |_| {
            fn tick(path: &Mutable<String>, raf: &RafHandle, id: NodeId) {
                let data = controller().analyser_scope(id);
                if !data.is_empty() {
                    let n = data.len();
                    let d: String = data.iter().enumerate().map(|(i, &b)| {
                        let x = i as f64 / (n - 1) as f64 * PW;
                        let y = (b as f64 / 255.0) * PH;
                        format!("{}{x:.1},{y:.1}", if i == 0 { "M" } else { "L" })
                    }).collect();
                    path.set(d);
                }
                let p2 = path.clone();
                let r2 = raf.clone();
                let cb = Closure::<dyn FnMut()>::new(move || tick(&p2, &r2, id));
                if let Some(w) = web_sys::window() {
                    if let Ok(h) = w.request_animation_frame(cb.as_ref().unchecked_ref()) {
                        *raf.borrow_mut() = Some((h, cb));
                    }
                }
            }
            tick(&path, &raf, id);
        }))
        .after_removed(clone!(raf => move |_| {
            if let Some((h, _)) = raf.borrow_mut().take() {
                if let Some(w) = web_sys::window() { w.cancel_animation_frame(h).ok(); }
            }
        }))
    })
}

fn param_section(node: Rc<EditorNode>, p: fields::ParamInfo) -> Dom {
    let key = p.key;
    let base = p.value;
    let events = p.automation.clone();
    html!("div", {
        .style("margin-bottom", "14px")
        .style("padding-bottom", "10px")
        .style("border-bottom", "1px solid var(--bg-2)")
        .child(html!("div", {
            .style("display", "flex")
            .style("justify-content", "space-between")
            .style("align-items", "baseline")
            .style("margin-bottom", "4px")
            .child(html!("span", { .style("font-weight", "600").text(p.label) }))
            .child(html!("span", {
                .style("opacity", "0.5")
                .style("font-size", "11.5px")
                .text(&format!("base {}", trim(base as f64)))
            }))
        }))
        // Graphical envelope plot (drag dots; click empty area to add a point).
        .child(envelope_plot(node.clone(), key, base, events.clone()))
        // Existing breakpoints (numeric, for precision).
        .children(events.iter().enumerate().map(clone!(node, events => move |(i, ev)| {
            event_row(node.clone(), key, events.clone(), i, ev.clone())
        })))
        // Add a breakpoint.
        .child(html!("button", {
            .style("margin-top", "4px")
            .style("padding", "2px 8px")
            .style("font-size", "11.5px")
            .style("border", "1px solid var(--line-strong)")
            .style("border-radius", "4px")
            .style("background", "var(--bg-2)")
            .style("color", "inherit")
            .style("cursor", "pointer")
            .text("+ breakpoint")
            .event(clone!(node, events => move |_: events::Click| {
                let mut next = events.clone();
                let t = next.len() as f64 * 0.2;
                next.push(AutomationEvent::SetValue { value: p.value, time: t });
                dispatch(&node, key, next);
            }))
        }))
    })
}

fn event_row(
    node: Rc<EditorNode>,
    key: &'static str,
    events: Vec<AutomationEvent>,
    i: usize,
    ev: AutomationEvent,
) -> Dom {
    let children: Vec<Dom> = match value_time(&ev) {
        Some((ty, value, time)) => vec![
            type_select(node.clone(), key, events.clone(), i, ty, value, time),
            num_input(
                value as f64,
                clone!(node, events => move |v| {
                    let mut next = events.clone();
                    next[i] = make_event(ty, v as f32, time);
                    dispatch(&node, key, next);
                }),
            ),
            num_input(
                time,
                clone!(node, events => move |v| {
                    let mut next = events.clone();
                    next[i] = make_event(ty, value, v);
                    dispatch(&node, key, next);
                }),
            ),
            remove_btn(node.clone(), key, events.clone(), i),
        ],
        None => vec![
            html!("span", {
                .style("flex", "1")
                .style("opacity", "0.5")
                .style("font-size", "11.5px")
                .text("(advanced)")
            }),
            remove_btn(node.clone(), key, events.clone(), i),
        ],
    };

    html!("div", {
        .style("display", "flex")
        .style("align-items", "center")
        .style("gap", "4px")
        .style("margin-top", "3px")
        .children(children)
    })
}

fn type_select(
    node: Rc<EditorNode>,
    key: &'static str,
    events: Vec<AutomationEvent>,
    i: usize,
    current: &'static str,
    value: f32,
    time: f64,
) -> Dom {
    html!("select" => web_sys::HtmlSelectElement, {
        .apply(small_input)
        .style("width", "62px")
        .children(EVENT_TYPES.iter().map(|t| {
            let t = *t;
            let selected = t == current;
            html!("option", {
                .attr("value", t)
                .apply(move |b| if selected { b.attr("selected", "") } else { b })
                .text(t)
            })
        }))
        .with_node!(sel => {
            .event(clone!(node, events, sel => move |_: events::Change| {
                let mut next = events.clone();
                next[i] = make_event(&sel.value(), value, time);
                dispatch(&node, key, next);
            }))
        })
    })
}

fn remove_btn(
    node: Rc<EditorNode>,
    key: &'static str,
    events: Vec<AutomationEvent>,
    i: usize,
) -> Dom {
    html!("button", {
        // `border: none` is a shorthand dominator's validator rejects in some
        // browsers; set it unchecked.
        .style_unchecked("border", "none")
        .style("background", "transparent")
        .style("color", "var(--text-2)")
        .style("cursor", "pointer")
        .style("font-size", "13px")
        .text("×")
        .event(clone!(node, events => move |_: events::Click| {
            let mut next = events.clone();
            next.remove(i);
            dispatch(&node, key, next);
        }))
    })
}

fn num_input(value: f64, on_change: impl Fn(f64) + 'static) -> Dom {
    html!("input" => web_sys::HtmlInputElement, {
        .attr("type", "number")
        .attr("step", "any")
        .attr("value", &trim(value))
        .apply(small_input)
        .style("width", "56px")
        .with_node!(input => {
            .event(clone!(input => move |_: events::Change| {
                if let Ok(v) = input.value().parse::<f64>() {
                    on_change(v);
                }
            }))
        })
    })
}

fn small_input<A>(b: dominator::DomBuilder<A>) -> dominator::DomBuilder<A>
where
    A: AsRef<web_sys::HtmlElement>,
{
    b.style("box-sizing", "border-box")
        .style("background", "var(--bg-2)")
        .style("color", "inherit")
        .style("border", "1px solid var(--line-strong)")
        .style("border-radius", "4px")
        .style("padding", "2px 4px")
        .style("font-size", "11.5px")
}

fn dispatch(node: &Rc<EditorNode>, key: &'static str, events: Vec<AutomationEvent>) {
    // The dispatched SetAutomation bumps inspector_rev → this panel re-renders.
    controller().dispatch(EditorCommand::SetAutomation {
        id: node.id,
        param: key.to_string(),
        events,
    });
}

/// Extract `(type, value, time)` for the editable value+time event kinds.
fn value_time(ev: &AutomationEvent) -> Option<(&'static str, f32, f64)> {
    match ev {
        AutomationEvent::SetValue { value, time } => Some(("set", *value, *time)),
        AutomationEvent::LinearRamp { value, time } => Some(("linear", *value, *time)),
        AutomationEvent::ExponentialRamp { value, time } => Some(("exp", *value, *time)),
        _ => None,
    }
}

fn make_event(ty: &str, value: f32, time: f64) -> AutomationEvent {
    match ty {
        "linear" => AutomationEvent::LinearRamp { value, time },
        "exp" => AutomationEvent::ExponentialRamp { value, time },
        _ => AutomationEvent::SetValue { value, time },
    }
}

/// Compact float formatting for display.
fn trim(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

// ======================================================================
// Graphical envelope plot.
// ======================================================================

/// Build an SVG path through control points, drawing each segment in the shape
/// of the target point's [`Curve`] so the plot mirrors playback.
fn curve_path(
    sorted: &[&awsm_audio_schema::ControlPoint],
    tmax: f64,
    vmin: f64,
    vmax: f64,
) -> String {
    use awsm_audio_schema::Curve;
    let mut out = String::new();
    for (i, p) in sorted.iter().enumerate() {
        let (x, y) = (mx(p.beat, tmax), my(p.value as f64, vmin, vmax));
        if i == 0 {
            out.push_str(&format!("M {x:.1} {y:.1}"));
            continue;
        }
        let prev = sorted[i - 1];
        let (pv, cv) = (prev.value as f64, p.value as f64);
        match p.curve {
            // Hold the previous value to this point's time, then jump.
            Curve::Step => out.push_str(&format!(
                " L {:.1} {:.1} L {x:.1} {y:.1}",
                x,
                my(pv, vmin, vmax)
            )),
            Curve::Linear => out.push_str(&format!(" L {x:.1} {y:.1}")),
            // Sample the eased value into short line segments.
            Curve::Exponential | Curve::Smooth => {
                const N: usize = 16;
                for k in 1..=N {
                    let f = k as f64 / N as f64;
                    let v = if matches!(p.curve, Curve::Smooth) {
                        pv + (cv - pv) * (f * f * (3.0 - 2.0 * f))
                    } else if pv.signum() == cv.signum() && pv.abs() > 1e-9 {
                        pv * (cv / pv).powf(f)
                    } else {
                        pv + (cv - pv) * f
                    };
                    out.push_str(&format!(
                        " L {:.1} {:.1}",
                        mx(prev.beat + (p.beat - prev.beat) * f, tmax),
                        my(v, vmin, vmax)
                    ));
                }
            }
        }
    }
    out
}

/// Cycle a point's curve shape: step → linear → exponential → smooth → step.
fn next_curve(c: awsm_audio_schema::Curve) -> awsm_audio_schema::Curve {
    use awsm_audio_schema::Curve::*;
    match c {
        Step => Linear,
        Linear => Exponential,
        Exponential => Smooth,
        Smooth => Step,
    }
}

fn mx(t: f64, tmax: f64) -> f64 {
    PAD + (t / tmax) * (PW - 2.0 * PAD)
}
fn my(v: f64, vmin: f64, vmax: f64) -> f64 {
    PH - PAD - ((v - vmin) / (vmax - vmin)) * (PH - 2.0 * PAD)
}

/// Pixel position (client) → (time, value), clamped to the plot's domain.
fn unmap(
    el: &web_sys::HtmlElement,
    client_x: f64,
    client_y: f64,
    tmax: f64,
    vmin: f64,
    vmax: f64,
) -> Option<(f64, f32)> {
    let r = el.get_bounding_client_rect();
    if r.width() < 1.0 {
        return None;
    }
    let lx = client_x - r.left();
    let ly = client_y - r.top();
    let t = (((lx - PAD) / (PW - 2.0 * PAD)) * tmax).clamp(0.0, tmax);
    let v = vmin + (1.0 - (ly - PAD) / (PH - 2.0 * PAD)) * (vmax - vmin);
    Some((t, v as f32))
}

/// Auto-scale the plot to the envelope: `(t_max, v_min, v_max)` with padding.
fn scaling(events: &[AutomationEvent], base: f32) -> (f64, f64, f64) {
    let mut tmax = 0.0f64;
    let mut vmin = base as f64;
    let mut vmax = base as f64;
    for e in events {
        tmax = tmax.max(fields::event_time(e));
        if let Some(v) = fields::event_value(e) {
            vmin = vmin.min(v as f64);
            vmax = vmax.max(v as f64);
        }
    }
    if tmax < 1e-6 {
        tmax = 1.0;
    } else {
        tmax *= 1.12;
    }
    if (vmax - vmin).abs() < 1e-6 {
        let pad = vmax.abs() * 0.5 + 1.0;
        vmin -= pad;
        vmax += pad;
    } else {
        let pad = (vmax - vmin) * 0.1;
        vmin -= pad;
        vmax += pad;
    }
    (tmax, vmin, vmax)
}

/// Live events for this param: the in-flight drag preview if it targets this
/// param, otherwise the committed automation.
fn pick<'a>(
    drag: &'a Option<EnvDrag>,
    node_id: NodeId,
    key: &str,
    static_events: &'a [AutomationEvent],
) -> &'a [AutomationEvent] {
    match drag {
        Some(d) if d.node == node_id && d.key == key => &d.events,
        _ => static_events,
    }
}

/// SVG path of the envelope: starts at `base`, then a segment per event (step
/// for `set`, line for `linear`, sampled curve for `exp`).
fn path_d(events: &[AutomationEvent], base: f32, tmax: f64, vmin: f64, vmax: f64) -> String {
    let mut evs: Vec<&AutomationEvent> = events.iter().collect();
    evs.sort_by(|a, b| {
        fields::event_time(a)
            .partial_cmp(&fields::event_time(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // No automation: a flat line at the base value reads as "constant", not blank.
    if evs.is_empty() {
        let y = my(base as f64, vmin, vmax);
        return format!(
            "M {:.2} {:.2} L {:.2} {:.2}",
            mx(0.0, tmax),
            y,
            mx(tmax, tmax),
            y
        );
    }
    let mut t_prev = 0.0f64;
    let mut v_prev = base as f64;
    let mut d = format!("M {:.2} {:.2}", mx(0.0, tmax), my(v_prev, vmin, vmax));
    for e in evs {
        let t = fields::event_time(e);
        match e {
            AutomationEvent::SetValue { value, .. } => {
                let v = *value as f64;
                d += &format!(" L {:.2} {:.2}", mx(t, tmax), my(v_prev, vmin, vmax));
                d += &format!(" L {:.2} {:.2}", mx(t, tmax), my(v, vmin, vmax));
                v_prev = v;
            }
            AutomationEvent::LinearRamp { value, .. } => {
                let v = *value as f64;
                d += &format!(" L {:.2} {:.2}", mx(t, tmax), my(v, vmin, vmax));
                v_prev = v;
            }
            AutomationEvent::ExponentialRamp { value, .. } => {
                let v = *value as f64;
                if v_prev > 1e-9 && v > 1e-9 && t > t_prev {
                    let n = 14;
                    for k in 1..=n {
                        let f = k as f64 / n as f64;
                        let tt = t_prev + (t - t_prev) * f;
                        let vv = v_prev * (v / v_prev).powf(f);
                        d += &format!(" L {:.2} {:.2}", mx(tt, tmax), my(vv, vmin, vmax));
                    }
                } else {
                    d += &format!(" L {:.2} {:.2}", mx(t, tmax), my(v, vmin, vmax));
                }
                v_prev = v;
            }
            other => {
                if let Some(v) = fields::event_value(other) {
                    let v = v as f64;
                    d += &format!(" L {:.2} {:.2}", mx(t, tmax), my(v, vmin, vmax));
                    v_prev = v;
                }
            }
        }
        t_prev = t;
    }
    d
}

/// A small caption above a plot/editor.
fn plot_caption(text: &str) -> Dom {
    html!("div", {
        .style("font-size", "11px")
        .style("opacity", "0.55")
        .style("margin", "6px 0 2px")
        .text(text)
    })
}

/// A drawable harmonics editor: a row of bars (partial amplitudes 0..1). Drag
/// across them to paint the spectrum; commits on release as the node's
/// `harmonics` list. The custom oscillator builds its PeriodicWave from these.
fn harmonics_editor(node: Rc<EditorNode>, initial: Vec<f32>) -> Dom {
    use crate::fields::FieldValue;
    const N: usize = 12;
    let mut start = initial;
    start.resize(N, 0.0);
    let live = Mutable::new(start);
    let drawing = Rc::new(RefCell::new(false));
    let host: Rc<RefCell<Option<web_sys::HtmlElement>>> = Rc::new(RefCell::new(None));
    let bw = PW / N as f64;

    let paint = clone!(live, host => move |cx: f64, cy: f64| {
        if let Some(el) = host.borrow().as_ref() {
            let r = el.get_bounding_client_rect();
            let i = (((cx - r.left()) / PW) * N as f64).floor().clamp(0.0, (N - 1) as f64) as usize;
            let amp = (1.0 - (cy - r.top()) / PH).clamp(0.0, 1.0) as f32;
            live.lock_mut()[i] = amp;
        }
    });
    let commit = clone!(live, node => move || {
        let s = live
            .lock_ref()
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        controller().dispatch(EditorCommand::SetField {
            id: node.id,
            key: "harmonics".to_string(),
            value: FieldValue::Text(s),
        });
    });

    html!("div", {
        .child(plot_caption("harmonics (drag to draw)"))
        .child(html!("div" => web_sys::HtmlElement, {
            .style("position", "relative")
            .style("width", &format!("{PW}px"))
            .style("height", &format!("{PH}px"))
            .style("touch-action", "none")
            .style("background", "var(--bg-0)")
            .style("border", "1px solid var(--line)")
            .style("border-radius", "6px")
            .style("box-sizing", "border-box")
            .style("cursor", "crosshair")
            .after_inserted(clone!(host => move |el| { *host.borrow_mut() = Some(el); }))
            .event(clone!(drawing, paint => move |e: events::PointerDown| {
                *drawing.borrow_mut() = true;
                paint(e.x(), e.y());
            }))
            .event(clone!(drawing, paint => move |e: events::PointerMove| {
                if *drawing.borrow() { paint(e.x(), e.y()); }
            }))
            .event(clone!(drawing, commit => move |_: events::PointerUp| {
                if std::mem::replace(&mut drawing.borrow_mut(), false) { commit(); }
            }))
            .event(clone!(drawing, commit => move |_: events::PointerLeave| {
                if std::mem::replace(&mut drawing.borrow_mut(), false) { commit(); }
            }))
            .child(svg!("svg", {
                .attr("style", &format!("position:absolute; left:0; top:0; width:{PW}px; height:{PH}px;"))
                .children((0..N).map(clone!(live => move |i| {
                    let x = i as f64 * bw + 1.0;
                    svg!("rect", {
                        .attr("x", &format!("{x}"))
                        .attr("width", &format!("{}", bw - 2.0))
                        .attr("rx", "1")
                        .attr("fill", if i == 0 { "oklch(0.78 0.14 150)" } else { "#5b8dd6" })
                        .attr("pointer-events", "none")
                        .attr_signal("y", live.signal_ref(move |v| {
                            format!("{}", PH * (1.0 - v[i] as f64))
                        }))
                        .attr_signal("height", live.signal_ref(move |v| {
                            format!("{}", PH * v[i] as f64)
                        }))
                    })
                })))
            }))
        }))
    })
}

/// A drawable WaveShaper transfer-curve editor: drag across the plot to paint
/// the output (vertical, -1..1) for each input sample (horizontal, -1..1).
/// Commits on release as the node's `curve` (a custom shape). Mirrors the
/// harmonics editor; the player resamples the points to the WebAudio table.
fn curve_editor(node: Rc<EditorNode>, initial: Vec<f32>) -> Dom {
    use crate::fields::FieldValue;
    const N: usize = 33; // odd → a sample sits exactly at input 0
    let mut start = initial;
    if start.len() != N {
        // Seed an identity ramp (-1..1) if unset, else resample the stored curve.
        start = (0..N)
            .map(|i| {
                let x = i as f32 / (N - 1) as f32 * 2.0 - 1.0;
                if start.is_empty() {
                    x
                } else {
                    let pos = i as f32 / (N - 1) as f32 * (start.len() - 1) as f32;
                    let lo = pos.floor() as usize;
                    let hi = (lo + 1).min(start.len() - 1);
                    start[lo] + (start[hi] - start[lo]) * (pos - lo as f32)
                }
            })
            .collect();
    }
    let live = Mutable::new(start);
    let drawing = Rc::new(RefCell::new(false));
    let host: Rc<RefCell<Option<web_sys::HtmlElement>>> = Rc::new(RefCell::new(None));

    let paint = clone!(live, host => move |cx: f64, cy: f64| {
        if let Some(el) = host.borrow().as_ref() {
            let r = el.get_bounding_client_rect();
            let i = (((cx - r.left()) / PW) * N as f64).floor().clamp(0.0, (N - 1) as f64) as usize;
            // y: top = +1, bottom = -1.
            let v = (1.0 - 2.0 * (cy - r.top()) / PH).clamp(-1.0, 1.0) as f32;
            live.lock_mut()[i] = v;
        }
    });
    let commit = clone!(live, node => move || {
        let s = live.lock_ref().iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
        controller().dispatch(EditorCommand::SetField {
            id: node.id,
            key: "curve".to_string(),
            value: FieldValue::Text(s),
        });
    });

    html!("div", {
        .child(plot_caption("transfer curve (drag to draw)"))
        .child(html!("div" => web_sys::HtmlElement, {
            .style("position", "relative")
            .style("width", &format!("{PW}px"))
            .style("height", &format!("{PH}px"))
            .style("touch-action", "none")
            .style("background", "var(--bg-0)")
            .style("border", "1px solid var(--line)")
            .style("border-radius", "6px")
            .style("box-sizing", "border-box")
            .style("cursor", "crosshair")
            .after_inserted(clone!(host => move |el| { *host.borrow_mut() = Some(el); }))
            .event(clone!(drawing, paint => move |e: events::PointerDown| {
                *drawing.borrow_mut() = true;
                paint(e.x(), e.y());
            }))
            .event(clone!(drawing, paint => move |e: events::PointerMove| {
                if *drawing.borrow() { paint(e.x(), e.y()); }
            }))
            .event(clone!(drawing, commit => move |_: events::PointerUp| {
                if std::mem::replace(&mut drawing.borrow_mut(), false) { commit(); }
            }))
            .event(clone!(drawing, commit => move |_: events::PointerLeave| {
                if std::mem::replace(&mut drawing.borrow_mut(), false) { commit(); }
            }))
            .child(svg!("svg", {
                .attr("style", &format!("position:absolute; left:0; top:0; width:{PW}px; height:{PH}px;"))
                // Zero line (output 0).
                .child(svg!("line", {
                    .attr("x1", "0").attr("x2", &format!("{PW}"))
                    .attr("y1", &format!("{}", PH/2.0)).attr("y2", &format!("{}", PH/2.0))
                    .attr("stroke", "oklch(0.315 0.008 255)").attr("pointer-events", "none")
                }))
                .child(svg!("path", {
                    .attr("fill", "none")
                    .attr("stroke", "oklch(0.8 0.15 30)")
                    .attr("stroke-width", "2")
                    .attr("pointer-events", "none")
                    .attr_signal("d", live.signal_ref(|v| {
                        v.iter().enumerate().map(|(i, val)| {
                            let x = i as f64 / (N - 1) as f64 * PW;
                            let y = (1.0 - (*val as f64 + 1.0) / 2.0) * PH;
                            format!("{}{x:.1},{y:.1}", if i == 0 { "M" } else { "L" })
                        }).collect::<String>()
                    }))
                }))
            }))
        }))
    })
}

/// Evaluate a WaveShaper's transfer curve at input `x` ∈ [-1, 1] (mirrors the
/// player's `distortion_curve`), for the read-only preview plot.
fn shape_at(shape: WaveShaperShape, amount: f32, x: f32) -> f32 {
    let drive = 1.0 + amount.max(0.0);
    match shape {
        WaveShaperShape::Tanh => (drive * x).tanh() / drive.tanh(),
        WaveShaperShape::HardClip => (drive * x).clamp(-1.0, 1.0),
        WaveShaperShape::Fold => (drive * x * std::f32::consts::FRAC_PI_2).sin(),
        // Custom uses the drawable editor, not this preview; show identity.
        WaveShaperShape::Custom => x,
    }
}

/// A read-only plot of a WaveShaper's transfer curve (input → output).
fn transfer_plot(shape: WaveShaperShape, amount: f32) -> Dom {
    const K: usize = 64;
    let pts: String = (0..=K)
        .map(|i| {
            let x = (i as f64 / K as f64) * 2.0 - 1.0;
            let y = shape_at(shape, amount, x as f32) as f64;
            let px = (x + 1.0) / 2.0 * PW;
            let py = (1.0 - (y + 1.0) / 2.0) * PH;
            format!("{}{px:.1},{py:.1}", if i == 0 { "M" } else { "L" })
        })
        .collect();
    static_plot("transfer curve", pts)
}

/// A read-only plot of an IIR filter's magnitude response (dB) over log
/// frequency, computed from its feedforward/feedback coefficients.
fn iir_response_plot(ff: &[f64], fb: &[f64]) -> Dom {
    const K: usize = 96;
    // |H(e^jw)| = |Σ b_k e^{-jwk}| / |Σ a_k e^{-jwk}|.
    let mag = |coeffs: &[f64], w: f64| -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (k, c) in coeffs.iter().enumerate() {
            re += c * (k as f64 * w).cos();
            im -= c * (k as f64 * w).sin();
        }
        (re * re + im * im).sqrt()
    };
    let pts: String = (0..=K)
        .map(|i| {
            // Log frequency sweep ~20Hz..20kHz mapped to w ∈ (0, π].
            let frac = i as f64 / K as f64;
            let w = std::f64::consts::PI * 10f64.powf((frac - 1.0) * 3.0);
            let denom = mag(fb, w).max(1e-9);
            let h = mag(ff, w) / denom;
            let db = 20.0 * (h.max(1e-9)).log10();
            let px = frac * PW;
            // Map [-36, +24] dB → [PH, 0].
            let py = ((24.0 - db) / 60.0 * PH).clamp(0.0, PH);
            format!("{}{px:.1},{py:.1}", if i == 0 { "M" } else { "L" })
        })
        .collect();
    static_plot("frequency response (dB)", pts)
}

/// A read-only SVG line plot in a bordered box, with a center reference line.
fn static_plot(caption: &str, path: String) -> Dom {
    html!("div", {
        .child(plot_caption(caption))
        .child(svg!("svg", {
            .attr("style", &format!("width:{PW}px; height:{PH}px; display:block;"))
            .child(svg!("rect", {
                .attr("x", "0.5")
                .attr("y", "0.5")
                .attr("width", &format!("{}", PW - 1.0))
                .attr("height", &format!("{}", PH - 1.0))
                .attr("rx", "6")
                .attr("fill", "oklch(0.155 0.006 255)")
                .attr("stroke", "oklch(0.315 0.008 255)")
            }))
            .child(svg!("line", {
                .attr("x1", "0").attr("y1", &format!("{}", PH / 2.0))
                .attr("x2", &format!("{PW}")).attr("y2", &format!("{}", PH / 2.0))
                .attr("stroke", "oklch(0.228 0.007 255)")
                .attr("stroke-dasharray", "3 3")
            }))
            .child(svg!("path", {
                .attr("d", &path)
                .attr("fill", "none")
                .attr("stroke", "#7fa6df")
                .attr("stroke-width", "1.5")
            }))
        }))
    })
}

fn envelope_plot(
    node: Rc<EditorNode>,
    key: &'static str,
    base: f32,
    events: Vec<AutomationEvent>,
) -> Dom {
    let (tmax, vmin, vmax) = scaling(&events, base);
    let node_id = node.id;
    let host: Rc<RefCell<Option<web_sys::HtmlElement>>> = Rc::new(RefCell::new(None));
    let dot_idx: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| value_time(e).is_some())
        .map(|(i, _)| i)
        .collect();

    html!("div" => web_sys::HtmlElement, {
        .style("position", "relative")
        .style("width", &format!("{PW}px"))
        .style("height", &format!("{PH}px"))
        .style("margin", "2px 0 6px")
        .style("touch-action", "none")
        .after_inserted(clone!(host => move |el| { *host.borrow_mut() = Some(el); }))
        // Pointer move/up over the plot drives the in-flight breakpoint drag.
        .event(clone!(host => move |e: events::PointerMove| {
            if let Some(el) = host.borrow().as_ref() {
                if let Some((t, v)) = unmap(el, e.x(), e.y(), tmax, vmin, vmax) {
                    controller().update_env_drag(v, t);
                }
            }
        }))
        .event(|_: events::PointerUp| controller().commit_env_drag())
        .child(svg!("svg", {
            .attr("style", &format!("position:absolute; left:0; top:0; width:{PW}px; height:{PH}px;"))
            // Background — click empty space to add a breakpoint there.
            .child(svg!("rect", {
                .attr("x", "0.5")
                .attr("y", "0.5")
                .attr("width", &format!("{}", PW - 1.0))
                .attr("height", &format!("{}", PH - 1.0))
                .attr("rx", "6")
                .attr("fill", "oklch(0.155 0.006 255)")
                .attr("stroke", "oklch(0.315 0.008 255)")
                .attr("style", "cursor:crosshair;")
                .event(clone!(node, host, events => move |e: events::PointerDown| {
                    if let Some(el) = host.borrow().as_ref() {
                        if let Some((t, v)) = unmap(el, e.x(), e.y(), tmax, vmin, vmax) {
                            let mut next = events.clone();
                            next.push(AutomationEvent::SetValue { value: v, time: t });
                            controller().dispatch(EditorCommand::SetAutomation {
                                id: node.id,
                                param: key.to_string(),
                                events: next,
                            });
                        }
                    }
                }))
            }))
            // The envelope curve (reactive to the live drag).
            .child(svg!("path", {
                .attr("fill", "none")
                .attr("stroke", "oklch(0.82 0.13 150)")
                .attr("stroke-width", "2")
                .attr("style", "pointer-events:none;")
                .attr_signal("d", controller().env_drag.signal_cloned().map(clone!(events => move |drag| {
                    path_d(pick(&drag, node_id, key, &events), base, tmax, vmin, vmax)
                })))
            }))
            // Draggable breakpoint dots.
            .children(dot_idx.into_iter().map(clone!(node, events => move |i| {
                dot(node.clone(), key, i, events.clone(), tmax, vmin, vmax)
            })))
        }))
    })
}

fn dot(
    node: Rc<EditorNode>,
    key: &'static str,
    i: usize,
    events: Vec<AutomationEvent>,
    tmax: f64,
    vmin: f64,
    vmax: f64,
) -> Dom {
    let node_id = node.id;
    svg!("circle", {
        .attr("r", "5")
        .attr("fill", "oklch(0.86 0.14 150)")
        .attr("stroke", "oklch(0.155 0.006 255)")
        .attr("stroke-width", "2")
        .attr("style", "cursor:grab;")
        .attr_signal("cx", controller().env_drag.signal_cloned().map(clone!(events => move |drag| {
            let evs = pick(&drag, node_id, key, &events);
            let t = evs.get(i).map(fields::event_time).unwrap_or(0.0);
            format!("{:.2}", mx(t, tmax))
        })))
        .attr_signal("cy", controller().env_drag.signal_cloned().map(clone!(events => move |drag| {
            let evs = pick(&drag, node_id, key, &events);
            let v = evs.get(i).and_then(fields::event_value).unwrap_or(0.0) as f64;
            format!("{:.2}", my(v, vmin, vmax))
        })))
        .event(clone!(node => move |e: events::PointerDown| {
            e.stop_propagation();
            controller().begin_env_drag(node.id, key, i);
        }))
    })
}
