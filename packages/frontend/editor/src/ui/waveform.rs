//! The waveform view: a `<canvas>` driven by a requestAnimationFrame loop that
//! pulls time-domain samples from the player's analyser and draws them. When
//! nothing is playing it shows a flat center line.

use std::cell::RefCell;
use std::rc::Rc;

use dominator::{html, Dom};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::controller::controller;

const BUF_LEN: usize = 2048;

// Canvas 2D can't resolve CSS `var(--…)`, so the waveform draws with concrete
// colors that mirror the design tokens (`--bg-0`, `--accent-bright`,
// `--line-strong`). Keep these in sync with `theme::init`.
const C_BG_0: &str = "oklch(0.155 0.006 255)";
const C_ACCENT_BRIGHT: &str = "#7fa6df";
const C_LINE_STRONG: &str = "oklch(0.38 0.010 255)";

/// The self-rescheduling rAF closure cell.
type RafCell = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

pub fn render() -> Dom {
    html!("canvas" => HtmlCanvasElement, {
        .attr("width", "1200")
        .attr("height", "150")
        .style("width", "100%")
        .style("height", "150px")
        .style("display", "block")
        .style("flex", "0 0 auto")
        .style("background", "var(--bg-0)")
        .style("border-top", "1px solid var(--line)")
        .after_inserted(start_loop)
    })
}

fn start_loop(canvas: HtmlCanvasElement) {
    let Ok(Some(obj)) = canvas.get_context("2d") else {
        return;
    };
    let Ok(ctx) = obj.dyn_into::<CanvasRenderingContext2d>() else {
        return;
    };

    let mut buf = vec![128u8; BUF_LEN];
    // Self-referential rAF closure: `f` holds the closure that reschedules
    // itself, forming a deliberate cycle that lives for the page's lifetime.
    let f: RafCell = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        draw(&canvas, &ctx, &mut buf);
        request(f.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut()>));
    request(g.borrow().as_ref().unwrap());
}

fn request(c: &Closure<dyn FnMut()>) {
    let _ = web_sys::window()
        .unwrap()
        .request_animation_frame(c.as_ref().unchecked_ref());
}

fn draw(canvas: &HtmlCanvasElement, ctx: &CanvasRenderingContext2d, buf: &mut [u8]) {
    let w = canvas.width() as f64;
    let h = canvas.height() as f64;
    let ctrl = controller();
    let playing = ctrl.read_waveform(buf);

    // Drive the piano-roll playhead off this same frame loop (beats; -1 hides).
    let ph = ctrl
        .piano_roll
        .get()
        .and_then(|(node, _)| ctrl.song_playhead_beats(node))
        .unwrap_or(-1.0);
    ctrl.playhead.set_neq(ph);
    // And the arrangement timeline playhead (seconds) while performing (the idle
    // scrub marker is managed by set_arrange_start / stop, so don't clobber here).
    if let Some(s) = ctrl.arrangement_playhead_secs() {
        ctrl.arrange_playhead.set_neq(s);
    }

    ctx.set_fill_style_str(C_BG_0);
    ctx.fill_rect(0.0, 0.0, w, h);

    ctx.set_line_width(2.0);
    ctx.set_stroke_style_str(if playing {
        C_ACCENT_BRIGHT
    } else {
        C_LINE_STRONG
    });
    ctx.begin_path();
    let denom = (buf.len().max(2) - 1) as f64;
    for (i, &sample) in buf.iter().enumerate() {
        // 128 = silence/center; scale to [0, h].
        let v = if playing { sample as f64 / 128.0 } else { 1.0 };
        let x = i as f64 / denom * w;
        let y = v * h / 2.0;
        if i == 0 {
            ctx.move_to(x, y);
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.stroke();
}
