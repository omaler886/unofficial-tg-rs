use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferMode {
    TdlibManaged,
    RawMtProtoAccelerated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferIntegration {
    pub mode: TransferMode,
    pub enable_accelerated_uploads: bool,
    pub enable_accelerated_downloads: bool,
    pub notes: Vec<String>,
}

impl Default for TransferIntegration {
    fn default() -> Self {
        Self {
            mode: TransferMode::RawMtProtoAccelerated,
            enable_accelerated_uploads: true,
            enable_accelerated_downloads: true,
            notes: vec![
                "Keep the Rust rewrite independent from official client code.".to_string(),
                "Treat accelerated transfer as a dedicated subsystem that can coexist with TDLib sessions."
                    .to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TdlibBootstrapConfig {
    pub api_id: i32,
    pub api_hash: String,
    pub database_dir: PathBuf,
    pub files_dir: PathBuf,
    pub device_model: String,
    pub system_version: String,
    pub app_version: String,
    pub use_test_dc: bool,
    pub custom_tdjson_path: Option<PathBuf>,
}

impl Default for TdlibBootstrapConfig {
    fn default() -> Self {
        Self {
            api_id: 0,
            api_hash: String::new(),
            database_dir: PathBuf::from("var/tdlib/db"),
            files_dir: PathBuf::from("var/tdlib/files"),
            device_model: "Unofficial TG RS".to_string(),
            system_version: std::env::consts::OS.to_string(),
            app_version: "0.1.0".to_string(),
            use_test_dc: false,
            custom_tdjson_path: None,
        }
    }
}

impl TdlibBootstrapConfig {
    pub fn library_candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(path) = &self.custom_tdjson_path {
            candidates.push(path.clone());
        }
        for name in default_tdjson_names() {
            candidates.push(PathBuf::from(name));
        }
        candidates
    }

    pub fn summary(&self) -> String {
        format!(
            "tdlib db={} files={} mode={}",
            self.database_dir.display(),
            self.files_dir.display(),
            if self.use_test_dc {
                "test"
            } else {
                "production"
            }
        )
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TdlibDiscoveryError {
    #[error("tdjson was not found in the configured candidate paths")]
    NotFound,
}

pub fn discover_tdjson(config: &TdlibBootstrapConfig) -> Result<PathBuf, TdlibDiscoveryError> {
    config
        .library_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(TdlibDiscoveryError::NotFound)
}

pub fn default_tdjson_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["tdjson.dll", "bin/tdjson.dll", "vendor/tdlib/tdjson.dll"]
    }
    #[cfg(target_os = "macos")]
    {
        &[
            "libtdjson.dylib",
            "bin/libtdjson.dylib",
            "vendor/tdlib/libtdjson.dylib",
        ]
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        &[
            "libtdjson.so",
            "bin/libtdjson.so",
            "vendor/tdlib/libtdjson.so",
        ]
    }
}
