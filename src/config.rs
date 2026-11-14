use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub const COLS: i64 = 5;
pub const ROWS: i64 = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Weather {
    pub api_key: String,
    pub city: String,
    pub units: String,
    pub hide_city: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Weather { api_key: String::new(), city: "London".into(), units: "metric".into(), hide_city: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Press {
    #[serde(rename = "type")]
    pub kind: String,
    pub page: i64,
    pub target: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Tile {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub settings: Map<String, Value>,
    pub press: Press,
}

impl Default for Tile {
    fn default() -> Self {
        Tile { id: String::new(), kind: String::new(), x: 0, y: 0, w: 1, h: 1, settings: Map::new(), press: Press::default() }
    }
}

impl Tile {
    pub fn in_grid(&self) -> bool {
        self.x >= 0 && self.y >= 0 && self.w >= 1 && self.h >= 1 && self.x + self.w <= COLS && self.y + self.h <= ROWS
    }

    pub fn covers(&self, col: i64, row: i64) -> bool {
        col >= self.x && col < self.x + self.w && row >= self.y && row < self.y + self.h
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Page {
    pub name: String,
    pub tiles: Vec<Tile>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub brightness: i64,
    pub gap: i64,
    pub weather: Weather,
    pub active_page: i64,
    pub pages: Vec<Page>,
}

fn tile(id: &str, kind: &str, x: i64, y: i64, w: i64, h: i64, settings: Value, press: &str) -> Tile {
    Tile {
        id: id.into(),
        kind: kind.into(),
        x,
        y,
        w,
        h,
        settings: settings.as_object().cloned().unwrap_or_default(),
        press: Press { kind: press.into(), page: 0, target: String::new() },
    }
}

impl Default for Config {
    fn default() -> Self {
        let clock = json!({ "h24": true, "seconds": false });
        Config {
            brightness: 70,
            gap: 25,
            weather: Weather::default(),
            active_page: 0,
            pages: vec![
                Page {
                    name: "Home".into(),
                    tiles: vec![
                        tile("t1", "clock", 0, 0, 3, 1, clock.clone(), "none"),
                        tile("t2", "weather", 3, 0, 2, 2, json!({}), "refresh"),
                        tile("t3", "pager", 0, 1, 1, 1, json!({}), "next"),
                        tile("t4", "cpu", 1, 1, 2, 1, json!({}), "none"),
                        tile("t5", "memory", 0, 2, 1, 1, json!({}), "none"),
                        tile("t6", "gpu", 1, 2, 1, 1, json!({}), "none"),
                        tile("t7", "network", 2, 2, 1, 1, json!({}), "none"),
                        tile("t8", "forecast", 3, 2, 2, 1, json!({}), "none"),
                    ],
                },
                Page {
                    name: "System".into(),
                    tiles: vec![
                        tile("s7", "clock", 0, 0, 1, 1, clock, "none"),
                        tile("s6", "pager", 0, 1, 1, 1, json!({}), "next"),
                        tile("s1", "cpu", 1, 0, 2, 2, json!({}), "none"),
                        tile("s2", "gpu", 3, 0, 2, 1, json!({}), "none"),
                        tile("s3", "memory", 3, 1, 2, 1, json!({}), "none"),
                        tile("s5", "network", 0, 2, 2, 1, json!({}), "none"),
                        tile("s4", "disk", 2, 2, 3, 1, json!({ "drive": "C:" }), "none"),
                    ],
                },
            ],
        }
    }
}

const PRESS: [&str; 6] = ["none", "next", "prev", "page", "open", "refresh"];

fn cut(s: &mut String, max: usize) {
    if let Some((i, _)) = s.char_indices().nth(max) {
        s.truncate(i);
    }
}

impl Config {
    pub fn sanitize(&mut self, known: &dyn Fn(&str) -> bool) -> Result<(), String> {
        if self.pages.is_empty() {
            return Err("Config needs at least one page".into());
        }
        self.pages.truncate(20);
        self.brightness = self.brightness.clamp(0, 100);
        self.gap = self.gap.clamp(0, 60);
        let w = &mut self.weather;
        cut(&mut w.api_key, 100);
        cut(&mut w.city, 100);
        w.api_key = w.api_key.trim().into();
        w.city = w.city.trim().into();
        if w.units != "imperial" {
            w.units = "metric".into();
        }
        for (i, p) in self.pages.iter_mut().enumerate() {
            cut(&mut p.name, 30);
            if p.name.trim().is_empty() {
                p.name = format!("Page {}", i + 1);
            }
            p.tiles.truncate(15);
            p.tiles.retain(|t| !t.id.is_empty() && known(&t.kind) && t.in_grid());
            for t in &mut p.tiles {
                cut(&mut t.id, 40);
                if !PRESS.contains(&t.press.kind.as_str()) {
                    t.press.kind = "none".into();
                }
                t.press.page = t.press.page.clamp(0, 19);
                cut(&mut t.press.target, 500);
            }
        }
        self.active_page = self.active_page.clamp(0, self.pages.len() as i64 - 1);
        Ok(())
    }
}

pub fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("streamdeckdisplay")
}

pub fn load() -> Config {
    let file = dir().join("config.json");
    match std::fs::read_to_string(&file) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            eprintln!("config unreadable, using defaults: {e}");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

pub fn save(cfg: &Config) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let tmp = d.join("config.json.tmp");
    let json = serde_json::to_string_pretty(cfg).unwrap_or_default();
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, d.join("config.json"));
    }
}
