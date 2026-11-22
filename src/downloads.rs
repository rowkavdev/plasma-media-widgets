use crate::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    url: String,
    #[serde(default)]
    move_paths: Vec<PathMapping>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathMapping {
    client_root: PathBuf,
    local_root: PathBuf,
}

pub struct Client {
    settings: Settings,
    credentials: Option<(String, String)>,
    http: ureq::Agent,
}

impl Client {
    pub fn new(mut settings: Settings) -> Result<Self> {
        settings.url = settings.url.trim_end_matches('/').to_owned();
        let uri: ureq::http::Uri = settings.url.parse()?;
        if !matches!(uri.scheme_str(), Some("https" | "http"))
            || uri.authority().is_none_or(|a| a.as_str().contains('@'))
            || uri.query().is_some()
            || settings.url.contains('#')
        {
            return Err(
                "Use an HTTP(S) qBittorrent Web UI URL without credentials, a query or a fragment"
                    .into(),
            );
        }
        for mapping in &mut settings.move_paths {
            if !clean_absolute(&mapping.client_root) || !clean_absolute(&mapping.local_root) {
                return Err(
                    "Move path roots must be absolute and contain no parent components".into(),
                );
            }
            mapping.local_root = mapping.local_root.canonicalize()?;
            if !mapping.local_root.is_dir() {
                return Err("Each local move root must be a directory".into());
            }
        }
        // Prefer specific mappings when container mounts overlap.
        settings
            .move_paths
            .sort_by_key(|m| std::cmp::Reverse(m.client_root.components().count()));
        let credentials = match (env::var("QB_USERNAME").ok(), env::var("QB_PASSWORD").ok()) {
            (Some(username), Some(password)) => Some((username, password)),
            (None, None) => None,
            _ => {
                return Err(
                    "Set both QB_USERNAME and QB_PASSWORD for Web UI authentication".into(),
                );
            }
        };
        let http = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(4)))
            .max_redirects(0)
            .build()
            .into();
        Ok(Self {
            settings,
            credentials,
            http,
        })
    }

    fn request(&self) -> Result<Value> {
        let base = &self.settings.url;
        let url = format!("{base}/api/v2/sync/maindata?rid=0");
        let get = || self.http.get(&url).header("Referer", base).call();
        let mut response = match get() {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(401 | 403)) => {
                let (username, password) = self
                    .credentials
                    .as_ref()
                    .ok_or("Set QB_USERNAME and QB_PASSWORD for the qBittorrent Web UI")?;
                let mut login = self
                    .http
                    .post(format!("{base}/api/v2/auth/login"))
                    .header("Referer", base)
                    .send_form([("username", username), ("password", password)])
                    .map_err(|_| "qBittorrent Web UI login request failed")?;
                if login.body_mut().read_to_string()?.trim() != "Ok." {
                    return Err("qBittorrent Web UI rejected the login".into());
                }
                get().map_err(|_| "qBittorrent Web UI rejected the API request after login")?
            }
            Err(_) => return Err("Cannot connect to the qBittorrent Web UI".into()),
        };
        let data: Value = response.body_mut().read_json()?;
        if !data["server_state"].is_object() {
            return Err("qBittorrent did not return a download snapshot".into());
        }
        Ok(data)
    }

    pub fn sample(&self) -> Result<Value> {
        let data = self.request()?;
        Ok(summarize(&data, &self.settings.move_paths))
    }
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

fn count(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}

fn clean_absolute(path: &Path) -> bool {
    path.is_absolute() && path.components().all(|c| c != Component::ParentDir)
}

fn destination(torrent: &Value) -> Option<PathBuf> {
    let content = Path::new(torrent["content_path"].as_str()?);
    let save = Path::new(torrent["save_path"].as_str()?);
    let destination_root = if number(&torrent["progress"]) < 1.0 {
        torrent["download_path"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(Path::new)
            .unwrap_or(save)
    } else {
        save
    };
    if !clean_absolute(content) || !clean_absolute(destination_root) || content == destination_root
    {
        return None;
    }
    if content.starts_with(destination_root) {
        Some(content.to_path_buf())
    } else {
        Some(destination_root.join(content.file_name()?))
    }
}

fn allocated_size(path: &Path) -> Option<u64> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.is_file() {
        return Some(metadata.blocks().saturating_mul(512));
    }
    if !metadata.is_dir() {
        return None;
    }
    let mut bytes = 0u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.blocks().saturating_mul(512));
            }
        }
    }
    Some(bytes)
}

fn move_fraction(torrent: &Value, mappings: &[PathMapping]) -> Option<f64> {
    let destination = destination(torrent)?;
    let total = count(&torrent["size"]);
    if total == 0 {
        return None;
    }
    let (mapping, relative) = mappings.iter().find_map(|mapping| {
        let relative = destination.strip_prefix(&mapping.client_root).ok()?;
        (!relative.as_os_str().is_empty()).then_some((mapping, relative))
    })?;
    let path = mapping.local_root.join(relative);
    let resolved = path.canonicalize().ok()?;
    if resolved == mapping.local_root || !resolved.starts_with(&mapping.local_root) {
        return None;
    }
    Some(allocated_size(&resolved)?.min(total) as f64 / total as f64)
}

fn summarize(data: &Value, mappings: &[PathMapping]) -> Value {
    let mut all: Vec<_> = data["torrents"]
        .as_object()
        .into_iter()
        .flat_map(|torrents| torrents.values())
        .collect();
    let moving: Vec<_> = all
        .iter()
        .copied()
        .filter(|t| t["state"] == "moving")
        .collect();
    let mut incomplete: Vec<_> = all
        .iter()
        .copied()
        .filter(|t| number(&t["progress"]) < 1.0 && t["state"] != "moving")
        .collect();
    incomplete.sort_by(|a, b| {
        let eta = |t: &Value| match count(&t["eta"]) {
            1..=8_639_999 => count(&t["eta"]),
            _ => u64::MAX,
        };
        eta(a)
            .cmp(&eta(b))
            .then_with(|| number(&b["progress"]).total_cmp(&number(&a["progress"])))
    });
    let rows: Vec<_> = moving.iter().chain(&incomplete).map(|t| {
        let is_move = t["state"] == "moving";
        let progress = if is_move { move_fraction(t, mappings) } else { Some(number(&t["progress"])) };
        json!({
            "title": t["name"].as_str().unwrap_or("Untitled torrent"),
            "category": t["category"].as_str().filter(|v| !v.is_empty()).unwrap_or("Uncategorised"),
            "progress": progress,
            "down": count(&t["dlspeed"]),
            "eta": count(&t["eta"]),
            "seeds": count(&t["num_seeds"]),
            "state": t["state"].as_str().unwrap_or("unknown"),
        })
    }).collect();
    all.sort_by_key(|t| std::cmp::Reverse(count(&t["completion_on"])));
    let completed: Vec<_> = all
        .iter()
        .filter(|t| number(&t["progress"]) >= 1.0 && t["state"] != "moving")
        .take(4)
        .map(|t| json!({"title": t["name"], "size": t["size"], "ratio": number(&t["ratio"])}))
        .collect();
    let seeding = all
        .iter()
        .filter(|t| {
            number(&t["progress"]) >= 1.0
                && matches!(
                    t["state"].as_str(),
                    Some("uploading" | "stalledUP" | "forcedUP")
                )
        })
        .count();
    json!({
        "downloads": rows,
        "active": incomplete.iter().filter(|t| count(&t["dlspeed"]) > 0).count(),
        "incomplete": incomplete.len(),
        "moving": moving.len(),
        "completed": completed,
        "seeding": seeding,
        "total": all.len(),
        "stats": {
            "total_peer_connections": data["server_state"]["total_peer_connections"],
            "free_space_on_disk": data["server_state"]["free_space_on_disk"],
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_prefers_destination_directory() {
        let move_finished = json!({
            "content_path": "/downloads/incomplete/album.flac",
            "download_path": "/downloads/incomplete",
            "save_path": "/downloads/complete",
            "progress": 1.0,
        });
        assert_eq!(
            destination(&move_finished),
            Some(PathBuf::from("/downloads/complete/album.flac"))
        );
        let mut move_unfinished = move_finished;
        move_unfinished["progress"] = json!(0.4);
        move_unfinished["content_path"] = json!("/downloads/complete/album.flac");
        assert_eq!(
            destination(&move_unfinished),
            Some(PathBuf::from("/downloads/incomplete/album.flac"))
        );
        move_unfinished["download_path"] = json!("/downloads/../media");
        assert_eq!(destination(&move_unfinished), None);
    }

    #[test]
    fn moving_items_stay_visible_without_a_path_mapping() {
        let data = json!({
            "torrents": {
                "a": {"name": "Unknown ETA", "eta": 8640000, "progress": 0.9},
                "b": {"name": "Known ETA", "eta": 200, "progress": 0.4},
                "c": {"name": "Moving", "progress": 1.0, "state": "moving"},
            },
            "server_state": {},
        });
        let snapshot = summarize(&data, &[]);
        let rows = snapshot["downloads"].as_array().unwrap();
        assert_eq!(rows[0]["title"], "Moving");
        assert_eq!(rows[0]["progress"], Value::Null);
        assert_eq!(rows[1]["title"], "Known ETA");
        assert_eq!(rows[2]["title"], "Unknown ETA");
        assert_eq!(snapshot["completed"], json!([]));
    }
}
