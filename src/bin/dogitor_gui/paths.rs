//! Filesystem locations for the GUI build. A Finder-launched `.app` has no
//! reliable working directory, so (unlike the headless CLI's cwd-relative
//! `config.toml`) everything lives under a fixed per-user directory.

use std::path::PathBuf;

pub fn app_support_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    PathBuf::from(home).join("Library/Application Support/Dogitor")
}

pub fn config_path() -> PathBuf {
    app_support_dir().join("config.toml")
}

pub fn mediamtx_yml_path() -> PathBuf {
    app_support_dir().join("mediamtx.yml")
}

pub fn log_path() -> PathBuf {
    app_support_dir().join("dogitor.log")
}

pub fn env_path() -> PathBuf {
    app_support_dir().join(".env")
}
