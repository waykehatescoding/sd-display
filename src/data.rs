use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Performance::*;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

pub const HISTORY: usize = 60;

#[derive(Default)]
pub struct Series {
    pub history: VecDeque<f64>,
    pub seq: u64,
}

impl Series {
    fn push(&mut self, v: f64) {
        self.seq += 1;
        self.history.push_back(v);
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }
    pub fn max(&self) -> f64 {
        self.history.iter().cloned().fold(0.0, f64::max)
    }
}

#[derive(Clone)]
pub struct Day {
    pub weekday: usize,
    pub min: f64,
    pub max: f64,
    pub icon: String,
}

#[derive(Default)]
pub struct WeatherState {
    pub error: Option<String>,
    pub current: Option<Value>,
    pub forecast: Option<Vec<Day>>,
    pub updated: u64,
    pub fetching: bool,
}

#[derive(Default)]
pub struct Data {
    pub cpu: Series,
    pub cpu_load: f64,
    pub mem_used: u64,
    pub mem_total: u64,
    pub gpu: Series,
    pub gpu_load: Option<f64>,
    pub vram: f64,
    pub net: Series,
    pub down: f64,
    pub up: f64,
    pub disks: HashMap<String, Option<(u64, u64)>>,
    pub weather: Arc<Mutex<WeatherState>>,
}

struct Pdh {
    query: PDH_HQUERY,
    load: PDH_HCOUNTER,
    vram: PDH_HCOUNTER,
}

impl Drop for Pdh {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query) };
    }
}

pub struct Sources {
    pub data: Data,
    last: HashMap<&'static str, Instant>,
    cpu_prev: Option<(u64, u64)>,
    net_prev: Option<(u64, u64, Instant)>,
    pdh: Option<Pdh>,
    weather_cfg: crate::config::Weather,
    wake: Sender<crate::Msg>,
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Sources {
    pub fn new(wake: Sender<crate::Msg>) -> Self {
        Sources {
            data: Data::default(),
            last: HashMap::new(),
            cpu_prev: None,
            net_prev: None,
            pdh: None,
            weather_cfg: Default::default(),
            wake,
        }
    }

    fn due(&mut self, name: &'static str, every: Duration) -> bool {
        let now = Instant::now();
        match self.last.get(name) {
            Some(t) if now.duration_since(*t) < every => false,
            _ => {
                self.last.insert(name, now);
                true
            }
        }
    }

    pub fn set_weather(&mut self, w: &crate::config::Weather) {
        if *w != self.weather_cfg {
            self.weather_cfg = w.clone();
            self.last.remove("weather");
        }
    }

    pub fn refresh_weather(&mut self) {
        self.last.remove("weather");
    }

    pub fn tick(&mut self, need: &HashSet<String>, drives: &HashSet<String>) {
        let has = |k: &str| need.contains(k);
        if has("cpu") && self.due("cpu", Duration::from_millis(1500)) {
            self.sample_cpu();
        }
        if has("memory") && self.due("memory", Duration::from_secs(3)) {
            self.sample_memory();
        }
        if has("disk") && self.due("disk", Duration::from_secs(60)) {
            self.sample_disks(drives);
        }
        if has("network") {
            if self.due("network", Duration::from_secs(2)) {
                self.sample_network();
            }
        } else {
            self.net_prev = None;
        }
        if has("gpu") {
            if self.due("gpu", Duration::from_secs(2)) {
                self.sample_gpu();
            }
        } else {
            self.pdh = None;
        }
        if (has("weather") || has("forecast")) && self.due("weather", Duration::from_secs(600)) {
            self.fetch_weather();
        }
        if !has("cpu") {
            self.cpu_prev = None;
        }
    }

    fn sample_cpu(&mut self) {
        let (mut idle, mut kernel, mut user) = unsafe { std::mem::zeroed::<(FILETIME, FILETIME, FILETIME)>() };
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return;
        }
        let (idle, total) = (ft(idle), ft(kernel) + ft(user));
        if let Some((pi, pt)) = self.cpu_prev {
            let dt = total.saturating_sub(pt);
            if dt > 0 {
                let load = 100.0 * (1.0 - idle.saturating_sub(pi) as f64 / dt as f64);
                self.data.cpu_load = load.clamp(0.0, 100.0);
                self.data.cpu.push(self.data.cpu_load);
            }
        }
        self.cpu_prev = Some((idle, total));
    }

    fn sample_memory(&mut self) {
        let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut m) } != 0 {
            self.data.mem_total = m.ullTotalPhys;
            self.data.mem_used = m.ullTotalPhys - m.ullAvailPhys;
        }
    }

    fn sample_disks(&mut self, drives: &HashSet<String>) {
        for d in drives {
            let path = wide(&format!("{}\\", d.trim_end_matches('\\')));
            let (mut free, mut total, mut _all_free) = (0u64, 0u64, 0u64);
            let ok = unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, &mut _all_free) } != 0;
            self.data.disks.insert(d.clone(), ok.then_some((total, free)));
        }
    }

    fn sample_network(&mut self) {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if unsafe { GetIfTable2(&mut table) } != 0 || table.is_null() {
            return;
        }
        let (mut rx, mut tx) = (0u64, 0u64);
        unsafe {
            let n = (*table).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
            for r in rows {
                let flags = r.InterfaceAndOperStatusFlags._bitfield;
                let hardware = flags & 1 != 0;
                let filter = flags & 2 != 0;
                if hardware && !filter && r.OperStatus == 1 {
                    rx += r.InOctets;
                    tx += r.OutOctets;
                }
            }
            FreeMibTable(table as *const _);
        }
        let now = Instant::now();
        if let Some((prx, ptx, pt)) = self.net_prev {
            let s = now.duration_since(pt).as_secs_f64().max(0.1);
            self.data.down = rx.saturating_sub(prx) as f64 / s;
            self.data.up = tx.saturating_sub(ptx) as f64 / s;
            self.data.net.push(self.data.down + self.data.up);
        }
        self.net_prev = Some((rx, tx, now));
    }

    fn sample_gpu(&mut self) {
        if self.pdh.is_none() {
            unsafe {
                let mut q: PDH_HQUERY = std::mem::zeroed();
                if PdhOpenQueryW(std::ptr::null(), 0, &mut q) != 0 {
                    return;
                }
                let mut load: PDH_HCOUNTER = std::mem::zeroed();
                let mut vram: PDH_HCOUNTER = std::mem::zeroed();
                PdhAddEnglishCounterW(q, wide("\\GPU Engine(*engtype_3D)\\Utilization Percentage").as_ptr(), 0, &mut load);
                PdhAddEnglishCounterW(q, wide("\\GPU Adapter Memory(*)\\Dedicated Usage").as_ptr(), 0, &mut vram);
                PdhCollectQueryData(q);
                self.pdh = Some(Pdh { query: q, load, vram });
            }
            return;
        }
        let pdh = self.pdh.as_ref().unwrap();
        if unsafe { PdhCollectQueryData(pdh.query) } != 0 {
            return;
        }
        let load = sum_counter(pdh.load);
        let vram = sum_counter(pdh.vram);
        if let Some(load) = load {
            let load = load.min(100.0);
            self.data.gpu_load = Some(load);
            self.data.gpu.push(load);
        }
        self.data.vram = vram.unwrap_or(0.0);
    }

    fn fetch_weather(&mut self) {
        let w = self.weather_cfg.clone();
        let state = self.data.weather.clone();
        let wake = self.wake.clone();
        {
            let mut s = state.lock().unwrap();
            if s.fetching {
                return;
            }
            s.fetching = true;
        }
        std::thread::spawn(move || {
            let result = fetch(&w);
            let mut s = state.lock().unwrap();
            s.fetching = false;
            match result {
                Ok((cur, fc)) => {
                    s.current = Some(cur);
                    s.forecast = fc;
                    s.error = None;
                }
                Err(e) => {
                    if s.current.is_none() || e != "Offline" {
                        s.error = Some(e);
                    }
                }
            }
            s.updated += 1;
            drop(s);
            let _ = wake.send(crate::Msg::Wake);
        });
    }
}

fn sum_counter(c: PDH_HCOUNTER) -> Option<f64> {
    unsafe {
        let (mut size, mut count) = (0u32, 0u32);
        let r = PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut());
        if r != PDH_MORE_DATA as u32 || size == 0 {
            return if count == 0 && r == 0 { Some(0.0) } else { None };
        }
        let mut buf = vec![0u8; size as usize];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, items) != 0 {
            return None;
        }
        let items = std::slice::from_raw_parts(items, count as usize);
        Some(items.iter().filter(|i| i.FmtValue.CStatus == 0).map(|i| i.FmtValue.Anonymous.doubleValue).sum())
    }
}

fn fetch(w: &crate::config::Weather) -> Result<(Value, Option<Vec<Day>>), String> {
    if w.api_key.is_empty() {
        return Err("Add an API key".into());
    }
    let units = if w.units == "imperial" { "imperial" } else { "metric" };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let get = |endpoint: &str| -> Result<Value, String> {
        agent
            .get(&format!("https://api.openweathermap.org/data/2.5/{endpoint}"))
            .query("q", &w.city)
            .query("units", units)
            .query("appid", &w.api_key)
            .call()
            .map_err(|e| match e {
                ureq::Error::StatusCode(401) => "API key rejected".to_string(),
                ureq::Error::StatusCode(404) => "City not found".to_string(),
                _ => "Offline".to_string(),
            })?
            .body_mut()
            .read_json::<Value>()
            .map_err(|_| "Offline".to_string())
    };
    let cur = get("weather")?;
    let fc = get("forecast").ok().map(|f| daily(&f));
    Ok((cur, fc))
}

pub fn daily(fc: &Value) -> Vec<Day> {
    let tz = fc["city"]["timezone"].as_i64().unwrap_or(0);
    let mut days: Vec<(i64, Day, i64)> = Vec::new();
    for item in fc["list"].as_array().into_iter().flatten() {
        let t = item["dt"].as_i64().unwrap_or(0) + tz;
        let day = t.div_euclid(86400);
        let hour = t.rem_euclid(86400) / 3600;
        let min = item["main"]["temp_min"].as_f64().unwrap_or(0.0);
        let max = item["main"]["temp_max"].as_f64().unwrap_or(0.0);
        let icon = item["weather"][0]["icon"].as_str().unwrap_or("03d").replace('n', "d");
        let dist = (hour - 13).abs();
        match days.iter_mut().find(|d| d.0 == day) {
            Some((_, d, best)) => {
                d.min = d.min.min(min);
                d.max = d.max.max(max);
                if dist < *best {
                    d.icon = icon;
                    *best = dist;
                }
            }
            None => days.push((day, Day { weekday: (day + 4).rem_euclid(7) as usize, min, max, icon }, dist)),
        }
    }
    days.into_iter().take(5).map(|d| d.1).collect()
}
