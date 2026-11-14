use crate::config::Config;
use crate::data::{Data, Series, WeatherState, HISTORY};
use crate::gfx::{hshader, rgb, rgba, vshader, white, Canvas, Cell, Weight};
use serde_json::{json, Map, Value};
use tiny_skia::{GradientStop, PathBuilder};

const INK: &str = "#0c0e12";
const INK2: &str = "#161a21";
const TEXT: &str = "#f3f1ec";
const MUTED: &str = "#8d95a3";
const FAINT: &str = "#2a2f39";
const ACCENT: &str = "#7c93ff";

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
];
const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

pub fn load_color(pct: f64) -> &'static str {
    if pct < 60.0 {
        "#3fd0b6"
    } else if pct < 85.0 {
        "#f5b041"
    } else {
        "#ff5d5d"
    }
}

#[derive(Clone, Copy)]
pub struct Time {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub weekday: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

pub struct Ctx<'a> {
    pub w: f32,
    pub h: f32,
    pub cells: Vec<Cell>,
    pub s: &'a Map<String, Value>,
    pub data: &'a Data,
    pub weather: &'a WeatherState,
    pub cfg: &'a Config,
    pub page_index: usize,
    pub page_count: usize,
    pub page_name: &'a str,
    pub now: Time,
}

impl Ctx<'_> {
    fn flag(&self, k: &str) -> bool {
        self.s.get(k).and_then(Value::as_bool).unwrap_or(false)
    }
    fn str(&self, k: &str) -> String {
        self.s.get(k).and_then(Value::as_str).unwrap_or("").to_string()
    }
}

pub const KINDS: [&str; 11] = ["clock", "date", "weather", "forecast", "cpu", "gpu", "memory", "disk", "network", "pager", "button"];

pub fn meta() -> Value {
    let toggle = |k: &str, l: &str| json!({ "key": k, "label": l, "type": "toggle" });
    let text = |k: &str, l: &str| json!({ "key": k, "label": l, "type": "text" });
    let w = |name: &str, group: &str, size: [i32; 2], defaults: Value, schema: Value, press: &str| {
        json!({ "name": name, "group": group, "size": size, "defaults": defaults, "schema": schema, "press": press })
    };
    json!({
        "clock": w("Clock", "Time", [3, 1], json!({ "h24": true, "seconds": false }),
            json!([toggle("h24", "24-hour time"), toggle("seconds", "Show seconds")]), "none"),
        "date": w("Date", "Time", [1, 1], json!({}), json!([]), "none"),
        "weather": w("Weather", "Weather", [2, 2], json!({}), json!([]), "none"),
        "forecast": w("Forecast", "Weather", [3, 1], json!({}), json!([]), "none"),
        "cpu": w("CPU", "System", [2, 1], json!({}), json!([]), "none"),
        "gpu": w("GPU", "System", [2, 1], json!({}), json!([]), "none"),
        "memory": w("Memory", "System", [2, 1], json!({}), json!([]), "none"),
        "disk": w("Disk", "System", [1, 1], json!({ "drive": "C:" }), json!([text("drive", "Drive")]), "none"),
        "network": w("Network", "System", [2, 1], json!({}), json!([]), "none"),
        "pager": w("Page", "Navigation", [1, 1], json!({}), json!([]), "next"),
        "button": w("Button", "Navigation", [1, 1], json!({ "label": "Open", "sub": "", "color": "#2b3245" }),
            json!([text("label", "Label"), text("sub", "Second line"), json!({ "key": "color", "label": "Color", "type": "color" })]), "none"),
    })
}

pub fn defaults(kind: &str) -> Map<String, Value> {
    meta()[kind]["defaults"].as_object().cloned().unwrap_or_default()
}

pub fn source(kind: &str) -> &str {
    if kind == "forecast" { "weather" } else { kind }
}

fn clock_parts(c: &Ctx) -> (String, String, String, &'static str) {
    let t = c.now;
    let h24 = c.flag("h24");
    let h = if h24 { t.hour } else if t.hour % 12 == 0 { 12 } else { t.hour % 12 };
    let hh = if h24 { format!("{h:02}") } else { h.to_string() };
    (hh, format!("{:02}", t.minute), format!("{:02}", t.second), if t.hour < 12 { "AM" } else { "PM" })
}

fn weather_sig(c: &Ctx) -> String {
    format!("{}|{:?}", c.weather.updated, c.weather.error)
}

pub fn sig(kind: &str, c: &Ctx) -> String {
    let d = c.data;
    match kind {
        "clock" => {
            let (hh, mm, ss, _) = clock_parts(c);
            format!("{hh}{mm}{}{}", if c.flag("seconds") { ss } else { String::new() }, c.now.day)
        }
        "date" => format!("{}-{}-{}", c.now.year, c.now.month, c.now.day),
        "weather" | "forecast" => weather_sig(c),
        "cpu" => d.cpu.seq.to_string(),
        "gpu" => format!("{}{}", d.gpu.seq, d.gpu_load.is_none()),
        "memory" => (d.mem_used >> 24).to_string(),
        "disk" => format!("{:?}", d.disks.get(&c.str("drive"))),
        "network" => d.net.seq.to_string(),
        "pager" => format!("{}/{}{}", c.page_index, c.page_count, c.page_name),
        _ => String::new(),
    }
}

pub fn draw(kind: &str, g: &mut Canvas, c: &Ctx) {
    match kind {
        "clock" => clock(g, c),
        "date" => date(g, c),
        "weather" => weather(g, c),
        "forecast" => forecast(g, c),
        "cpu" => {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
            meter(g, c, "CPU", c.data.cpu_load, &c.data.cpu, &[
                (threads.to_string(), "threads"),
                (format!("{}%", c.data.cpu.max().round()), "peak 90s"),
            ]);
        }
        "gpu" => match c.data.gpu_load {
            None => {
                g.base(INK2, INK);
                g.label("GPU", c.cells[0], 22.0, 14.0, rgb(MUTED), Weight::SemiBold);
                g.label("Waiting", c.cells[0], 46.0, 13.0, rgb(MUTED), Weight::Medium);
            }
            Some(load) => meter(g, c, "GPU", load, &c.data.gpu, &[(gb(c.data.vram), "GB VRAM")]),
        },
        "memory" => {
            let (u, t) = (c.data.mem_used as f64, c.data.mem_total.max(1) as f64);
            usage(g, c, "RAM", u / t * 100.0, (format!("{} / {}", gb(u), gb(t)), "GB used"), None);
        }
        "disk" => {
            let drive = c.str("drive");
            match c.data.disks.get(&drive).cloned().flatten() {
                Some((total, free)) => {
                    let pct = (total - free) as f64 / total.max(1) as f64 * 100.0;
                    usage(g, c, &drive, pct, (format!("{} GB", gb(free as f64)), "free"), Some((format!("{} GB", gb(total as f64)), "total")));
                }
                None => {
                    g.base(INK2, INK);
                    g.label(&drive, c.cells[0], 24.0, 16.0, rgb(MUTED), Weight::SemiBold);
                    g.label("Not found", c.cells[0], 46.0, 12.0, rgb(MUTED), Weight::Medium);
                }
            }
        }
        "network" => network(g, c),
        "pager" => pager(g, c),
        "button" => button(g, c),
        _ => {}
    }
}

fn gb(b: f64) -> String {
    let v = b / 1024f64.powi(3);
    if v >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") }
}

fn rate(mut bps: f64) -> (String, String) {
    let units = ["B", "KB", "MB", "GB"];
    let mut i = 0;
    while bps >= 1000.0 && i < units.len() - 1 {
        bps /= 1024.0;
        i += 1;
    }
    let v = if bps >= 100.0 || i == 0 { format!("{}", bps.round()) } else { format!("{bps:.1}") };
    (v, format!("{}/s", units[i]))
}

fn area_graph(g: &mut Canvas, hist: &Series, max: f64, x: f32, y: f32, w: f32, h: f32, color: &str) {
    let n = hist.history.len();
    if n < 2 {
        return;
    }
    let step = w / (HISTORY as f32 - 1.0);
    let off = (HISTORY - n) as f32;
    let pt = |i: usize, v: f64| (x + (off + i as f32) * step, y + h - (v.min(max) / max) as f32 * h);
    let mut line = PathBuilder::new();
    let mut area = PathBuilder::new();
    for (i, v) in hist.history.iter().enumerate() {
        let (px, py) = pt(i, *v);
        if i == 0 {
            line.move_to(px, py);
            area.move_to(px, py);
        } else {
            line.line_to(px, py);
            area.line_to(px, py);
        }
    }
    area.line_to(x + w, y + h);
    area.line_to(x + off * step, y + h);
    area.close();
    if let (Some(a), Some(s)) = (area.finish(), vshader(y, y + h, rgba(color, 0.25), rgba(color, 0.0))) {
        g.fill_shader(&a, s);
    }
    if let Some(l) = line.finish() {
        g.stroke(&l, rgb(color), 2.0);
    }
}

fn bar(g: &mut Canvas, x: f32, y: f32, w: f32, h: f32, pct: f64, color: &str) {
    g.round_rect(x, y, w, h, h / 2.0, rgb(FAINT));
    g.round_rect(x, y, (w * pct as f32 / 100.0).max(h), h, h / 2.0, rgb(color));
}

fn meter(g: &mut Canvas, c: &Ctx, name: &str, value: f64, hist: &Series, extras: &[(String, &str)]) {
    let color = load_color(value);
    g.base(INK2, INK);
    let top = if c.h > 72.0 { 60.0 } else { c.h * 0.64 };
    let (w, h) = (c.w, c.h);
    area_graph(g, hist, 100.0, 0.0, top, w, h - top, color);
    let a = c.cells[0];
    g.shadow = true;
    g.label(name, a, 11.0, 11.0, rgb(MUTED), Weight::SemiBold);
    g.label(&format!("{}%", value.round()), a, a.h * 0.42, 30.0, rgb(TEXT), Weight::SemiBold);
    for (cell, e) in c.cells.iter().skip(1).filter(|x| x.y == a.y).zip(extras) {
        g.label(&e.0, *cell, cell.h * 0.3, 22.0, rgb(TEXT), Weight::SemiBold);
        g.label(e.1, *cell, cell.h * 0.54, 11.0, rgb(MUTED), Weight::Medium);
    }
    g.shadow = false;
}

fn usage(g: &mut Canvas, c: &Ctx, name: &str, pct: f64, detail: (String, &str), more: Option<(String, &str)>) {
    g.base(INK2, INK);
    let (w, h) = (c.w, c.h);
    bar(g, 8.0, h - 12.0, w - 16.0, 5.0, pct, load_color(pct));
    let a = c.cells[0];
    let b = c.cells.get(1).copied();
    g.label(name, a, 11.0, 11.0, rgb(MUTED), Weight::SemiBold);
    g.label(&format!("{}%", pct.round()), a, a.h * if b.is_some() { 0.46 } else { 0.4 }, if b.is_some() { 32.0 } else { 26.0 }, rgb(TEXT), Weight::SemiBold);
    match b {
        Some(b) => {
            g.label(&detail.0, b, b.h * 0.36, 18.0, rgb(TEXT), Weight::SemiBold);
            g.label(detail.1, b, b.h * 0.6, 11.0, rgb(MUTED), Weight::Medium);
        }
        None => g.label(&detail.0, a, a.h * 0.68, 11.0, rgb(MUTED), Weight::Medium),
    }
    if let (Some(m), Some(cell)) = (more, c.cells.get(2)) {
        if cell.y == a.y {
            g.label(&m.0, *cell, cell.h * 0.36, 18.0, rgb(TEXT), Weight::SemiBold);
            g.label(m.1, *cell, cell.h * 0.6, 11.0, rgb(MUTED), Weight::Medium);
        }
    }
}

fn clock(g: &mut Canvas, c: &Ctx) {
    g.base("#171b22", "#0b0d11");
    let (w, h) = (c.w, c.h);
    let stops = vec![
        GradientStop::new(0.0, rgba(ACCENT, 0.0)),
        GradientStop::new(0.5, rgba(ACCENT, 0.55)),
        GradientStop::new(1.0, rgba(ACCENT, 0.0)),
    ];
    if let (Some(s), Some(r)) = (hshader(0.0, w, stops), tiny_skia::Rect::from_xywh(0.0, h - 3.0, w, 3.0)) {
        g.fill_shader(&PathBuilder::from_rect(r), s);
    }
    let (hh, mm, ss, ampm) = clock_parts(c);
    let seconds = c.flag("seconds");
    let h24 = c.flag("h24");
    if c.cells.len() == 1 {
        let a = c.cells[0];
        g.label(&format!("{hh}:{mm}"), a, a.h * if seconds { 0.42 } else { 0.5 }, 34.0, rgb(TEXT), Weight::SemiBold);
        if seconds {
            g.label(&ss, a, a.h * 0.78, 16.0, rgb(MUTED), Weight::Medium);
        } else if !h24 {
            g.label(ampm, a, a.h * 0.84, 11.0, rgb(MUTED), Weight::SemiBold);
        }
        return;
    }
    let (a, b) = (c.cells[0], c.cells[1]);
    g.label(&hh, a, a.h * 0.53, 62.0, rgb(TEXT), Weight::SemiBold);
    g.label(&mm, b, b.h * 0.53, 62.0, rgb(TEXT), Weight::SemiBold);
    let t = c.now;
    let mut extras: Vec<(String, String)> = Vec::new();
    if seconds {
        extras.push((ss, "sec".into()));
    }
    if !h24 {
        extras.push((ampm.into(), String::new()));
    }
    extras.push((DAYS[t.weekday as usize % 7].into(), t.day.to_string()));
    extras.push((MONTHS[(t.month as usize + 11) % 12][..3].into(), t.year.to_string()));
    for (i, (cell, e)) in c.cells.iter().skip(2).zip(extras).enumerate() {
        let two = !e.1.is_empty();
        let color = if i == 0 && seconds { ACCENT } else { TEXT };
        g.label(&e.0, *cell, cell.h * if two { 0.4 } else { 0.53 }, if two { 28.0 } else { 34.0 }, rgb(color), Weight::SemiBold);
        if two {
            g.label(&e.1, *cell, cell.h * 0.76, 15.0, rgb(MUTED), Weight::Medium);
        }
    }
}

fn date(g: &mut Canvas, c: &Ctx) {
    g.base(INK2, INK);
    let t = c.now;
    let a = c.cells[0];
    g.round_rect(a.x, a.y, a.w, 18.0, 0.0, rgb(ACCENT));
    g.label(DAYS[t.weekday as usize % 7], a, 10.0, 13.0, rgb(INK), Weight::Bold);
    g.label(&t.day.to_string(), a, a.h * 0.56, 36.0, rgb(TEXT), Weight::SemiBold);
    let month = MONTHS[(t.month as usize + 11) % 12];
    g.label(&month[..3], a, a.h * 0.88, 12.0, rgb(MUTED), Weight::SemiBold);
    if let Some(b) = c.cells.get(1) {
        g.label(month, *b, b.h * 0.42, 20.0, rgb(TEXT), Weight::SemiBold);
        g.label(&t.year.to_string(), *b, b.h * 0.7, 15.0, rgb(MUTED), Weight::Medium);
    }
}

fn sky(code: &str) -> (&'static str, &'static str) {
    match code {
        "01d" => ("#2a73c4", "#86c5ee"),
        "01n" => ("#070d24", "#202b58"),
        "02d" => ("#3a73ad", "#9fc2df"),
        "02n" => ("#0f1730", "#2d3858"),
        "03n" => ("#151b2b", "#343d52"),
        "04d" => ("#4b5766", "#8a97a6"),
        "04n" => ("#171b24", "#363d4b"),
        "09d" => ("#28374a", "#566a7f"),
        "09n" | "10n" => ("#10151e", "#283240"),
        "10d" => ("#2f4258", "#6a8199"),
        "11d" => ("#1f1b33", "#4b4066"),
        "11n" => ("#120f20", "#2f2845"),
        "13d" => ("#6c86a2", "#c3d1df"),
        "13n" => ("#212c42", "#56677f"),
        "50d" => ("#5d6872", "#a3acb4"),
        "50n" => ("#1c2127", "#454c55"),
        _ => ("#4a6a8c", "#9db3c8"),
    }
}

fn sun(g: &mut Canvas, cx: f32, cy: f32, r: f32) {
    g.glow(cx, cy, r * 0.4, r * 2.2, rgba("#ffd166", 0.55));
    g.circle(cx, cy, r, rgb("#ffd166"));
}

fn moon(g: &mut Canvas, cx: f32, cy: f32, r: f32, bg: (&str, &str)) {
    g.circle(cx, cy, r, rgb("#e9edf7"));
    let h = g.h;
    if let (Some(p), Some(s)) = (PathBuilder::from_circle(cx + r * 0.45, cy - r * 0.3, r * 0.85), vshader(0.0, h, rgb(bg.0), rgb(bg.1))) {
        g.fill_shader(&p, s);
    }
}

fn cloud(g: &mut Canvas, cx: f32, cy: f32, s: f32, color: &str) {
    let col = rgb(color);
    let mut pb = PathBuilder::new();
    pb.push_circle(cx - s * 0.5, cy + s * 0.1, s * 0.42);
    pb.push_circle(cx + s * 0.05, cy - s * 0.2, s * 0.55);
    pb.push_circle(cx + s * 0.6, cy + s * 0.12, s * 0.38);
    if let Some(p) = pb.finish() {
        g.fill(&p, col);
    }
    g.round_rect(cx - s * 0.92, cy + s * 0.05, s * 1.9, s * 0.47, s * 0.23, col);
}

fn drops(g: &mut Canvas, cx: f32, cy: f32, s: f32, snow: bool) {
    for i in -1i32..=1 {
        let x = cx + i as f32 * s * 0.45;
        let y = cy + if i == 0 { s * 0.15 } else { 0.0 };
        if snow {
            g.circle(x, y + s * 0.1, s * 0.09, white(1.0));
        } else {
            let mut pb = PathBuilder::new();
            pb.move_to(x + s * 0.08, y - s * 0.1);
            pb.line_to(x - s * 0.06, y + s * 0.25);
            if let Some(p) = pb.finish() {
                g.stroke(&p, rgb("#9fd3ff"), (s * 0.09).max(1.5));
            }
        }
    }
}

fn bolt(g: &mut Canvas, cx: f32, cy: f32, s: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(cx + s * 0.1, cy - s * 0.3);
    pb.line_to(cx - s * 0.2, cy + s * 0.12);
    pb.line_to(cx, cy + s * 0.12);
    pb.line_to(cx - s * 0.1, cy + s * 0.45);
    pb.line_to(cx + s * 0.22, cy - s * 0.02);
    pb.line_to(cx + s * 0.02, cy - s * 0.02);
    pb.close();
    if let Some(p) = pb.finish() {
        g.fill(&p, rgb("#ffd166"));
    }
}

fn icon(g: &mut Canvas, code: &str, cx: f32, cy: f32, s: f32, bg: (&str, &str)) {
    let kind = code.get(..2).unwrap_or("03");
    let night = code.ends_with('n');
    let orb = |g: &mut Canvas| {
        if night {
            moon(g, cx - s * 0.2, cy - s * 0.2, s * 0.42, bg)
        } else {
            sun(g, cx - s * 0.2, cy - s * 0.2, s * 0.42)
        }
    };
    match kind {
        "01" if night => moon(g, cx, cy, s * 0.55, bg),
        "01" => sun(g, cx, cy, s * 0.5),
        "02" => {
            orb(g);
            cloud(g, cx + s * 0.15, cy + s * 0.15, s * 0.6, "#f4f6fa");
        }
        "03" => cloud(g, cx, cy, s * 0.75, "#f4f6fa"),
        "04" => {
            cloud(g, cx + s * 0.25, cy - s * 0.2, s * 0.55, "#b7c0cc");
            cloud(g, cx - s * 0.05, cy + s * 0.1, s * 0.7, "#f4f6fa");
        }
        "09" | "10" => {
            if kind == "10" {
                orb(g);
            }
            cloud(g, cx, cy - s * 0.15, s * 0.7, "#dfe5ee");
            drops(g, cx, cy + s * 0.45, s * 0.8, false);
        }
        "11" => {
            cloud(g, cx, cy - s * 0.15, s * 0.7, "#a9b1c2");
            bolt(g, cx, cy + s * 0.35, s);
        }
        "13" => {
            cloud(g, cx, cy - s * 0.15, s * 0.7, "#f4f6fa");
            drops(g, cx, cy + s * 0.45, s * 0.8, true);
        }
        _ => {
            for i in -1i32..=1 {
                let mut pb = PathBuilder::new();
                pb.move_to(cx - s * 0.5 + if i & 1 != 0 { s * 0.15 } else { 0.0 }, cy + i as f32 * s * 0.25);
                pb.line_to(cx + s * 0.5, cy + i as f32 * s * 0.25);
                if let Some(p) = pb.finish() {
                    g.stroke(&p, rgb("#e8ecf2"), s * 0.1);
                }
            }
        }
    }
}

fn weather_error(g: &mut Canvas, c: &Ctx) {
    g.base("#232a36", "#12161c");
    let a = c.cells[0];
    cloud(g, a.x + a.w / 2.0, a.y + a.h * 0.36, a.w * 0.32, "#4a5363");
    let msg = c.weather.error.as_deref().unwrap_or("Loading");
    g.label(msg, a, a.h * 0.78, 13.0, rgb(MUTED), Weight::SemiBold);
}

fn deg(t: f64) -> String {
    format!("{}°", t.round())
}

fn weather(g: &mut Canvas, c: &Ctx) {
    let Some(cur) = &c.weather.current else { return weather_error(g, c) };
    let code = cur["weather"][0]["icon"].as_str().unwrap_or("03d");
    let bg = sky(code);
    g.base(bg.0, bg.1);
    let m = &cur["main"];
    let f = |v: &Value| v.as_f64().unwrap_or(0.0);
    let temp = deg(f(&m["temp"]));
    let one = c.cells.len() == 1;
    let a = c.cells[0];
    icon(g, code, a.x + a.w / 2.0, a.y + a.h * if one { 0.36 } else { 0.5 }, a.w * if one { 0.34 } else { 0.48 }, bg);
    g.shadow = true;
    if one {
        g.label(&temp, a, a.h * 0.8, 20.0, rgb(TEXT), Weight::SemiBold);
        g.shadow = false;
        return;
    }
    let soft = white(0.85);
    let mut desc = cur["weather"][0]["description"].as_str().unwrap_or("").to_string();
    if let Some(first) = desc.get(..1) {
        desc = first.to_uppercase() + &desc[1..];
    }
    let unit = if c.cfg.weather.units == "imperial" { "mph" } else { "m/s" };
    for (i, cell) in c.cells.iter().enumerate().skip(1) {
        let cell = *cell;
        match i {
            1 => {
                g.label(&temp, cell, cell.h * 0.46, 40.0, rgb(TEXT), Weight::SemiBold);
                g.label(&format!("Feels {}", deg(f(&m["feels_like"]))), cell, cell.h * 0.82, 12.0, white(0.8), Weight::SemiBold);
            }
            2 if c.cfg.weather.hide_city => {
                g.label(&desc, cell, cell.h * 0.4, 17.0, rgb(TEXT), Weight::SemiBold);
                let clouds = cur["clouds"]["all"].as_i64().unwrap_or(0);
                g.label(&format!("{clouds}% clouds"), cell, cell.h * 0.7, 12.0, soft, Weight::Medium);
            }
            2 => {
                g.label(cur["name"].as_str().unwrap_or(""), cell, cell.h * 0.4, 17.0, rgb(TEXT), Weight::SemiBold);
                g.label(&desc, cell, cell.h * 0.7, 12.0, soft, Weight::Medium);
            }
            3 => {
                g.label(&format!("{} / {}", deg(f(&m["temp_max"])), deg(f(&m["temp_min"]))), cell, cell.h * 0.4, 18.0, rgb(TEXT), Weight::SemiBold);
                g.label(&format!("{}% humid", f(&m["humidity"]).round()), cell, cell.h * 0.7, 12.0, soft, Weight::Medium);
            }
            4 => {
                g.label(&format!("{}", f(&cur["wind"]["speed"]).round()), cell, cell.h * 0.42, 26.0, rgb(TEXT), Weight::SemiBold);
                g.label(&format!("wind {unit}"), cell, cell.h * 0.74, 12.0, soft, Weight::Medium);
            }
            _ => {}
        }
    }
    g.shadow = false;
}

fn forecast(g: &mut Canvas, c: &Ctx) {
    let Some(days) = &c.weather.forecast else { return weather_error(g, c) };
    g.base("#1a2230", "#0c1016");
    for (i, (cell, d)) in c.cells.iter().zip(days).enumerate() {
        let cell = *cell;
        let name = if i == 0 { "Today" } else { DAYS[d.weekday % 7] };
        g.label(name, cell, 11.0, 12.0, rgb(MUTED), Weight::SemiBold);
        icon(g, &d.icon, cell.x + cell.w / 2.0, cell.y + cell.h * 0.43, cell.w * 0.25, ("#1a2230", "#0c1016"));
        let y = cell.y + cell.h * 0.82;
        let mid = cell.x + cell.w / 2.0;
        g.text(&deg(d.max), mid - 3.0, y, 15.0, rgb(TEXT), Weight::SemiBold, 1.0);
        g.text(&deg(d.min), mid + 3.0, y, 15.0, rgb(MUTED), Weight::SemiBold, 0.0);
    }
}

fn network(g: &mut Canvas, c: &Ctx) {
    g.base(INK2, INK);
    let d = c.data;
    let (w, h) = (c.w, c.h);
    let max = d.net.max().max(64.0 * 1024.0);
    let top = h - h.min(72.0) * 0.36;
    area_graph(g, &d.net, max, 0.0, top, w, h - top, ACCENT);
    let (down, down_u) = rate(d.down);
    let (up, up_u) = rate(d.up);
    let a = c.cells[0];
    g.shadow = true;
    match c.cells.get(1) {
        None => {
            g.label(&down, a, a.h * 0.28, 22.0, rgb(TEXT), Weight::SemiBold);
            g.label(&format!("{down_u} down"), a, a.h * 0.53, 11.0, rgb(MUTED), Weight::Medium);
        }
        Some(b) => {
            g.label(&down, a, a.h * 0.3, 26.0, rgb(TEXT), Weight::SemiBold);
            g.label(&format!("{down_u} down"), a, a.h * 0.56, 11.0, rgb(MUTED), Weight::Medium);
            g.label(&up, *b, b.h * 0.3, 26.0, rgb(TEXT), Weight::SemiBold);
            g.label(&format!("{up_u} up"), *b, b.h * 0.56, 11.0, rgb(MUTED), Weight::Medium);
        }
    }
    g.shadow = false;
}

fn pager(g: &mut Canvas, c: &Ctx) {
    g.base(INK2, INK);
    let a = c.cells[0];
    g.label(c.page_name, a, a.h * 0.36, 16.0, rgb(TEXT), Weight::SemiBold);
    let gap = 10.0;
    let x0 = a.x + a.w / 2.0 - (c.page_count as f32 - 1.0) * gap / 2.0;
    for i in 0..c.page_count {
        let on = i == c.page_index;
        g.circle(x0 + i as f32 * gap, a.y + a.h * 0.7, if on { 3.5 } else { 2.5 }, rgb(if on { ACCENT } else { FAINT }));
    }
}

fn shade(hex: &str, amt: f32) -> String {
    let n = u32::from_str_radix(hex.trim_start_matches('#').get(..6).unwrap_or("2b3245"), 16).unwrap_or(0x2b3245);
    let ch = |v: u32| (v as f32 * (1.0 + amt)).clamp(0.0, 255.0) as u32;
    format!("#{:02x}{:02x}{:02x}", ch(n >> 16 & 255), ch(n >> 8 & 255), ch(n & 255))
}

fn button(g: &mut Canvas, c: &Ctx) {
    let mut color = c.str("color");
    if !color.starts_with('#') || color.len() < 7 {
        color = "#2b3245".into();
    }
    g.base(&color, &shade(&color, -0.35));
    let a = c.cells[0];
    let sub = c.str("sub");
    g.label(&c.str("label"), a, a.h * if sub.is_empty() { 0.5 } else { 0.42 }, 20.0, rgb(TEXT), Weight::SemiBold);
    if !sub.is_empty() {
        g.label(&sub, a, a.h * 0.7, 12.0, white(0.7), Weight::Medium);
    }
}
