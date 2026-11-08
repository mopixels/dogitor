//! Manages the MediaMTX child process. Dogitor's recorder and snapshot code
//! read RTSP from MediaMTX (see `docs/SERVER.md`), so it's a hard dependency
//! for recording, not just for the phone app's live view — the GUI owns its
//! lifecycle so launching Dogitor.app is the only thing the user has to do.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::Result;

/// The repo's `mediamtx.yml`, installed into Application Support.
const TEMPLATE: &str = include_str!("../../../mediamtx.yml");
/// Marker for configs that send auth to Dogitor's hook. Older installs lack
/// it — they ran MediaMTX with no auth at all — and get upgraded.
const AUTH_MARKER: &str = "authMethod: http";
const AUTH_ADDRESS_KEY: &str = "authHTTPAddress:";
/// The one setting users are told to edit in this file; kept across upgrades.
const ADDITIONAL_HOSTS_KEY: &str = "webrtcAdditionalHosts:";

/// Install the app's `mediamtx.yml` if missing, upgrade a pre-auth one (the
/// old file is kept as `mediamtx.yml.bak`, and its `webrtcAdditionalHosts`
/// carried over), and point the auth hook at the API's current port. Writes
/// only when something changed.
pub fn sync_config(path: &Path, api_port: u16) -> Result<()> {
    let existing = std::fs::read_to_string(path).ok();
    let mut yml = match existing.as_deref() {
        Some(old) if old.contains(AUTH_MARKER) => old.to_string(),
        Some(old) => {
            std::fs::write(path.with_extension("yml.bak"), old)?;
            match find_key_line(old, ADDITIONAL_HOSTS_KEY) {
                Some(hosts) => replace_key_line(TEMPLATE, ADDITIONAL_HOSTS_KEY, hosts),
                None => TEMPLATE.to_string(),
            }
        }
        None => TEMPLATE.to_string(),
    };
    yml = replace_key_line(
        &yml,
        AUTH_ADDRESS_KEY,
        &format!("{AUTH_ADDRESS_KEY} http://127.0.0.1:{api_port}/internal/mediamtx-auth"),
    );
    if existing.as_deref() != Some(yml.as_str()) {
        std::fs::write(path, yml)?;
    }
    Ok(())
}

/// The first top-level (unindented, uncommented) line starting with `key`.
fn find_key_line<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find(|l| l.starts_with(key))
}

/// Replace every top-level line starting with `key` by `new_line`.
fn replace_key_line(text: &str, key: &str, new_line: &str) -> String {
    let mut out: String = text
        .lines()
        .map(|l| if l.starts_with(key) { new_line } else { l })
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub struct MediaMtxProcess {
    child: Option<Child>,
    pub error: Option<String>,
}

impl MediaMtxProcess {
    /// Spawn `mediamtx <yml_path>`. A failure to spawn (e.g. not installed) is
    /// captured in `error` rather than propagated, so the rest of the app can
    /// still start and show a banner instead of refusing to launch.
    pub fn spawn(yml_path: &Path, log_file: &std::fs::File) -> Self {
        let stdout = log_file
            .try_clone()
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        let stderr = log_file
            .try_clone()
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        match Command::new("mediamtx")
            .arg(yml_path)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
        {
            Ok(child) => Self {
                child: Some(child),
                error: None,
            },
            Err(e) => Self {
                child: None,
                error: Some(format!(
                    "couldn't start mediamtx: {e} (is it installed? `brew install mediamtx`)"
                )),
            },
        }
    }

    /// A stand-in with no child process, for UI previews in tests.
    #[cfg(test)]
    pub fn none() -> Self {
        Self {
            child: None,
            error: None,
        }
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for MediaMtxProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_UNAUTHENTICATED: &str = "logLevel: info\nrtsp: yes\nrtspAddress: :8554\n\
        # webrtcAdditionalHosts: [commented]\nwebrtcAdditionalHosts: [100.101.1.2]\n";

    fn temp_yml(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dogitor-mediamtx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("mediamtx.yml")
    }

    #[test]
    fn installs_template_with_api_port() {
        let path = temp_yml("fresh");
        sync_config(&path, 9100).unwrap();
        let yml = std::fs::read_to_string(&path).unwrap();
        assert!(yml.contains(AUTH_MARKER));
        assert!(yml.contains("authHTTPAddress: http://127.0.0.1:9100/internal/mediamtx-auth"));
        assert!(yml.contains("rtspAddress: 127.0.0.1:8554"));
    }

    #[test]
    fn upgrades_pre_auth_config_keeping_hosts() {
        let path = temp_yml("upgrade");
        std::fs::write(&path, OLD_UNAUTHENTICATED).unwrap();
        sync_config(&path, 8088).unwrap();

        let yml = std::fs::read_to_string(&path).unwrap();
        assert!(yml.contains(AUTH_MARKER));
        assert!(yml.contains("webrtcAdditionalHosts: [100.101.1.2]"));
        assert!(!yml.contains("webrtcAdditionalHosts: [100.64.0.1]"));
        let backup = std::fs::read_to_string(path.with_extension("yml.bak")).unwrap();
        assert_eq!(backup, OLD_UNAUTHENTICATED);
    }

    #[test]
    fn keeps_user_edits_and_follows_port_changes() {
        let path = temp_yml("edits");
        sync_config(&path, 8088).unwrap();
        let edited = std::fs::read_to_string(&path)
            .unwrap()
            .replace("logLevel: info", "logLevel: debug");
        std::fs::write(&path, &edited).unwrap();

        sync_config(&path, 8090).unwrap();
        let yml = std::fs::read_to_string(&path).unwrap();
        assert!(yml.contains("logLevel: debug"));
        assert!(yml.contains("127.0.0.1:8090/internal/mediamtx-auth"));
        assert!(!path.with_extension("yml.bak").exists());
    }
}
