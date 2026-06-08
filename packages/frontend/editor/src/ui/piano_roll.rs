//! The piano-roll overlay: author/edit a song track's notes. Mounted once;
//! visible whenever the controller's `piano_roll` holds a `(node, track)`.
//!
//! Interaction (kept deliberately simple): drag on empty grid to draw a note
//! (horizontal drag sets its length); click an existing note to delete it. Time
//! runs left→right in beats (snapped to 1/4), pitch runs bottom→top in
//! semitones.

use std::cell::RefCell;
use std::rc::Rc;

use dominator::{clone, events, html, svg, Dom, EventOptions};
use futures_signals::map_ref;
use futures_signals::signal::SignalExt;
use wasm_bindgen::JsCast;

use crate::controller::{controller, EditorCommand, SongOp};
use awsm_audio_schema::{NodeId, NoteEvent};

/// An in-progress piano-roll drag.
#[derive(Clone)]
enum DragState {
    /// Drawing a new note from `start` at `pitch` (drag right sets length).
    New { start: f64, pitch: u8 },
    /// Moving an existing note: `offset` keeps the grab point under the cursor.
    Move {
        idx: usize,
        len: f64,
        vel: u8,
        offset: f64,
        orig_start: f64,
        orig_pitch: u8,
    },
    /// Resizing an existing note's length by dragging its right edge.
    Resize {
        idx: usize,
        start: f64,
        pitch: u8,
        vel: u8,
    },
    /// Dragging the playback-window start (`is_end` false) or stop marker.
    Marker { is_end: bool },
}

/// Note fill colour: a green whose lightness tracks velocity (louder = brighter).
fn note_fill(vel: u8) -> String {
    let l = 0.42 + 0.42 * (vel.min(127) as f64 / 127.0);
    format!("oklch({l:.2} 0.15 150)")
}

const NOTE_MIN: u8 = 36; // C2
const NOTE_MAX: u8 = 84; // C6
const ROW_H: f64 = 12.0;
const BEAT_W: f64 = 36.0;
const SNAP: f64 = 0.25;
/// Width of the sticky pitch/drum-name label column on the left of the grid.
const GUTTER_W: f64 = 66.0;

/// The sticky left label column: one label per row — drum-sound names on a drum
/// track, octave `C` markers on a melodic one. Pinned (`position:sticky`) so it
/// scrolls vertically with the grid but stays put horizontally; pointer-through
/// so it never intercepts note edits.
fn row_gutter(is_drum: bool, height: f64) -> Dom {
    html!("div", {
        .style("position", "sticky")
        .style("left", "0")
        .style("flex", "0 0 auto")
        .style("width", &format!("{GUTTER_W}px"))
        .style("height", &format!("{height}px"))
        .style("background", "var(--bg-1)")
        .style("border-right", "1px solid var(--line)")
        .style("z-index", "2")
        .style("pointer-events", "none")
        .children((0..rows()).filter_map(move |r| {
            let pitch = NOTE_MAX - r as u8;
            let label = if is_drum {
                awsm_audio_schema::gm_drum_name(pitch)?.to_string()
            } else if pitch % 12 == 0 {
                format!("C{}", pitch as i32 / 12 - 1)
            } else {
                return None;
            };
            Some(html!("div", {
                .style("position", "absolute")
                .style("left", "4px")
                .style("top", &format!("{}px", r as f64 * ROW_H))
                .style("height", &format!("{ROW_H}px"))
                .style("line-height", &format!("{ROW_H}px"))
                .style("width", &format!("{}px", GUTTER_W - 8.0))
                .style("font-size", "8px")
                .style("color", "var(--text-2)")
                .style("white-space", "nowrap")
                .style("overflow", "hidden")
                .style("text-overflow", "ellipsis")
                .text(&label)
            }))
        }))
    })
}

fn rows() -> usize {
    (NOTE_MAX - NOTE_MIN + 1) as usize
}

fn snap(beat: f64) -> f64 {
    (beat / SNAP).floor() * SNAP
}

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .child_signal(map_ref! {
            let pr = ctrl.piano_roll.signal(),
            let _rev = ctrl.inspector_rev.signal() =>
            pr.map(|(node, track)| view(node, track))
        })
    })
}

fn view(node: NodeId, track: usize) -> Dom {
    let Some(ms) = controller().song_node(node) else {
        return html!("div", {});
    };
    let Some(tr) = ms.song.tracks.get(track) else {
        return html!("div", {});
    };
    let title = if tr.name.is_empty() {
        format!("track {}", track + 1)
    } else {
        tr.name.clone()
    };
    // Grid length: honor the authored length if set (never clipping notes), else
    // auto-fit the content plus a bar of room. "+ bar"/"− bar" edit the length.
    let content_beats = tr.duration_beats().ceil();
    let total_beats = if ms.length > 0.0 {
        ms.length.max(content_beats)
    } else {
        (content_beats + 4.0).max(16.0)
    };
    let width = total_beats * BEAT_W;
    // Playback-window markers (beats): start, and stop (defaults to grid end).
    let win_start = ms.start;
    let win_end = ms.end.unwrap_or(total_beats);
    let height = rows() as f64 * ROW_H;
    let events = tr.events.clone();
    // The sequencer's mode is node-level (Melodic vs Drum); every track shares it,
    // so the roll keys off the node, not the individual track.
    let is_drum = ms.mode.is_drum();
    // Row to centre the initial scroll on: the middle of the track's pitch range
    // (so drum hits at the bottom aren't hidden off-screen on open).
    let focus_row = if events.is_empty() {
        NOTE_MAX as f64 - 60.0 // ~middle C
    } else {
        let (lo, hi) = events
            .iter()
            .fold((127u8, 0u8), |(a, b), e| (a.min(e.note), b.max(e.note)));
        NOTE_MAX as f64 - (lo as f64 + hi as f64) / 2.0
    };

    // Drag state + live preview rect.
    let host: Rc<RefCell<Option<web_sys::Element>>> = Rc::new(RefCell::new(None));
    let drag: Rc<RefCell<Option<DragState>>> = Rc::new(RefCell::new(None));
    let preview = futures_signals::signal::Mutable::new(None::<(f64, f64, u8)>);
    // Live marker positions while dragging (start beat, stop beat); seeded from
    // the node so the lines/shading render correctly before any drag.
    let marker = futures_signals::signal::Mutable::new((win_start, win_end));

    let cell = clone!(host => move |cx: f64, cy: f64| -> Option<(f64, u8)> {
        let h = host.borrow();
        let el = h.as_ref()?;
        let r = el.get_bounding_client_rect();
        let beat = ((cx - r.left()) / BEAT_W).max(0.0);
        let row = ((cy - r.top()) / ROW_H).floor();
        let pitch = (NOTE_MAX as f64 - row).clamp(NOTE_MIN as f64, NOTE_MAX as f64) as u8;
        Some((beat, pitch))
    });

    html!("div", {
        .style("position", "fixed")
        .style("inset", "0")
        .style("z-index", "1100")
        // Backdrop (click to close); a sibling so panel clicks don't bubble in.
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .style("background", "oklch(0 0 0 / 0.6)")
            .event(|_: events::Click| controller().close_piano_roll())
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
                .style("max-width", "92vw")
                .style("max-height", "86vh")
                .style("display", "flex")
                .style("flex-direction", "column")
                .style("background", "var(--bg-2)")
                .style("border", "1px solid var(--text-3)")
                .style("border-radius", "10px")
                .style("box-shadow", "0 24px 70px oklch(0 0 0 / 0.6)")
                .style("padding", "12px")
                // Header.
                .child(html!("div", {
                    .style("display", "flex")
                    .style("align-items", "center")
                    .style("justify-content", "space-between")
                    .style("margin-bottom", "8px")
                    .child(html!("div", {
                        .style("display", "flex")
                        .style("align-items", "center")
                        .style("gap", "9px")
                        .child(html!("div", {
                            .style("font-weight", "700")
                            .text(&format!("Piano roll — {title}"))
                        }))
                        // Node-type badge: the whole sequencer is one type now.
                        .child(html!("span", {
                            .style("display", "inline-flex")
                            .style("align-items", "center")
                            .style("gap", "5px")
                            .style("padding", "2px 8px")
                            .style("border-radius", "999px")
                            .style("font-size", "10.5px")
                            .style("font-weight", "600")
                            .style("letter-spacing", "0.04em")
                            .style("text-transform", "uppercase")
                            .style("color", if is_drum { "var(--warn)" } else { "var(--accent-bright)" })
                            .style("background", if is_drum { "var(--warn-soft)" } else { "var(--accent-ghost)" })
                            .style("border", if is_drum { "1px solid color-mix(in oklch, var(--warn) 40%, transparent)" } else { "1px solid var(--accent-line)" })
                            .text(if is_drum { "Drum kit" } else { "Melodic" })
                        }))
                    }))
                    .child(html!("button", {
                        .style_unchecked("border", "none")
                        .style("background", "transparent")
                        .style("color", "var(--text-2)")
                        .style("font-size", "16px")
                        .style("cursor", "pointer")
                        .style("line-height", "1")
                        .text("×")
                        .event(|_: events::Click| controller().close_piano_roll())
                    }))
                }))
                // Track tabs: switch which track you're editing in place.
                .child(html!("div", {
                    .style("display", "flex")
                    .style("flex-wrap", "wrap")
                    .style("gap", "4px")
                    .style("margin-bottom", "6px")
                    .children(ms.song.tracks.iter().enumerate().map(move |(i, t)| {
                        let active = i == track;
                        let label = if t.name.is_empty() { format!("track {}", i + 1) } else { t.name.clone() };
                        html!("button", {
                            .style("padding", "2px 8px")
                            .style("cursor", "pointer")
                            .style("border-radius", "4px")
                            .style("font-size", "11.5px")
                            .style("color", "inherit")
                            .style("border", "1px solid var(--line-strong)")
                            .style("background", if active { "oklch(0.42 0.08 230)" } else { "var(--bg-3)" })
                            .text(&label)
                            .event(move |_: events::Click| controller().open_piano_roll(node, i))
                        })
                    }))
                    .child(html!("button", {
                        .style("padding", "2px 8px")
                        .style("cursor", "pointer")
                        .style("border-radius", "4px")
                        .style("font-size", "11.5px")
                        .style("color", "inherit")
                        .style("border", "1px dashed var(--line-strong)")
                        .style("background", "transparent")
                        .text("+ track")
                        .event(move |_: events::Click| {
                            controller().dispatch(EditorCommand::EditSong { node, op: SongOp::AddTrack });
                            // Switch to the newly added (last) track.
                            if let Some(ms) = controller().song_node(node) {
                                controller().open_piano_roll(node, ms.song.tracks.len().saturating_sub(1));
                            }
                        })
                    }))
                    // (No per-track type toggle: the type is fixed by the node —
                    // a Melodic Sequencer or a Drum Sequencer — so every track in
                    // it is the same kind. Add the other kind as its own node.)
                    // Spacer, then length (column) controls — extend/shrink the song.
                    .child(html!("div", { .style("flex", "1") }))
                    .child(html!("span", {
                        .style("font-size", "11px").style("opacity", "0.5").style("align-self", "center")
                        .text(&format!("{} bars", (total_beats / 4.0).ceil() as u32))
                    }))
                    .child(html!("button", {
                        .style("padding", "2px 8px").style("cursor", "pointer").style("border-radius", "4px")
                        .style("font-size", "11.5px").style("color", "inherit")
                        .style("border", "1px solid var(--line-strong)").style("background", "var(--bg-3)")
                        .attr("title", "Remove a bar from the song length")
                        .text("\u{2212} bar")
                        .event(move |_: events::Click| {
                            let len = (total_beats - 4.0).max(4.0);
                            controller().dispatch(EditorCommand::EditSong { node, op: SongOp::SetLength(len) });
                        })
                    }))
                    .child(html!("button", {
                        .style("padding", "2px 8px").style("cursor", "pointer").style("border-radius", "4px")
                        .style("font-size", "11.5px").style("color", "inherit")
                        .style("border", "1px solid var(--line-strong)").style("background", "var(--bg-3)")
                        .attr("title", "Add a bar to the song length")
                        .text("+ bar")
                        .event(move |_: events::Click| {
                            controller().dispatch(EditorCommand::EditSong { node, op: SongOp::SetLength(total_beats + 4.0) });
                        })
                    }))
                }))
                // Mode explainer: what this track's type means for its outputs.
                .child(html!("div", {
                    .style("font-size", "11px")
                    .style("opacity", "0.62")
                    .style("margin-bottom", "3px")
                    .text(if is_drum {
                        "Drums: every pitch row is a separate sound — each becomes its own output you wire to its own instrument (kick, snare, hat…). The note picks the sound, not the pitch."
                    } else {
                        "Melodic: the whole track is one sound — its single output drives one instrument, and each note sets the pitch."
                    })
                }))
                .child(html!("div", {
                    .style("font-size", "11px")
                    .style("opacity", "0.5")
                    .style("margin-bottom", "6px")
                    .text("Drag to draw a note (drag right for length) · click a note to delete · drag the ▸◂ handles to set the play range")
                }))
                // Scrollable grid (auto-scrolled to the track's pitch range).
                // `flex: 1` + `min-height: 0` bound it within the column panel so
                // it scrolls vertically (a flex item's default `min-height: auto`
                // would otherwise let it grow to full content height and never
                // scroll up/down); `align-items: flex-start` keeps the gutter and
                // grid top-aligned rather than stretched.
                .child(html!("div" => web_sys::HtmlElement, {
                    .style("overflow", "auto")
                    .style("border", "1px solid var(--line)")
                    .style("border-radius", "6px")
                    .style("display", "flex")
                    .style("align-items", "flex-start")
                    .style("flex", "1 1 auto")
                    .style("min-height", "0")
                    .after_inserted(move |el| {
                        let ch = el.client_height() as f64;
                        let target = (focus_row * ROW_H + ROW_H / 2.0 - ch / 2.0).max(0.0);
                        el.set_scroll_top(target);
                    })
                    // Sticky label column, then the note grid.
                    .child(row_gutter(is_drum, height))
                    .child(svg!("svg" => web_sys::SvgElement, {
                        .attr("width", &format!("{width}"))
                        .attr("height", &format!("{height}"))
                        .attr("style", "display:block; flex:0 0 auto; touch-action:none;")
                        .after_inserted(clone!(host => move |el| {
                            *host.borrow_mut() = Some(el.unchecked_into());
                        }))
                        // Row backgrounds (black-key rows shaded).
                        .children((0..rows()).map(|r| {
                            let pitch = NOTE_MAX - r as u8;
                            let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
                            svg!("rect", {
                                .attr("x", "0")
                                .attr("y", &format!("{}", r as f64 * ROW_H))
                                .attr("width", &format!("{width}"))
                                .attr("height", &format!("{ROW_H}"))
                                .attr("fill", if black { "oklch(0.155 0.006 255)" } else { "oklch(0.196 0.006 255)" })
                                .attr("pointer-events", "none")
                            })
                        }))
                        // (Row labels live in the sticky gutter, not the SVG.)
                        // Beat / bar lines.
                        .children((0..=total_beats as usize).map(|b| {
                            let x = b as f64 * BEAT_W;
                            let bar = b % 4 == 0;
                            svg!("line", {
                                .attr("x1", &format!("{x}")).attr("y1", "0")
                                .attr("x2", &format!("{x}")).attr("y2", &format!("{height}"))
                                .attr("stroke", if bar { "oklch(0.315 0.008 255)" } else { "oklch(0.228 0.007 255)" })
                                .attr("pointer-events", "none")
                            })
                        }))
                        // Existing notes (data-idx for delete hit-testing).
                        .children(events.iter().enumerate().map(|(i, ev)| {
                            let x = ev.start * BEAT_W;
                            let y = (NOTE_MAX - ev.note.clamp(NOTE_MIN, NOTE_MAX)) as f64 * ROW_H;
                            let w = (ev.length * BEAT_W).max(2.0);
                            svg!("rect", {
                                .attr("x", &format!("{x}"))
                                .attr("y", &format!("{}", y + 0.5))
                                .attr("width", &format!("{w}"))
                                .attr("height", &format!("{}", ROW_H - 1.0))
                                .attr("rx", "2")
                                .attr("data-idx", &i.to_string())
                                .attr("fill", &note_fill(ev.velocity))
                                .attr("style", "cursor:pointer")
                            })
                        }))
                        // Live playhead during song playback (beats → x; hidden at <0).
                        .child(svg!("line", {
                            .attr("y1", "0")
                            .attr("y2", &format!("{height}"))
                            .attr("stroke", "oklch(0.85 0.18 90)")
                            .attr("stroke-width", "1.5")
                            .attr("pointer-events", "none")
                            .attr_signal("x1", controller().playhead.signal().map(|b| {
                                format!("{:.1}", if b < 0.0 { -10.0 } else { b * BEAT_W })
                            }))
                            .attr_signal("x2", controller().playhead.signal().map(|b| {
                                format!("{:.1}", if b < 0.0 { -10.0 } else { b * BEAT_W })
                            }))
                            .attr_signal("opacity", controller().playhead.signal().map(|b| {
                                (if b < 0.0 { "0" } else { "0.9" }).to_string()
                            }))
                        }))
                        // Live preview of the note being drawn.
                        .child_signal(preview.signal().map(|p| p.map(|(start, len, pitch)| {
                            svg!("rect", {
                                .attr("x", &format!("{}", start * BEAT_W))
                                .attr("y", &format!("{}", (NOTE_MAX - pitch) as f64 * ROW_H + 0.5))
                                .attr("width", &format!("{}", (len * BEAT_W).max(2.0)))
                                .attr("height", &format!("{}", ROW_H - 1.0))
                                .attr("rx", "2")
                                .attr("fill", "oklch(0.8 0.15 90 / 0.8)")
                                .attr("pointer-events", "none")
                            })
                        })))
                        // Playback window: dim the excluded regions, draw the
                        // start/stop lines, and the draggable grab handles on top.
                        .child_signal(marker.signal().map(move |(s, e)| {
                            let sx = (s * BEAT_W).max(0.0);
                            let ex = e * BEAT_W;
                            Some(svg!("g", {
                                .child(svg!("rect", {
                                    .attr("x", "0").attr("y", "0")
                                    .attr("width", &format!("{sx:.1}"))
                                    .attr("height", &format!("{height}"))
                                    .attr("fill", "oklch(0 0 0 / 0.55)")
                                    .attr("pointer-events", "none")
                                }))
                                .child(svg!("rect", {
                                    .attr("x", &format!("{ex:.1}")).attr("y", "0")
                                    .attr("width", &format!("{:.1}", (width - ex).max(0.0)))
                                    .attr("height", &format!("{height}"))
                                    .attr("fill", "oklch(0 0 0 / 0.55)")
                                    .attr("pointer-events", "none")
                                }))
                                .child(svg!("line", {
                                    .attr("x1", &format!("{sx:.1}")).attr("x2", &format!("{sx:.1}"))
                                    .attr("y1", "0").attr("y2", &format!("{height}"))
                                    .attr("stroke", "oklch(0.82 0.16 150)").attr("stroke-width", "1.5")
                                    .attr("pointer-events", "none")
                                }))
                                .child(svg!("line", {
                                    .attr("x1", &format!("{ex:.1}")).attr("x2", &format!("{ex:.1}"))
                                    .attr("y1", "0").attr("y2", &format!("{height}"))
                                    .attr("stroke", "oklch(0.72 0.18 25)").attr("stroke-width", "1.5")
                                    .attr("pointer-events", "none")
                                }))
                                .child(svg!("rect", {
                                    .attr("x", &format!("{:.1}", sx - 4.0)).attr("y", "0")
                                    .attr("width", "8").attr("height", "14").attr("rx", "2")
                                    .attr("data-marker", "start")
                                    .attr("fill", "oklch(0.82 0.16 150)")
                                    .attr("style", "cursor:ew-resize")
                                }))
                                .child(svg!("rect", {
                                    .attr("x", &format!("{:.1}", ex - 4.0)).attr("y", "0")
                                    .attr("width", "8").attr("height", "14").attr("rx", "2")
                                    .attr("data-marker", "end")
                                    .attr("fill", "oklch(0.72 0.18 25)")
                                    .attr("style", "cursor:ew-resize")
                                }))
                            }))
                        }))
                        // Pointer-down: a play-range handle (data-marker) drags the
                        // window; else on a note's right edge → resize, its body →
                        // move (a click with no move deletes), empty grid → draw.
                        .event(clone!(drag, preview, cell, events => move |e: events::PointerDown| {
                            if let Some(m) = e.target()
                                .and_then(|t| t.dyn_ref::<web_sys::Element>().and_then(|el| el.get_attribute("data-marker")))
                            {
                                *drag.borrow_mut() = Some(DragState::Marker { is_end: m == "end" });
                                return;
                            }
                            let Some((beat, pitch)) = cell(e.x(), e.y()) else { return };
                            let hit = e.target()
                                .and_then(|t| t.dyn_ref::<web_sys::Element>().and_then(|el| el.get_attribute("data-idx")))
                                .and_then(|s| s.parse::<usize>().ok())
                                .and_then(|i| events.get(i).map(|ev| (i, ev.clone())));
                            match hit {
                                Some((idx, ev)) => {
                                    // Resize handle = a fixed pixel band at the right edge, only on
                                    // notes wide enough to also leave a body (so short drum hits stay
                                    // fully clickable to move/delete).
                                    let width_px = ev.length * BEAT_W;
                                    let grab_px = (beat - ev.start) * BEAT_W;
                                    let resize = width_px >= 16.0 && grab_px >= width_px - 6.0;
                                    if resize {
                                        *drag.borrow_mut() = Some(DragState::Resize {
                                            idx, start: ev.start, pitch: ev.note, vel: ev.velocity,
                                        });
                                    } else {
                                        *drag.borrow_mut() = Some(DragState::Move {
                                            idx, len: ev.length, vel: ev.velocity,
                                            offset: ev.start - beat,
                                            orig_start: ev.start, orig_pitch: ev.note,
                                        });
                                    }
                                    preview.set(Some((ev.start, ev.length, ev.note)));
                                }
                                None => {
                                    let start = snap(beat);
                                    *drag.borrow_mut() = Some(DragState::New { start, pitch });
                                    preview.set(Some((start, SNAP, pitch)));
                                }
                            }
                        }))
                        .event(clone!(drag, preview, cell, marker => move |e: events::PointerMove| {
                            let Some((beat, pitch)) = cell(e.x(), e.y()) else { return };
                            // Dragging a play-range handle moves the marker (snapped,
                            // kept ordered) — not a note.
                            if let Some(DragState::Marker { is_end }) = &*drag.borrow() {
                                let b = ((beat / SNAP).round() * SNAP).clamp(0.0, total_beats);
                                let (s, e2) = marker.get();
                                if *is_end {
                                    marker.set((s, b.max(s + SNAP)));
                                } else {
                                    marker.set((b.min(e2 - SNAP).max(0.0), e2));
                                }
                                return;
                            }
                            let next = match &*drag.borrow() {
                                Some(DragState::New { start, pitch }) => {
                                    let len = (((beat - start) / SNAP).round() * SNAP).max(SNAP);
                                    Some((*start, len, *pitch))
                                }
                                Some(DragState::Move { len, offset, .. }) => {
                                    let start = snap(beat + offset).max(0.0);
                                    Some((start, *len, pitch))
                                }
                                Some(DragState::Resize { start, pitch, .. }) => {
                                    let len = (((beat - start) / SNAP).round() * SNAP).max(SNAP);
                                    Some((*start, len, *pitch))
                                }
                                Some(DragState::Marker { .. }) | None => None,
                            };
                            if let Some(p) = next {
                                preview.set(Some(p));
                            }
                        }))
                        .event(clone!(drag, preview, marker => move |_: events::PointerUp| {
                            let st = drag.borrow_mut().take();
                            // Committing a play-range drag: snap the marker to start
                            // / stop (a stop at the grid end means "no stop").
                            if let Some(DragState::Marker { is_end }) = st {
                                let (s, e2) = marker.get();
                                let op = if is_end {
                                    let end = if e2 >= total_beats - 0.001 { None } else { Some(e2) };
                                    SongOp::SetEnd(end)
                                } else {
                                    SongOp::SetStart(s)
                                };
                                controller().dispatch(EditorCommand::EditSong { node, op });
                                return;
                            }
                            let pv = preview.get();
                            preview.set(None);
                            let (Some(st), Some((s, l, p))) = (st, pv) else { return };
                            match st {
                                DragState::New { .. } => {
                                    controller().dispatch(EditorCommand::EditSong { node, op: SongOp::AddNote { track, event: NoteEvent {
                                        start: s, length: l, note: p, velocity: 100,
                                    } } });
                                }
                                DragState::Move { idx, len, vel, orig_start, orig_pitch, .. } => {
                                    if (s - orig_start).abs() < 1e-9 && p == orig_pitch {
                                        controller().dispatch(EditorCommand::EditSong { node, op: SongOp::RemoveNote { track, index: idx } }); // click = delete
                                    } else {
                                        controller().dispatch(EditorCommand::EditSong { node, op: SongOp::UpdateNote { track, index: idx, event: NoteEvent {
                                            start: s, length: len, note: p, velocity: vel,
                                        } } });
                                    }
                                }
                                DragState::Resize { idx, start, pitch, vel } => {
                                    controller().dispatch(EditorCommand::EditSong { node, op: SongOp::UpdateNote { track, index: idx, event: NoteEvent {
                                        start, length: l, note: pitch, velocity: vel,
                                    } } });
                                }
                                DragState::Marker { .. } => {}
                            }
                        }))
                        // Wheel over a note adjusts its velocity (louder/softer).
                        .event_with_options(&EventOptions::preventable(), clone!(events => move |e: events::Wheel| {
                            let hit = e.target()
                                .and_then(|t| t.dyn_ref::<web_sys::Element>().and_then(|el| el.get_attribute("data-idx")))
                                .and_then(|s| s.parse::<usize>().ok())
                                .and_then(|i| events.get(i).map(|ev| (i, ev.clone())));
                            if let Some((idx, ev)) = hit {
                                e.prevent_default();
                                let dv = if e.delta_y() < 0.0 { 8 } else { -8 };
                                let vel = (ev.velocity as i32 + dv).clamp(1, 127) as u8;
                                controller().dispatch(EditorCommand::EditSong { node, op: SongOp::UpdateNote { track, index: idx, event: NoteEvent { velocity: vel, ..ev } } });
                            }
                        }))
                    }))
                }))
            }))
        }))
    })
}
