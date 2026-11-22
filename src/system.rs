use crate::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    disks: Vec<Disk>,
    devices: Vec<Device>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Disk {
    name: String,
    path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Device {
    name: String,
    device: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            disks: vec![Disk {
                name: "System".to_owned(),
                path: PathBuf::from("/"),
            }],
            devices: Vec::new(),
        }
    }
}

type Counters = HashMap<String, (u64, u64)>;

pub struct Collector {
    settings: Settings,
    cpu_previous: Counters,
    devices_previous: Option<(Instant, Counters)>,
    slow: Option<(Instant, Value)>,
}

impl Collector {
    pub fn new(settings: Settings) -> Result<Self> {
        if settings.disks.iter().any(|disk| !disk.path.is_absolute()) {
            return Err("System disk paths must be absolute".into());
        }
        if settings.devices.iter().any(|item| {
            item.device.is_empty()
                || item.device == "."
                || item.device == ".."
                || item.device.contains('/')
        }) {
            return Err("Use block device names such as sda or nvme0n1".into());
        }
        Ok(Self {
            settings,
            cpu_previous: HashMap::new(),
            devices_previous: None,
            slow: None,
        })
    }

    pub fn sample(&mut self) -> Result<Value> {
        let stat = fs::read_to_string("/proc/stat")?;
        let mut cores = Vec::new();
        let mut overall = None;
        for line in stat.lines().take_while(|line| line.starts_with("cpu")) {
            let mut fields = line.split_whitespace();
            let name = fields.next().unwrap_or_default();
            let counters: Vec<u64> = fields
                .take(8)
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            if counters.len() < 5 {
                continue;
            }
            let total: u64 = counters.iter().sum();
            let idle = counters[3] + counters[4];
            let percent = self
                .cpu_previous
                .insert(name.to_owned(), (total, idle))
                .and_then(|(total_before, idle_before)| {
                    let elapsed = total.checked_sub(total_before)?;
                    let idle = idle.saturating_sub(idle_before).min(elapsed);
                    (elapsed > 0)
                        .then(|| ((elapsed - idle) as f64 / elapsed as f64 * 1000.0).round() / 10.0)
                });
            if name == "cpu" {
                overall = percent;
            } else {
                cores.push(json!({"name": name, "percent": percent}));
            }
        }
        if self
            .slow
            .as_ref()
            .is_none_or(|(time, _)| time.elapsed() >= Duration::from_secs(10))
        {
            self.slow = Some((Instant::now(), self.slower_metrics()?));
        }
        let mut value = self.slow.as_ref().unwrap().1.clone();
        value["cpu"] = json!(overall);
        value["cores"] = json!(cores);
        let uptime = fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|line| line.split_whitespace().next()?.parse::<f64>().ok())
            .map(|seconds| seconds as u64);
        value["uptime"] = json!(uptime);
        value["drive_rates"] = json!(self.drive_rates());
        Ok(value)
    }

    fn slower_metrics(&self) -> Result<Value> {
        let info = fs::read_to_string("/proc/meminfo")?;
        let memory: HashMap<_, _> = info
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((
                    name,
                    value.split_whitespace().next()?.parse::<u64>().ok()? * 1024,
                ))
            })
            .collect();
        let total = memory.get("MemTotal").copied().unwrap_or(0);
        let available = memory.get("MemAvailable").copied().unwrap_or(0);
        let swap = memory.get("SwapTotal").copied().unwrap_or(0);
        let swap_free = memory.get("SwapFree").copied().unwrap_or(0);
        let disks: Vec<_> = self
            .settings
            .disks
            .iter()
            .map(|disk| match disk_usage(&disk.path) {
                Some((total, used, available)) => json!({
                    "name": disk.name, "total": total, "used": used, "free": available,
                }),
                None => json!({"name": disk.name}),
            })
            .collect();
        Ok(json!({
            "memory_used": total.saturating_sub(available),
            "memory_total": total,
            "swap_used": swap.saturating_sub(swap_free),
            "swap_total": swap,
            "disks": disks,
            "temperature": cpu_temperature(),
        }))
    }

    fn drive_rates(&mut self) -> Vec<Value> {
        let now = Instant::now();
        let mut current = HashMap::new();
        let mut rates = Vec::new();
        for item in &self.settings.devices {
            let counters = fs::read_to_string(format!("/sys/block/{}/stat", item.device))
                .ok()
                .and_then(|stat| {
                    let fields: Vec<_> = stat.split_whitespace().collect();
                    let read: u64 = fields.get(2)?.parse().ok()?;
                    let written: u64 = fields.get(6)?.parse().ok()?;
                    Some((read.saturating_mul(512), written.saturating_mul(512)))
                });
            let sample = counters.and_then(|(read, written)| {
                current.insert(item.device.clone(), (read, written));
                let (before, previous) = self.devices_previous.as_ref()?;
                let seconds = now.duration_since(*before).as_secs_f64().max(0.001);
                let (read_before, written_before) = previous.get(&item.device)?;
                Some((
                    (read.saturating_sub(*read_before) as f64 / seconds) as u64,
                    (written.saturating_sub(*written_before) as f64 / seconds) as u64,
                ))
            });
            rates.push(json!({
                "name": item.name,
                "read": sample.map(|(read, _)| read),
                "write": sample.map(|(_, write)| write),
            }));
        }
        self.devices_previous = Some((now, current));
        rates
    }
}

fn cpu_temperature() -> Option<f64> {
    fs::read_dir("/sys/class/hwmon")
        .ok()?
        .flatten()
        .find_map(|hw| {
            let path = hw.path();
            let name = fs::read_to_string(path.join("name")).ok()?;
            if !matches!(name.trim(), "k10temp" | "coretemp" | "zenpower") {
                return None;
            }
            let temperature = fs::read_to_string(path.join("temp1_input")).ok()?;
            temperature
                .trim()
                .parse::<f64>()
                .ok()
                .map(|millis| (millis / 100.0).round() / 10.0)
        })
}

fn disk_usage(path: &Path) -> Option<(u64, u64, u64)> {
    // A missing mount must not silently display its containing filesystem.
    let path = path.canonicalize().ok()?;
    let mountinfo = fs::read("/proc/self/mountinfo").ok()?;
    if !mountinfo.split(|&byte| byte == b'\n').any(|line| {
        line.split(|&byte| byte == b' ')
            .nth(4)
            .is_some_and(|field| decode_mount(field) == path.as_os_str().as_bytes())
    }) {
        return None;
    }
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut result = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: c_path is NUL-terminated and result has storage for statvfs to write.
    if unsafe { libc::statvfs(c_path.as_ptr(), result.as_mut_ptr()) } != 0 {
        return None;
    }
    let result = unsafe { result.assume_init() };
    let total = u64::from(result.f_blocks).saturating_mul(result.f_frsize as u64);
    let free = u64::from(result.f_bfree).saturating_mul(result.f_frsize as u64);
    let available = u64::from(result.f_bavail).saturating_mul(result.f_frsize as u64);
    Some((total, total.saturating_sub(free), available))
}

fn decode_mount(mut raw: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(raw.len());
    while let Some((&first, rest)) = raw.split_first() {
        if let [
            b'\\',
            a @ b'0'..=b'3',
            b @ b'0'..=b'7',
            c @ b'0'..=b'7',
            tail @ ..,
        ] = raw
        {
            decoded.push((a - b'0') * 64 + (b - b'0') * 8 + (c - b'0'));
            raw = tail;
        } else {
            decoded.push(first);
            raw = rest;
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_paths_decode_kernel_octal_escapes() {
        assert_eq!(
            decode_mount(br"/mnt/my\040disk\134new"),
            br"/mnt/my disk\new"
        );
        assert_eq!(decode_mount(br"\377"), [255]);
    }
}
