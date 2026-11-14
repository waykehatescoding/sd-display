use crate::{App, Msg};
use std::io::{Read, Write};
use std::sync::mpsc::Sender;
use tiny_http::{Header, Method, Request, Response, Server};

const INDEX: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const APP_CSS: &str = include_str!("../web/app.css");
const FONTS: [(&str, &[u8]); 4] = [
    ("BarlowSemiCondensed-Regular.ttf", include_bytes!("../assets/fonts/BarlowSemiCondensed-Regular.ttf")),
    ("BarlowSemiCondensed-Medium.ttf", crate::gfx::FONT_MEDIUM),
    ("BarlowSemiCondensed-SemiBold.ttf", crate::gfx::FONT_SEMIBOLD),
    ("BarlowSemiCondensed-Bold.ttf", crate::gfx::FONT_BOLD),
];
const ICON: &[u8] = include_bytes!("../assets/icon.png");

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn get_header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn reply(req: Request, code: u16, body: Vec<u8>, ctype: &str) {
    let resp = Response::from_data(body)
        .with_status_code(code)
        .with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

fn json(req: Request, code: u16, body: serde_json::Value) {
    reply(req, code, body.to_string().into_bytes(), "application/json");
}

pub fn run(server: Server, port: u16, app: App, tx: Sender<Msg>) {
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    for mut req in server.incoming_requests() {
        let host_ok = get_header(&req, "Host").is_some_and(|h| allowed.iter().any(|a| a == h));
        let origin_ok = get_header(&req, "Origin")
            .map_or(true, |o| allowed.iter().any(|a| o.trim_start_matches("http://") == a));
        if !host_ok || !origin_ok {
            json(req, 403, serde_json::json!({ "error": "Forbidden" }));
            continue;
        }
        let path = req.url().split('?').next().unwrap_or("").to_string();

        if *req.method() == Method::Get {
            match path.as_str() {
                "/" => reply(req, 200, INDEX.into(), "text/html; charset=utf-8"),
                "/app.js" => reply(req, 200, APP_JS.into(), "text/javascript"),
                "/app.css" => reply(req, 200, APP_CSS.into(), "text/css"),
                "/icon.png" => reply(req, 200, ICON.to_vec(), "image/png"),
                "/events" => {
                    let mut w = req.into_writer();
                    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n";
                    let mut s = app.lock().unwrap();
                    let first = format!("{head}event: state\ndata: {}\n\n", s.state());
                    if w.write_all(first.as_bytes()).and_then(|_| w.flush()).is_ok() {
                        s.clients.push(w);
                        s.new_client = true;
                        let _ = tx.send(Msg::Wake);
                    }
                }
                p => match FONTS.iter().find(|(n, _)| p == format!("/fonts/{n}")) {
                    Some((_, bytes)) => reply(req, 200, bytes.to_vec(), "font/ttf"),
                    None => json(req, 404, serde_json::json!({ "error": "Not found" })),
                },
            }
            continue;
        }

        let is_json = get_header(&req, "Content-Type").is_some_and(|c| c.starts_with("application/json"));
        if !is_json {
            json(req, 415, serde_json::json!({ "error": "Expected JSON" }));
            continue;
        }
        let mut body = String::new();
        if req.as_reader().take(1_000_000).read_to_string(&mut body).is_err() {
            json(req, 400, serde_json::json!({ "error": "Bad body" }));
            continue;
        }
        let result = match (req.method(), path.as_str()) {
            (Method::Put, "/api/config") => serde_json::from_str::<crate::config::Config>(&body)
                .map_err(|e| e.to_string())
                .and_then(|cfg| app.lock().unwrap().set_config(cfg)),
            (Method::Post, "/api/page") => {
                let index = serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v["index"].as_i64()).unwrap_or(0);
                app.lock().unwrap().set_page(index);
                Ok(())
            }
            (Method::Post, "/api/quit") => {
                json(req, 200, serde_json::json!({ "ok": true }));
                let _ = tx.send(Msg::Quit);
                continue;
            }
            _ => Err("Not found".into()),
        };
        match result {
            Ok(()) => {
                let _ = tx.send(Msg::Wake);
                json(req, 200, serde_json::json!({ "ok": true }))
            }
            Err(e) => json(req, 400, serde_json::json!({ "error": e })),
        }
    }
}
