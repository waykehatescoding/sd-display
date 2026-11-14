use crate::config::{Config, Page, Tile, COLS, ROWS};
use crate::data::{Data, WeatherState};
use crate::gfx::{Canvas, Cell};
use crate::widgets::{self, Ctx, Time};
use std::collections::HashMap;
use std::sync::Arc;

pub const K: u32 = 72;
pub type Key = Arc<Vec<u8>>;

pub fn cells_for(t: &Tile, gap: f32) -> Vec<Cell> {
    let k = K as f32;
    let mut out = Vec::new();
    for r in 0..t.h {
        for c in 0..t.w {
            out.push(Cell { x: c as f32 * (k + gap), y: r as f32 * (k + gap), w: k, h: k });
        }
    }
    out
}

pub struct Frame<'a> {
    pub cfg: &'a Config,
    pub data: &'a Data,
    pub weather: &'a WeatherState,
    pub page_index: usize,
    pub now: Time,
}

pub struct Renderer {
    cache: HashMap<String, (String, Vec<Key>)>,
    black: Key,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer { cache: HashMap::new(), black: Arc::new(vec![0; (K * K * 3) as usize]) }
    }
}

impl Renderer {
    pub fn frame(&mut self, page: &Page, f: &Frame) -> Vec<Key> {
        let mut keys = vec![self.black.clone(); (COLS * ROWS) as usize];
        let gap = f.cfg.gap as f32;
        let mut seen = Vec::new();
        for t in &page.tiles {
            if !t.in_grid() || !widgets::KINDS.contains(&t.kind.as_str()) {
                continue;
            }
            seen.push(t.id.clone());
            let w = t.w as f32 * K as f32 + (t.w - 1) as f32 * gap;
            let h = t.h as f32 * K as f32 + (t.h - 1) as f32 * gap;
            let mut s = widgets::defaults(&t.kind);
            s.extend(t.settings.clone());
            let ctx = Ctx {
                w,
                h,
                cells: cells_for(t, gap),
                s: &s,
                data: f.data,
                weather: f.weather,
                cfg: f.cfg,
                page_index: f.page_index,
                page_count: f.cfg.pages.len(),
                page_name: &page.name,
                now: f.now,
            };
            let sig = format!(
                "{}|{w}x{h}|{:?}|{}{}|{}",
                t.kind,
                s,
                f.cfg.weather.units,
                f.cfg.weather.hide_city,
                widgets::sig(&t.kind, &ctx)
            );
            let fresh = self.cache.get(&t.id).map_or(true, |(old, _)| *old != sig);
            if fresh {
                let mut g = Canvas::new(w.round() as u32, h.round() as u32);
                widgets::draw(&t.kind, &mut g, &ctx);
                let slices = ctx.cells.iter().map(|c| Arc::new(slice(&g, c.x as u32, c.y as u32))).collect();
                self.cache.insert(t.id.clone(), (sig, slices));
            }
            let slices = &self.cache[&t.id].1;
            for (i, _) in ctx.cells.iter().enumerate() {
                let col = t.x + i as i64 % t.w;
                let row = t.y + i as i64 / t.w;
                keys[(row * COLS + col) as usize] = slices[i].clone();
            }
        }
        self.cache.retain(|id, _| seen.contains(id));
        keys
    }
}

fn slice(g: &Canvas, x0: u32, y0: u32) -> Vec<u8> {
    let w = g.pm.width();
    let px = g.pm.pixels();
    let mut out = Vec::with_capacity((K * K * 3) as usize);
    for y in y0..y0 + K {
        for x in x0..x0 + K {
            let c = px.get((y * w + x) as usize).map(|p| p.demultiply());
            let (r, gr, b) = c.map_or((0, 0, 0), |c| (c.red(), c.green(), c.blue()));
            out.extend_from_slice(&[r, gr, b]);
        }
    }
    out
}

pub fn panel_png(keys: &[Key], gap: u32) -> Vec<u8> {
    let w = COLS as u32 * K + (COLS as u32 - 1) * gap;
    let h = ROWS as u32 * K + (ROWS as u32 - 1) * gap;
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    let data = pm.data_mut();
    for (i, key) in keys.iter().enumerate() {
        let ox = (i as u32 % COLS as u32) * (K + gap);
        let oy = (i as u32 / COLS as u32) * (K + gap);
        for y in 0..K {
            for x in 0..K {
                let s = ((y * K + x) * 3) as usize;
                let d = (((oy + y) * w + ox + x) * 4) as usize;
                data[d..d + 4].copy_from_slice(&[key[s], key[s + 1], key[s + 2], 255]);
            }
        }
    }
    pm.encode_png().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tile(kind: &str, x: i64, y: i64, w: i64, h: i64, settings: serde_json::Value) -> Tile {
        Tile { id: kind.into(), kind: kind.into(), x, y, w, h, settings: settings.as_object().unwrap().clone(), ..Default::default() }
    }

    fn now() -> Time {
        Time { year: 2026, month: 11, day: 14, weekday: 6, hour: 14, minute: 7, second: 3 }
    }

    #[test]
    fn three_wide_tile_slices_at_97px_pitch() {
        let cells = cells_for(&tile("clock", 1, 2, 3, 1, json!({})), 25.0);
        let xs: Vec<f32> = cells.iter().map(|c| c.x).collect();
        assert_eq!(xs, vec![0.0, 97.0, 194.0]);
    }

    #[test]
    fn tiles_must_fit_the_grid() {
        assert!(tile("cpu", 3, 1, 2, 2, json!({})).in_grid());
        assert!(!tile("cpu", 4, 0, 2, 1, json!({})).in_grid());
        assert!(!tile("cpu", 0, 2, 1, 2, json!({})).in_grid());
    }

    #[test]
    fn frame_fills_covered_keys_and_reuses_slices() {
        let cfg = Config::default();
        let data = Data::default();
        let weather = WeatherState::default();
        let page = Page { name: "Test".into(), tiles: vec![tile("button", 0, 0, 2, 1, json!({ "label": "Hi", "color": "#ff0000" }))] };
        let f = Frame { cfg: &cfg, data: &data, weather: &weather, page_index: 0, now: now() };
        let mut r = Renderer::default();
        let one = r.frame(&page, &f);
        assert_eq!(one.len(), 15);
        assert_eq!(one[0].len(), (K * K * 3) as usize);
        assert!(one[0][0] > 100, "button color reaches the key");
        assert_eq!(one[2][0], 0, "uncovered key stays black");
        let two = r.frame(&page, &f);
        assert!(Arc::ptr_eq(&one[0], &two[0]), "unchanged tile is not redrawn");
    }

    #[test]
    fn hide_city_changes_the_city_key() {
        let mut cfg = Config::default();
        let data = Data::default();
        let weather = WeatherState {
            current: Some(json!({
                "name": "Springfield", "clouds": { "all": 40 }, "wind": { "speed": 3 },
                "weather": [{ "icon": "02d", "description": "few clouds" }],
                "main": { "temp": 10, "feels_like": 9, "temp_max": 12, "temp_min": 7, "humidity": 60 }
            })),
            ..Default::default()
        };
        let page = Page { name: "Test".into(), tiles: vec![tile("weather", 0, 0, 2, 2, json!({}))] };
        let shown = Renderer::default().frame(&page, &Frame { cfg: &cfg, data: &data, weather: &weather, page_index: 0, now: now() });
        cfg.weather.hide_city = true;
        let hidden = Renderer::default().frame(&page, &Frame { cfg: &cfg, data: &data, weather: &weather, page_index: 0, now: now() });
        assert_eq!(shown[1], hidden[1], "temperature key is the same");
        assert_ne!(shown[5], hidden[5], "city key changes");
    }

    #[test]
    fn every_widget_draws_without_panicking() {
        let cfg = Config::default();
        let data = Data::default();
        let weather = WeatherState::default();
        let mut r = Renderer::default();
        for kind in widgets::KINDS {
            for (w, h) in [(1, 1), (2, 1), (3, 1), (2, 2), (5, 3)] {
                let page = Page { name: "P".into(), tiles: vec![tile(kind, 0, 0, w, h, json!({}))] };
                r.frame(&page, &Frame { cfg: &cfg, data: &data, weather: &weather, page_index: 0, now: now() });
            }
        }
    }
}
