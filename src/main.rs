#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod data;
mod gfx;
mod render;
mod server;
mod widgets;

use config::{Config, COLS};
use elgato_streamdeck::{list_devices, new_hidapi, refresh_device_list, StreamDeck, StreamDeckInput};
use render::{Key, Renderer, K};
use std::collections::HashSet;
use std::io::Write;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub enum Msg {
    Wake,
    Quit,
}

pub struct Shared {
    pub cfg: Config,
    pub device: serde_json::Value,
    pub clients: Vec<Box<dyn Write + Send>>,
    pub new_client: bool,
    pub refresh_weather: bool,
}

pub type App = Arc<Mutex<Shared>>;

impl Shared {
    pub fn state(&self) -> String {
        serde_json::json!({ "config": self.cfg, "device": self.device, "widgets": widgets::meta() }).to_string()
    }

    pub fn broadcast(&mut self, event: &str, data: &str) {
        let msg = format!("event: {event}\ndata: {data}\n\n");
        self.clients.retain_mut(|c| c.write_all(msg.as_bytes()).and_then(|_| c.flush()).is_ok());
    }

    fn broadcast_state(&mut self) {
        let s = self.state();
        self.broadcast("state", &s);
    }

    pub fn set_config(&mut self, mut cfg: Config) -> Result<(), String> {
        cfg.sanitize(&|k| widgets::KINDS.contains(&k))?;
        config::save(&cfg);
        self.cfg = cfg;
        self.broadcast_state();
        Ok(())
    }

    pub fn set_page(&mut self, index: i64) {
        let n = self.cfg.pages.len() as i64;
        self.cfg.active_page = index.rem_euclid(n);
        config::save(&self.cfg);
        self.broadcast_state();
    }
}

static QUIT: AtomicBool = AtomicBool::new(false);

fn now() -> widgets::Time {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    widgets::Time {
        year: t.wYear,
        month: t.wMonth,
        day: t.wDay,
        weekday: t.wDayOfWeek,
        hour: t.wHour,
        minute: t.wMinute,
        second: t.wSecond,
    }
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn open(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file) = (w("open"), w(target));
    unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
}

fn disable_sleep(api: &hidapi::HidApi, vid: u16, pid: u16, serial: &str) -> bool {
    let Ok(dev) = api.open_serial(vid, pid, serial) else { return false };
    let mut set = [0u8; 32];
    set[..2].copy_from_slice(&[0x03, 0x0D]);
    let mut get = [0u8; 32];
    get[0] = 0x0A;
    dev.send_feature_report(&set).is_ok() && dev.get_feature_report(&mut get).is_ok() && get[2..6] == [0; 4]
}

fn connect(hid: &mut Option<hidapi::HidApi>, app: &App) -> Option<StreamDeck> {
    if hid.is_none() {
        *hid = new_hidapi().ok();
    }
    let api = hid.as_mut()?;
    refresh_device_list(api).ok()?;
    let (kind, serial) = list_devices(api).into_iter().find(|(k, _)| k.key_count() == 15)?;
    let deck = match StreamDeck::connect(api, kind, &serial) {
        Ok(d) => d,
        Err(e) => {
            let mut s = app.lock().unwrap();
            if s.device.get("error").is_none() {
                eprintln!("Could not open the Stream Deck: {e}");
                s.device = serde_json::json!({ "error": "busy" });
                s.broadcast_state();
            }
            return None;
        }
    };
    if !disable_sleep(api, kind.vendor_id(), kind.product_id(), &serial) {
        eprintln!("Could not turn off the Stream Deck sleep timer");
    }
    let name = deck.product().unwrap_or_else(|_| "Stream Deck".into());
    println!("Connected to {name}");
    let mut s = app.lock().unwrap();
    s.device = serde_json::json!({ "name": name, "serial": serial });
    s.broadcast_state();
    Some(deck)
}

fn press(app: &App, sources: &mut data::Sources, key: usize) {
    let mut s = app.lock().unwrap();
    let col = key as i64 % COLS;
    let row = key as i64 / COLS;
    let page = &s.cfg.pages[s.cfg.active_page as usize];
    let Some(t) = page.tiles.iter().find(|t| t.covers(col, row)) else { return };
    let action = if t.press.kind == "none" && t.kind == "pager" { "next".to_string() } else { t.press.kind.clone() };
    let (target, page_no) = (t.press.target.clone(), t.press.page);
    let active = s.cfg.active_page;
    match action.as_str() {
        "next" => s.set_page(active + 1),
        "prev" => s.set_page(active - 1),
        "page" => s.set_page(page_no),
        "refresh" => sources.refresh_weather(),
        "open" if !target.is_empty() => open(&target),
        _ => {}
    }
}

fn drop_deck(deck: &mut Option<StreamDeck>, app: &App) {
    if deck.take().is_some() {
        eprintln!("Stream Deck disconnected");
        let mut s = app.lock().unwrap();
        s.device = serde_json::Value::Null;
        s.broadcast_state();
    }
}

fn run(app: App, tx: Sender<Msg>, rx: Receiver<Msg>) {
    let mut sources = data::Sources::new(tx.clone());
    let mut renderer = Renderer::default();
    let mut hid = None;
    let mut deck: Option<StreamDeck> = None;
    let mut last_sent: Vec<Option<Key>> = vec![None; 15];
    let mut held = vec![false; 15];
    let mut last_try = Instant::now() - Duration::from_secs(10);
    let mut brightness = -1;

    loop {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_millis()).unwrap_or(0);
        let deadline = Instant::now() + Duration::from_millis(1000 - ms as u64 + 5);
        loop {
            if QUIT.load(Ordering::Relaxed) {
                if let Some(d) = &deck {
                    let _ = d.reset();
                }
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let mut wake = false;
            if let Some(d) = &deck {
                match d.read_input(Some(left.min(Duration::from_millis(100)))) {
                    Ok(StreamDeckInput::ButtonStateChange(states)) => {
                        for (i, (&down, was)) in states.iter().zip(held.iter_mut()).enumerate() {
                            if down && !*was {
                                press(&app, &mut sources, i);
                                wake = true;
                            }
                            *was = down;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        drop_deck(&mut deck, &app);
                        wake = true;
                    }
                }
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Msg::Wake => wake = true,
                        Msg::Quit => QUIT.store(true, Ordering::Relaxed),
                    }
                }
            } else {
                match rx.recv_timeout(left) {
                    Ok(Msg::Wake) => wake = true,
                    Ok(Msg::Quit) => QUIT.store(true, Ordering::Relaxed),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if wake {
                break;
            }
        }

        if deck.is_none() && last_try.elapsed() > Duration::from_secs(3) {
            last_try = Instant::now();
            deck = connect(&mut hid, &app);
            last_sent = vec![None; 15];
            held = vec![false; 15];
            brightness = -1;
        }

        let (cfg, clients, new_client, refresh) = {
            let mut s = app.lock().unwrap();
            let r = (s.cfg.clone(), s.clients.len(), s.new_client, s.refresh_weather);
            s.new_client = false;
            s.refresh_weather = false;
            r
        };
        if let Some(d) = &deck {
            if brightness != cfg.brightness {
                brightness = cfg.brightness;
                let _ = d.set_brightness(cfg.brightness as u8);
            }
        }

        let page_index = cfg.active_page as usize;
        let page = &cfg.pages[page_index];
        let need: HashSet<String> = page.tiles.iter().map(|t| widgets::source(&t.kind).to_string()).collect();
        let drives: HashSet<String> = page
            .tiles
            .iter()
            .filter(|t| t.kind == "disk")
            .map(|t| t.settings.get("drive").and_then(|v| v.as_str()).unwrap_or("C:").to_string())
            .collect();
        sources.set_weather(&cfg.weather);
        if refresh {
            sources.refresh_weather();
        }
        sources.tick(&need, &drives);

        if deck.is_none() && clients == 0 {
            continue;
        }

        let weather = sources.data.weather.clone();
        let weather = weather.lock().unwrap();
        let keys = renderer.frame(page, &render::Frame { cfg: &cfg, data: &sources.data, weather: &weather, page_index, now: now() });
        drop(weather);

        let mut changed = false;
        let mut failed = false;
        for (i, key) in keys.iter().enumerate() {
            if last_sent[i].as_ref().is_some_and(|k| Arc::ptr_eq(k, key)) {
                continue;
            }
            changed = true;
            last_sent[i] = Some(key.clone());
            if let Some(d) = &deck {
                let img = image::RgbImage::from_raw(K, K, key.to_vec()).unwrap();
                failed |= d.set_button_image(i as u8, image::DynamicImage::ImageRgb8(img)).is_err();
            }
        }
        if let Some(d) = &deck {
            if changed && (failed || d.flush().is_err()) {
                drop_deck(&mut deck, &app);
            }
        }
        if clients > 0 && (changed || new_client) {
            let png = render::panel_png(&keys, cfg.gap as u32);
            let url = format!("\"data:image/png;base64,{}\"", base64(&png));
            app.lock().unwrap().broadcast("frame", &url);
        }
    }
}

extern "system" fn on_ctrl(_: u32) -> i32 {
    QUIT.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(400));
    1
}

fn main() {
    unsafe {
        use windows_sys::Win32::System::Console::{AttachConsole, SetConsoleCtrlHandler, ATTACH_PARENT_PROCESS};
        AttachConsole(ATTACH_PARENT_PROCESS);
        SetConsoleCtrlHandler(Some(on_ctrl), 1);
    }
    let background = std::env::args().any(|a| a == "--background");
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3325);
    let url = format!("http://localhost:{port}");

    let server = match tiny_http::Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("Already running at {url}");
            if let (false, Some(t)) = (background, config::saved_token()) {
                open(&format!("{url}/?t={t}"));
            }
            return;
        }
    };
    let token = config::token();
    println!("Configure at {url}");

    let app: App = Arc::new(Mutex::new(Shared {
        cfg: config::load(&|k| widgets::KINDS.contains(&k)),
        device: serde_json::Value::Null,
        clients: Vec::new(),
        new_client: false,
        refresh_weather: false,
    }));
    let (tx, rx) = channel();
    {
        let (app, tx) = (app.clone(), tx.clone());
        let token = token.clone();
        std::thread::spawn(move || server::run(server, port, &token, app, tx));
    }
    if !background {
        open(&format!("{url}/?t={token}"));
    }
    run(app, tx, rx);
}
