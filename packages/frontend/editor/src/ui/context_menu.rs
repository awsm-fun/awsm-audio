//! The node right-click context menu. Mounted once; shown wherever the
//! controller's `context_menu` holds `(node, x, y)`. A transparent full-screen
//! backdrop closes it; the menu offers Clone and Delete.

use dominator::{events, html, Dom};
use futures_signals::signal::SignalExt;

use crate::controller::{controller, ContextTarget, EditorCommand};

pub fn render() -> Dom {
    let ctrl = controller();
    html!("div", {
        .child_signal(ctrl.context_menu.signal_cloned().map(|cm| cm.map(|(target, x, y)| view(target, x, y))))
    })
}

fn view(target: ContextTarget, x: f64, y: f64) -> Dom {
    // Two siblings: a click-to-close backdrop, and the menu itself positioned at
    // the cursor. The menu is NOT a child of the backdrop, so clicking an item
    // never reaches the backdrop's dismiss handler — dominator's propagation
    // doesn't honour `stop_propagation` reliably here, and the backdrop's
    // pointerdown would otherwise close the menu before an item's click fires.
    html!("div", {
        .style("position", "fixed")
        .style("inset", "0")
        .style("z-index", "900")
        // Backdrop: a click in the empty area dismisses.
        .child(html!("div", {
            .style("position", "absolute")
            .style("inset", "0")
            .event(|_: events::PointerDown| controller().close_context_menu())
        }))
        // Menu (sibling, on top).
        .child(html!("div", {
            .style("position", "fixed")
            .style("left", &format!("{x}px"))
            .style("top", &format!("{y}px"))
            .style("min-width", "120px")
            .style("padding", "4px")
            .style("border-radius", "8px")
            .style("background", "var(--bg-3)")
            .style("border", "1px solid var(--line-strong)")
            .style("box-shadow", "0 10px 30px oklch(0 0 0 / 0.5)")
            .children(items_for(target))
        }))
    })
}

/// The menu items for a given target.
fn items_for(target: ContextTarget) -> Vec<Dom> {
    match target {
        ContextTarget::Node(node) => vec![
            menu_item("Clone", move || {
                let c = controller();
                c.dispatch(EditorCommand::CloneNode { id: node });
                c.close_context_menu();
            }),
            menu_item("Delete", move || {
                let c = controller();
                c.dispatch(EditorCommand::RemoveNode { id: node });
                c.close_context_menu();
            }),
        ],
        ContextTarget::Wire(id) => vec![menu_item("Delete wire", move || {
            let c = controller();
            c.dispatch(EditorCommand::Disconnect { id });
            c.close_context_menu();
        })],
        ContextTarget::Clip { track, clip } => {
            let sel = controller().selected_clips();
            // A multi-selection (with this clip in it) gets group actions.
            if sel.len() > 1 && sel.iter().any(|&(t, c)| t == track && c == clip) {
                let n = sel.len();
                return vec![
                    menu_item(&format!("Copy {n} clips"), move || {
                        let c = controller();
                        c.copy_selected_clips();
                        c.close_context_menu();
                    }),
                    menu_item(&format!("Delete {n} clips"), move || {
                        let c = controller();
                        c.delete_selected_clips();
                        c.close_context_menu();
                    }),
                ];
            }
            let looping = controller()
                .clip_looping(track, clip)
                .unwrap_or(false);
            vec![
                menu_item(if looping { "Loop: on" } else { "Loop: off" }, move || {
                    let c = controller();
                    c.dispatch(EditorCommand::EditArrange {
                        op: crate::controller::ArrangeOp::SetClipLoop {
                            track,
                            clip,
                            looping: !looping,
                        },
                    });
                    c.close_context_menu();
                }),
                menu_item("Copy", move || {
                    let c = controller();
                    c.copy_clip(track, clip);
                    c.close_context_menu();
                }),
                menu_item("Open source", move || {
                    let c = controller();
                    c.open_clip_source(track, clip);
                    c.close_context_menu();
                }),
                menu_item("Delete clip", move || {
                    let c = controller();
                    c.dispatch(EditorCommand::EditArrange {
                        op: crate::controller::ArrangeOp::RemoveClip { track, clip },
                    });
                    c.close_context_menu();
                }),
            ]
        }
        ContextTarget::Sound(id) => vec![
            menu_item("Go to", move || {
                let c = controller();
                c.open_sample(id);
                c.close_context_menu();
            }),
            menu_item("Place at playhead", move || {
                let c = controller();
                c.place_sound_at_playhead(id);
                c.close_context_menu();
            }),
        ],
        ContextTarget::Lane { track, secs } => {
            if controller().has_clip_clipboard() {
                vec![
                    menu_item("Paste here", move || {
                        let c = controller();
                        c.paste_clip_at(track, secs);
                        c.close_context_menu();
                    }),
                    menu_item("Paste at playhead", move || {
                        let c = controller();
                        c.paste_clip();
                        c.close_context_menu();
                    }),
                ]
            } else {
                vec![menu_item_disabled("Nothing to paste")]
            }
        }
        ContextTarget::SampleTab(id) => vec![menu_item("Clone", move || {
            let c = controller();
            c.clone_sample(id);
            c.close_context_menu();
        })],
    }
}

fn menu_item(label: &str, on_click: impl Fn() + 'static) -> Dom {
    html!("div", {
        .style("padding", "6px 10px")
        .style("border-radius", "5px")
        .style("font-size", "12.5px")
        .style("cursor", "pointer")
        .text(label)
        .event(move |_: events::Click| on_click())
    })
}

/// A non-interactive, dimmed menu row (e.g. "Nothing to paste").
fn menu_item_disabled(label: &str) -> Dom {
    html!("div", {
        .style("padding", "6px 10px")
        .style("font-size", "12.5px")
        .style("opacity", "0.5")
        .text(label)
    })
}
