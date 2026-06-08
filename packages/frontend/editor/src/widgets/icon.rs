//! Geometric 24×24 line icons (ported from the awsm-renderer set, plus the
//! audio-specific glyphs this editor needs). Each icon is a name → list of svg
//! child shapes; [`Icon`] wraps them in an `<svg>` that strokes with
//! `currentColor`, so a glyph inherits the surrounding text color.

use dominator::{svg, Dom};

/// Stroked path (inherits `fill:none; stroke:currentColor` from the wrapper).
fn sp(d: &str) -> Dom {
    svg!("path", { .attr("d", d) })
}
fn sc(cx: &str, cy: &str, r: &str) -> Dom {
    svg!("circle", { .attr("cx", cx).attr("cy", cy).attr("r", r) })
}
fn fc(cx: &str, cy: &str, r: &str) -> Dom {
    svg!("circle", {
        .attr("cx", cx).attr("cy", cy).attr("r", r).attr("fill", "currentColor").attr("stroke", "none")
    })
}
fn se(cx: &str, cy: &str, rx: &str, ry: &str) -> Dom {
    svg!("ellipse", { .attr("cx", cx).attr("cy", cy).attr("rx", rx).attr("ry", ry) })
}
fn sr(x: &str, y: &str, w: &str, h: &str, rx: &str) -> Dom {
    svg!("rect", { .attr("x", x).attr("y", y).attr("width", w).attr("height", h).attr("rx", rx) })
}
/// Filled path (no stroke) — solid transport glyphs (play/pause/stop).
fn fp(d: &str) -> Dom {
    svg!("path", {
        .attr("d", d).attr("fill", "currentColor").attr("stroke", "none")
    })
}

/// The svg child shapes for a named icon. Unknown names fall back to `dot`.
fn icon_children(name: &str) -> Vec<Dom> {
    match name {
        // --- brand / audio ---
        // Eighth note: a filled head + a stem with a flag.
        "note" => vec![
            se("8", "17.5", "3.2", "2.4"),
            sp("M11.2 17.5V5l7 -2v9.2"),
            fp("M18.2 3l0 4.2 -7 2 0 -4.2z"),
        ],
        "speaker" => vec![
            sp("M4 9v6h3.5L13 19V5L7.5 9H4z"),
            sp("M16 9.2a4 4 0 010 5.6M18.4 7a7 7 0 010 10"),
        ],
        "wave" => vec![sp(
            "M2 12c2 0 2-6 4-6s2 12 4 12 2-9 4-9 2 6 4 6 2-3 2-3",
        )],

        // --- transport (filled, definitive shapes) ---
        "play" => vec![fp("M7 5.5v13l11-6.5z")],
        "pause" => vec![sr("6.5", "5.5", "3.5", "13", "1"), sr("14", "5.5", "3.5", "13", "1")],
        "stop" => vec![sr("6", "6", "12", "12", "1.6")],
        "loop" => vec![
            sp("M5 9a6 6 0 016-6h6"),
            sp("M14 0.5L17.5 3 14 5.5"),
            sp("M19 15a6 6 0 01-6 6H7"),
            sp("M10 23.5L6.5 21 10 18.5"),
        ],
        "record" => vec![fc("12", "12", "6")],

        // --- actions ---
        "plus" => vec![sp("M12 5v14M5 12h14")],
        "minus" => vec![sp("M5 12h14")],
        "trash" => vec![sp("M4.5 7h15M9 7V5.2A1.2 1.2 0 0110.2 4h3.6A1.2 1.2 0 0115 5.2V7M6.5 7l.8 12a1.5 1.5 0 001.5 1.4h6.4a1.5 1.5 0 001.5-1.4l.8-12")],
        "search" => vec![sc("11", "11", "6.5"), sp("M16 16l4 4")],
        "save" => vec![sp("M5 4.5h11l3 3v12h-14z"), sp("M8 4.5v5h7v-5M8 19.5v-6h8v6")],
        "folder" => vec![sp("M3.5 6.5h6l1.6 2h9.4v9.5a1 1 0 01-1 1h-15a1 1 0 01-1-1z")],
        "undo" => vec![sp("M9 7L4.5 11.5 9 16"), sp("M4.5 11.5H14a5.5 5.5 0 010 11h-3")],
        "redo" => vec![sp("M15 7l4.5 4.5L15 16"), sp("M19.5 11.5H10a5.5 5.5 0 000 11h3")],
        "more" => vec![fc("6", "12", "1.4"), fc("12", "12", "1.4"), fc("18", "12", "1.4")],
        "help" => vec![sc("12", "12", "8.5"), sp("M9.6 9.4a2.4 2.4 0 114 1.8c-1 .7-1.6 1.2-1.6 2.3"), fc("12", "16.6", "0.6")],
        "settings" => vec![
            sc("12", "12", "3.2"),
            sp("M12 3.5v2.2M12 18.3v2.2M3.5 12h2.2M18.3 12h2.2M5.9 5.9l1.6 1.6M16.5 16.5l1.6 1.6M18.1 5.9l-1.6 1.6M7.5 16.5l-1.6 1.6"),
        ],
        "fit" => vec![
            sp("M4 8V5.5A1.5 1.5 0 015.5 4H8M16 4h2.5A1.5 1.5 0 0120 5.5V8M20 16v2.5a1.5 1.5 0 01-1.5 1.5H16M8 20H5.5A1.5 1.5 0 014 18.5V16"),
        ],
        "sparkle" => vec![
            sp("M12 4l1.6 4.4L18 10l-4.4 1.6L12 16l-1.6-4.4L6 10l4.4-1.6z"),
            sp("M18 14l.7 1.8L20.5 16.5l-1.8.7L18 19l-.7-1.8L15.5 16.5l1.8-.7z"),
        ],
        "doc" => vec![sp("M6 3.5h7l5 5v12h-12z"), sp("M13 3.5v5h5")],
        "download" => vec![sp("M12 4v10"), sp("M8 10.5l4 4 4-4"), sp("M4.5 19.5h15")],
        "star" => vec![sp("M12 4l2.3 5 5.4.5-4.1 3.6 1.2 5.3L12 20.6 7.2 23.4l1.2-5.3L4.3 14.5l5.4-.5z")],
        "chevron" => vec![sp("M9 6l6 6-6 6")],
        "chevdown" => vec![sp("M6 9l6 6 6-6")],
        "close" => vec![sp("M6 6l12 12M18 6L6 18")],
        "copy" => vec![sr("8.5", "8.5", "11", "11", "2"), sp("M5.5 15.5h-1a1 1 0 01-1-1v-9a1 1 0 011-1h9a1 1 0 011 1v1")],
        "dot" => vec![fc("12", "12", "3.5")],
        _ => vec![fc("12", "12", "3.5")], // dot fallback
    }
}

/// Builder for a single line icon. Defaults: 16px, 1.6 stroke, `currentColor`.
pub struct Icon {
    name: String,
    size: f64,
    sw: f64,
    styles: Vec<(String, String)>,
}

impl Icon {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 16.0,
            sw: 1.6,
            styles: Vec::new(),
        }
    }
    pub fn size(mut self, size: f64) -> Self {
        self.size = size;
        self
    }
    pub fn stroke_width(mut self, sw: f64) -> Self {
        self.sw = sw;
        self
    }
    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.styles.push(("color".to_string(), color.into()));
        self
    }
    pub fn style(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.styles.push((key.into(), value.into()));
        self
    }

    pub fn render(self) -> Dom {
        let size = format!("{}", self.size);
        let sw = format!("{}", self.sw);
        // SVG elements don't take dominator's `.style()` builder cleanly, so
        // compose all inline styles into the `style` attribute string.
        let mut style = String::from("display:block;flex-shrink:0;");
        for (k, v) in &self.styles {
            style.push_str(k);
            style.push(':');
            style.push_str(v);
            style.push(';');
        }
        svg!("svg", {
            .attr("viewBox", "0 0 24 24")
            .attr("width", &size)
            .attr("height", &size)
            .attr("fill", "none")
            .attr("stroke", "currentColor")
            .attr("stroke-width", &sw)
            .attr("stroke-linecap", "round")
            .attr("stroke-linejoin", "round")
            .attr("aria-hidden", "true")
            .attr("style", &style)
            .children(icon_children(&self.name))
        })
    }
}
