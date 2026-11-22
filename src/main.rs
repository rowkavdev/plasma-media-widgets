mod downloads;
mod system;

use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::error::Error;
use std::fs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server, StatusCode};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type Feed = Arc<Mutex<Value>>;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    port: Option<u16>,
    qbittorrent: Option<downloads::Settings>,
    system: system::Settings,
}

fn poll(interval: Duration, mut sample: impl FnMut() -> Result<Value> + Send + 'static) -> Feed {
    let feed = Arc::new(Mutex::new(json!({"message": "Waiting for first sample"})));
    let destination = Arc::clone(&feed);
    thread::spawn(move || {
        loop {
            let next = match sample() {
                Ok(data) => json!({"data": data}),
                Err(error) => json!({"message": error.to_string()}),
            };
            *destination.lock().unwrap() = next;
            thread::sleep(interval);
        }
    });
    feed
}

fn fixed(message: &str) -> Feed {
    Arc::new(Mutex::new(json!({"message": message})))
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let (config, demo) = match args.as_slice() {
        [] => (Config::default(), false),
        [arg] if arg == "--demo" => (Config::default(), true),
        [arg] if arg == "--help" || arg == "-h" => {
            println!("Usage: plasma-media-widgets [--demo | --config PATH]");
            return Ok(());
        }
        [arg, config] if arg == "--config" => {
            (serde_json::from_str(&fs::read_to_string(config)?)?, false)
        }
        _ => return Err("Usage: plasma-media-widgets [--demo | --config PATH]".into()),
    };
    let port = config.port.unwrap_or(18771);
    if port == 0 {
        return Err("Choose a port between 1 and 65535".into());
    }
    let server = Server::http(("127.0.0.1", port))?;
    let (downloads, system) = if demo {
        let fixture: Value = serde_json::from_str(include_str!("../examples/demo.json"))?;
        (
            Arc::new(Mutex::new(
                json!({"demo": true, "data": fixture["downloads"]}),
            )),
            Arc::new(Mutex::new(json!({"demo": true, "data": fixture["system"]}))),
        )
    } else {
        let downloads = if let Some(settings) = config.qbittorrent {
            let client = downloads::Client::new(settings)?;
            poll(Duration::from_secs(5), move || client.sample())
        } else {
            fixed("Add a qBittorrent URL to the backend configuration")
        };
        let mut system = system::Collector::new(config.system)?;
        let system = poll(Duration::from_secs(1), move || system.sample());
        (downloads, system)
    };
    eprintln!("Plasma Media Widgets listening on http://127.0.0.1:{port}");
    for request in server.incoming_requests() {
        // Qt QML XHR does not need permissive CORS. Accept the local URL only.
        let host = format!("127.0.0.1:{port}");
        let local = request
            .headers()
            .iter()
            .any(|header| header.field.equiv("Host") && header.value.as_str() == host);
        let (status, body) = match (local, request.method(), request.url()) {
            (false, _, _) => (400, json!({"message": "Use the loopback URL"})),
            (true, &Method::Get, "/downloads") => (200, downloads.lock().unwrap().clone()),
            (true, &Method::Get, "/system") => (200, system.lock().unwrap().clone()),
            _ => (404, json!({"message": "Route not found"})),
        };
        let response = Response::from_string(body.to_string())
            .with_status_code(StatusCode(status))
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
        let _ = request.respond(response);
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
