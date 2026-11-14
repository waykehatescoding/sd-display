use ab_glyph::{Font, FontRef, OutlineCurve};
use std::sync::OnceLock;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pixmap, Point,
    RadialGradient, Rect, Shader, SpreadMode, Stroke, Transform, LineCap,
};

pub static FONT_MEDIUM: &[u8] = include_bytes!("../assets/fonts/BarlowSemiCondensed-Medium.ttf");
pub static FONT_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/BarlowSemiCondensed-SemiBold.ttf");
pub static FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/BarlowSemiCondensed-Bold.ttf");

#[derive(Clone, Copy, PartialEq)]
pub enum Weight {
    Medium,
    SemiBold,
    Bold,
}

fn font(w: Weight) -> &'static FontRef<'static> {
    static FONTS: OnceLock<[FontRef<'static>; 3]> = OnceLock::new();
    let f = FONTS.get_or_init(|| {
        [
            FontRef::try_from_slice(FONT_MEDIUM).unwrap(),
            FontRef::try_from_slice(FONT_SEMIBOLD).unwrap(),
            FontRef::try_from_slice(FONT_BOLD).unwrap(),
        ]
    });
    &f[w as usize]
}

pub fn rgb(hex: &str) -> Color {
    rgba(hex, 1.0)
}

pub fn rgba(hex: &str, a: f32) -> Color {
    let h = hex.trim_start_matches('#');
    let n = u32::from_str_radix(h.get(..6).unwrap_or("000000"), 16).unwrap_or(0);
    Color::from_rgba8((n >> 16) as u8, (n >> 8) as u8, n as u8, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn white(a: f32) -> Color {
    Color::from_rgba8(255, 255, 255, (a * 255.0) as u8)
}

#[derive(Clone, Copy)]
pub struct Cell {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Canvas {
    pub pm: Pixmap,
    pub w: f32,
    pub h: f32,
    pub shadow: bool,
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        Canvas { pm: Pixmap::new(w, h).unwrap(), w: w as f32, h: h as f32, shadow: false }
    }

    fn paint(shader: Shader<'static>) -> Paint<'static> {
        Paint { shader, anti_alias: true, ..Default::default() }
    }

    pub fn fill(&mut self, path: &Path, color: Color) {
        self.pm.fill_path(path, &Self::paint(Shader::SolidColor(color)), FillRule::Winding, Transform::identity(), None);
    }

    pub fn fill_shader(&mut self, path: &Path, shader: Shader<'static>) {
        self.pm.fill_path(path, &Self::paint(shader), FillRule::Winding, Transform::identity(), None);
    }

    pub fn stroke(&mut self, path: &Path, color: Color, width: f32) {
        let stroke = Stroke { width, line_cap: LineCap::Round, ..Default::default() };
        self.pm.stroke_path(path, &Self::paint(Shader::SolidColor(color)), &stroke, Transform::identity(), None);
    }

    pub fn vgradient(&mut self, x: f32, y: f32, w: f32, h: f32, top: Color, bottom: Color) {
        if let (Some(r), Some(s)) = (Rect::from_xywh(x, y, w, h), vshader(y, y + h, top, bottom)) {
            self.pm.fill_rect(r, &Self::paint(s), Transform::identity(), None);
        }
    }

    pub fn base(&mut self, top: &str, bottom: &str) {
        let (w, h) = (self.w, self.h);
        self.vgradient(0.0, 0.0, w, h, rgb(top), rgb(bottom));
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r.max(0.1)) {
            self.fill(&p, color);
        }
    }

    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
        if let Some(p) = round_rect(x, y, w, h, r) {
            self.fill(&p, color);
        }
    }

    pub fn glow(&mut self, cx: f32, cy: f32, r0: f32, r1: f32, color: Color) {
        let mut clear = color;
        clear.set_alpha(0.0);
        let stops = vec![GradientStop::new(0.0, color), GradientStop::new(1.0, clear)];
        let c = Point::from_xy(cx, cy);
        if let (Some(s), Some(rect)) = (
            RadialGradient::new(c, r0, c, r1, stops, SpreadMode::Pad, Transform::identity()),
            Rect::from_xywh(cx - r1, cy - r1, r1 * 2.0, r1 * 2.0),
        ) {
            self.pm.fill_rect(rect, &Self::paint(s), Transform::identity(), None);
        }
    }

    pub fn text_width(text: &str, size: f32, weight: Weight) -> f32 {
        let f = font(weight);
        let s = size / f.units_per_em().unwrap_or(1000.0);
        let mut w = 0.0;
        let mut prev = None;
        for ch in text.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                w += f.kern_unscaled(p, id) * s;
            }
            w += f.h_advance_unscaled(id) * s;
            prev = Some(id);
        }
        w
    }

    pub fn text(&mut self, text: &str, cx: f32, cy: f32, size: f32, color: Color, weight: Weight, align: f32) {
        let f = font(weight);
        let s = size / f.units_per_em().unwrap_or(1000.0);
        let width = Self::text_width(text, size, weight);
        let mut x = cx - width * align;
        let baseline = cy + size * 0.35;
        let mut pb = PathBuilder::new();
        let mut prev = None;
        for ch in text.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                x += f.kern_unscaled(p, id) * s;
            }
            if let Some(outline) = f.outline(id) {
                let map = |p: ab_glyph::Point| (x + p.x * s, baseline - p.y * s);
                let mut last: Option<(f32, f32)> = None;
                for curve in &outline.curves {
                    let start = match curve {
                        OutlineCurve::Line(a, _) | OutlineCurve::Quad(a, _, _) | OutlineCurve::Cubic(a, _, _, _) => map(*a),
                    };
                    if last.map_or(true, |l| (l.0 - start.0).abs() > 0.01 || (l.1 - start.1).abs() > 0.01) {
                        if last.is_some() {
                            pb.close();
                        }
                        pb.move_to(start.0, start.1);
                    }
                    let end = match curve {
                        OutlineCurve::Line(_, b) => {
                            let b = map(*b);
                            pb.line_to(b.0, b.1);
                            b
                        }
                        OutlineCurve::Quad(_, c, b) => {
                            let (c, b) = (map(*c), map(*b));
                            pb.quad_to(c.0, c.1, b.0, b.1);
                            b
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let (c1, c2, b) = (map(*c1), map(*c2), map(*b));
                            pb.cubic_to(c1.0, c1.1, c2.0, c2.1, b.0, b.1);
                            b
                        }
                    };
                    last = Some(end);
                }
                if last.is_some() {
                    pb.close();
                }
            }
            x += f.h_advance_unscaled(id) * s;
            prev = Some(id);
        }
        let Some(path) = pb.finish() else { return };
        if self.shadow {
            for (dx, dy, a) in [(0.0, 1.5, 0.35), (0.0, 0.5, 0.25), (1.0, 1.0, 0.15), (-1.0, 1.0, 0.15)] {
                if let Some(p) = path.clone().transform(Transform::from_translate(dx, dy)) {
                    self.fill(&p, Color::from_rgba(0.0, 0.0, 0.0, a).unwrap());
                }
            }
        }
        self.fill(&path, color);
    }

    pub fn label(&mut self, text: &str, c: Cell, y: f32, size: f32, color: Color, weight: Weight) {
        let max = c.w - 10.0;
        let w = Self::text_width(text, size, weight);
        let size = if w > max { (size * max / w).max(8.0) } else { size };
        self.text(text, c.x + c.w / 2.0, c.y + y, size, color, weight, 0.5);
    }
}

pub fn vshader(y0: f32, y1: f32, top: Color, bottom: Color) -> Option<Shader<'static>> {
    LinearGradient::new(
        Point::from_xy(0.0, y0),
        Point::from_xy(0.0, y1.max(y0 + 0.01)),
        vec![GradientStop::new(0.0, top), GradientStop::new(1.0, bottom)],
        SpreadMode::Pad,
        Transform::identity(),
    )
}

pub fn hshader(x0: f32, x1: f32, stops: Vec<GradientStop>) -> Option<Shader<'static>> {
    LinearGradient::new(Point::from_xy(x0, 0.0), Point::from_xy(x1.max(x0 + 0.01), 0.0), stops, SpreadMode::Pad, Transform::identity())
}

pub fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = r * 0.5523;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}
