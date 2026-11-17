//! Self-update from the GitHub releases of this repository.
//!
//! The release workflow publishes `yeebar.exe` and `yeebar.exe.sha256`. Updating downloads both
//! over HTTPS, checks the SHA-256, then swaps the files next to the running exe (Windows lets a
//! running exe be renamed, not overwritten): `yeebar.exe` → `yeebar.old.exe`, new → `yeebar.exe`.
//! The leftover `yeebar.old.exe` is removed at the next start.
//! Network calls go through PowerShell to avoid bundling an HTTP/TLS stack in a small exe.

pub const REPO: &str = "MinixBF/yeebar";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// `latest` (a tag like "v0.3.0") is strictly newer than `current` ("0.2.0").
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(a), Some(b)) if a > b)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    fn quote(p: &Path) -> String {
        format!("'{}'", p.display().to_string().replace('\'', "''"))
    }

    fn ps(script: &str) -> Result<String, String> {
        let full = format!(
            "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; \
             [Net.ServicePointManager]::SecurityProtocol='Tls12'; {script}"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &full])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("PowerShell introuvable : {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            if err.contains("Not Found") || err.contains("404") {
                return Err("aucune version publiée trouvée sur GitHub".into());
            }
            if err.contains("resolve") || err.contains("connect") {
                return Err("GitHub injoignable (pas d'Internet ?)".into());
            }
            Err(err
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("erreur inconnue")
                .trim()
                .to_string())
        }
    }

    /// Tag of the newest release, if it is newer than this exe.
    pub fn check() -> Result<Option<String>, String> {
        let tag = ps(&format!(
            "(Invoke-RestMethod -UseBasicParsing -Headers @{{'User-Agent'='yeebar'}} \
             https://api.github.com/repos/{REPO}/releases/latest).tag_name"
        ))?;
        Ok(is_newer(&tag, VERSION).then_some(tag))
    }

    /// Download `tag`, verify it and put it in place of the running exe. Returns the exe path.
    pub fn install(tag: &str) -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe
            .parent()
            .ok_or("dossier de l'exe introuvable")?
            .to_path_buf();
        let new = dir.join("yeebar.new.exe");
        let sum = dir.join("yeebar.new.sha256");
        let old = dir.join("yeebar.old.exe");
        let base = format!("https://github.com/{REPO}/releases/download/{tag}/");
        let res = ps(&format!(
            "Invoke-WebRequest -UseBasicParsing '{base}yeebar.exe' -OutFile {new}; \
             Invoke-WebRequest -UseBasicParsing '{base}yeebar.exe.sha256' -OutFile {sum}; \
             $want = ((Get-Content {sum} -Raw).Trim() -split '\\s+')[0]; \
             $got = (Get-FileHash {new} -Algorithm SHA256).Hash; \
             if ($want -ne $got) {{ throw 'empreinte SHA-256 differente, mise a jour annulee' }}",
            new = quote(&new),
            sum = quote(&sum),
        ));
        let _ = std::fs::remove_file(&sum);
        if let Err(e) = res {
            let _ = std::fs::remove_file(&new);
            return Err(e);
        }
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old)
            .map_err(|e| format!("remplacement impossible ({e}) — dossier protégé ?"))?;
        if let Err(e) = std::fs::rename(&new, &exe) {
            let _ = std::fs::rename(&old, &exe);
            return Err(format!("remplacement impossible : {e}"));
        }
        Ok(exe)
    }

    /// Start `exe` again in two seconds, once this instance has quit and released its mutex.
    pub fn restart_later(exe: &Path) {
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("Start-Sleep 2; Start-Process {}", quote(exe)),
            ])
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn();
    }

    /// Remove what a previous update left behind.
    pub fn cleanup() {
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf))
        {
            for f in ["yeebar.old.exe", "yeebar.new.exe", "yeebar.new.sha256"] {
                let _ = std::fs::remove_file(dir.join(f));
            }
        }
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("v1.0.0", "0.99.0"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.9", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
        assert!(!is_newer("v0.3", "0.2.0"));
    }
}
