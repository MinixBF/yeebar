//! Small settings files in %APPDATA%\yeebar: the device IP (so start-up does not depend on discovery)
//! and the "ambiance selon l'heure" option.

use std::net::IpAddr;
use std::path::PathBuf;

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(base).join("yeebar"))
}

pub fn load_ip() -> Option<IpAddr> {
    std::fs::read_to_string(dir()?.join("ip.txt"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// "Ambiance selon l'heure": ticked unless the user unticked it (file contains "0").
pub fn load_auto() -> bool {
    dir()
        .and_then(|d| std::fs::read_to_string(d.join("ambiance.txt")).ok())
        .is_none_or(|s| s.trim() != "0")
}

pub fn save_auto(on: bool) {
    if let Some(d) = dir() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join("ambiance.txt"), if on { "1" } else { "0" });
    }
}

pub fn save_ip(ip: IpAddr) {
    if let Some(d) = dir() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join("ip.txt"), ip.to_string());
    }
}
