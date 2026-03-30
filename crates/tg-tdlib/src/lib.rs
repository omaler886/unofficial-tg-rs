use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use libloading::Library;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TdlibBootstrapPreview {
    pub requests: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TdlibTransferPreview {
    pub download_file: Value,
    pub add_to_downloads: Value,
    pub preliminary_upload_file: Value,
    pub send_document_message: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TdlibProbe {
    pub library_path: PathBuf,
    pub service_request: Value,
    pub service_response: Option<Value>,
    pub auth_request: Value,
    pub auth_messages: Vec<Value>,
}

#[derive(Debug, Error)]
pub enum TdlibRuntimeError {
    #[error("tdjson was not found in the configured candidate paths")]
    NotFound,
    #[error("failed to load tdjson from {path}: {message}")]
    Load { path: PathBuf, message: String },
    #[error("tdjson is missing required symbol {symbol}")]
    MissingSymbol { symbol: &'static str },
    #[error("request contained an interior NUL byte")]
    InvalidCString,
    #[error("tdjson returned invalid UTF-8")]
    InvalidUtf8,
    #[error("tdjson returned invalid JSON: {0}")]
    InvalidJson(String),
}

type TdJsonClientCreateFn = unsafe extern "C" fn() -> *mut c_void;
type TdJsonClientSendFn = unsafe extern "C" fn(client: *mut c_void, request: *const c_char);
type TdJsonClientReceiveFn =
    unsafe extern "C" fn(client: *mut c_void, timeout: f64) -> *const c_char;
type TdJsonClientExecuteFn =
    unsafe extern "C" fn(client: *mut c_void, request: *const c_char) -> *const c_char;
type TdJsonClientDestroyFn = unsafe extern "C" fn(client: *mut c_void);

pub struct TdjsonApi {
    _library: Library,
    create_client: TdJsonClientCreateFn,
    send: TdJsonClientSendFn,
    receive: TdJsonClientReceiveFn,
    execute: TdJsonClientExecuteFn,
    destroy: TdJsonClientDestroyFn,
    path: PathBuf,
}

impl TdjsonApi {
    pub fn load(path: impl AsRef<Path>) -> Result<Arc<Self>, TdlibRuntimeError> {
        let path = path.as_ref().to_path_buf();
        let library = unsafe { Library::new(&path) }.map_err(|error| TdlibRuntimeError::Load {
            path: path.clone(),
            message: error.to_string(),
        })?;

        let create_client =
            load_symbol::<TdJsonClientCreateFn>(&library, b"td_json_client_create\0")?;
        let send = load_symbol::<TdJsonClientSendFn>(&library, b"td_json_client_send\0")?;
        let receive = load_symbol::<TdJsonClientReceiveFn>(&library, b"td_json_client_receive\0")?;
        let execute = load_symbol::<TdJsonClientExecuteFn>(&library, b"td_json_client_execute\0")?;
        let destroy = load_symbol::<TdJsonClientDestroyFn>(&library, b"td_json_client_destroy\0")?;

        Ok(Arc::new(Self {
            _library: library,
            create_client,
            send,
            receive,
            execute,
            destroy,
            path,
        }))
    }

    pub fn create_client(self: &Arc<Self>) -> TdjsonClient {
        let raw = unsafe { (self.create_client)() };
        TdjsonClient {
            api: Arc::clone(self),
            raw,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct TdjsonClient {
    api: Arc<TdjsonApi>,
    raw: *mut c_void,
}

impl TdjsonClient {
    pub fn execute(&self, request: &Value) -> Result<Option<Value>, TdlibRuntimeError> {
        let request =
            CString::new(request.to_string()).map_err(|_| TdlibRuntimeError::InvalidCString)?;
        let response = unsafe { (self.api.execute)(self.raw, request.as_ptr()) };
        parse_json_ptr(response)
    }

    pub fn send(&self, request: &Value) -> Result<(), TdlibRuntimeError> {
        let request =
            CString::new(request.to_string()).map_err(|_| TdlibRuntimeError::InvalidCString)?;
        unsafe {
            (self.api.send)(self.raw, request.as_ptr());
        }
        Ok(())
    }

    pub fn receive(&self, timeout: Duration) -> Result<Option<Value>, TdlibRuntimeError> {
        let response = unsafe { (self.api.receive)(self.raw, timeout.as_secs_f64()) };
        parse_json_ptr(response)
    }

    pub fn receive_batch(
        &self,
        timeout: Duration,
        max_messages: usize,
    ) -> Result<Vec<Value>, TdlibRuntimeError> {
        let deadline = Instant::now() + timeout;
        let mut messages = Vec::new();
        while Instant::now() < deadline && messages.len() < max_messages {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = remaining.min(Duration::from_millis(200));
            if let Some(message) = self.receive(wait)? {
                messages.push(message);
            }
        }
        Ok(messages)
    }
}

impl Drop for TdjsonClient {
    fn drop(&mut self) {
        unsafe {
            (self.api.destroy)(self.raw);
        }
    }
}

pub fn discover_tdjson(config: &TdlibBootstrapConfig) -> Result<PathBuf, TdlibRuntimeError> {
    config
        .library_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(TdlibRuntimeError::NotFound)
}

pub fn probe_tdjson(config: &TdlibBootstrapConfig) -> Result<TdlibProbe, TdlibRuntimeError> {
    let path = discover_tdjson(config)?;
    let api = TdjsonApi::load(&path)?;
    let client = api.create_client();

    let service_request = tdlib_requests::sample_text_entities();
    let service_response = client.execute(&service_request)?;

    let auth_request = tdlib_requests::get_authorization_state("probe-auth-state");
    client.send(&auth_request)?;
    let auth_messages = client.receive_batch(Duration::from_secs(1), 6)?;

    Ok(TdlibProbe {
        library_path: api.path().to_path_buf(),
        service_request,
        service_response,
        auth_request,
        auth_messages,
    })
}

pub fn bootstrap_preview(config: &TdlibBootstrapConfig) -> TdlibBootstrapPreview {
    TdlibBootstrapPreview {
        requests: vec![
            tdlib_requests::set_log_verbosity_level(1),
            tdlib_requests::set_tdlib_parameters(config),
            tdlib_requests::get_authorization_state("bootstrap-state"),
        ],
    }
}

pub fn transfer_preview(
    local_path: impl Into<String>,
    chat_id: i64,
    file_id: i32,
) -> TdlibTransferPreview {
    let local_path = local_path.into();
    TdlibTransferPreview {
        download_file: tdlib_requests::download_file(file_id, 16, 0, 0, false),
        add_to_downloads: tdlib_requests::add_file_to_downloads(file_id, chat_id, 0, 16),
        preliminary_upload_file: tdlib_requests::preliminary_upload_file(&local_path, 16),
        send_document_message: tdlib_requests::send_document_message(chat_id, &local_path),
    }
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

fn load_symbol<T>(library: &Library, symbol: &'static [u8]) -> Result<T, TdlibRuntimeError>
where
    T: Copy,
{
    let loaded =
        unsafe { library.get::<T>(symbol) }.map_err(|_| TdlibRuntimeError::MissingSymbol {
            symbol: symbol_name(symbol),
        })?;
    Ok(*loaded)
}

fn symbol_name(symbol: &'static [u8]) -> &'static str {
    let trimmed = &symbol[..symbol.len().saturating_sub(1)];
    std::str::from_utf8(trimmed).unwrap_or("unknown")
}

fn parse_json_ptr(ptr: *const c_char) -> Result<Option<Value>, TdlibRuntimeError> {
    if ptr.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| TdlibRuntimeError::InvalidUtf8)?;
    serde_json::from_str(value)
        .map(Some)
        .map_err(|error| TdlibRuntimeError::InvalidJson(error.to_string()))
}

pub mod tdlib_requests {
    use serde_json::{Value, json};

    use crate::TdlibBootstrapConfig;

    pub fn sample_text_entities() -> Value {
        json!({
            "@type": "getTextEntities",
            "text": "@telegram https://telegram.org"
        })
    }

    pub fn set_log_verbosity_level(level: i32) -> Value {
        json!({
            "@type": "setLogVerbosityLevel",
            "new_verbosity_level": level
        })
    }

    pub fn get_authorization_state(extra: &str) -> Value {
        json!({
            "@type": "getAuthorizationState",
            "@extra": extra
        })
    }

    pub fn set_tdlib_parameters(config: &TdlibBootstrapConfig) -> Value {
        json!({
            "@type": "setTdlibParameters",
            "use_test_dc": config.use_test_dc,
            "database_directory": config.database_dir,
            "files_directory": config.files_dir,
            "database_encryption_key": "",
            "use_file_database": true,
            "use_chat_info_database": true,
            "use_message_database": true,
            "use_secret_chats": true,
            "api_id": config.api_id,
            "api_hash": config.api_hash,
            "system_language_code": "en",
            "device_model": config.device_model,
            "system_version": config.system_version,
            "application_version": config.app_version
        })
    }

    pub fn download_file(
        file_id: i32,
        priority: i32,
        offset: i64,
        limit: i64,
        synchronous: bool,
    ) -> Value {
        json!({
            "@type": "downloadFile",
            "file_id": file_id,
            "priority": priority,
            "offset": offset,
            "limit": limit,
            "synchronous": synchronous
        })
    }

    pub fn add_file_to_downloads(
        file_id: i32,
        chat_id: i64,
        message_id: i64,
        priority: i32,
    ) -> Value {
        json!({
            "@type": "addFileToDownloads",
            "file_id": file_id,
            "chat_id": chat_id,
            "message_id": message_id,
            "priority": priority
        })
    }

    pub fn preliminary_upload_file(local_path: &str, priority: i32) -> Value {
        json!({
            "@type": "preliminaryUploadFile",
            "file": {
                "@type": "inputFileLocal",
                "path": local_path
            },
            "file_type": Value::Null,
            "priority": priority
        })
    }

    pub fn send_document_message(chat_id: i64, local_path: &str) -> Value {
        json!({
            "@type": "sendMessage",
            "chat_id": chat_id,
            "message_thread_id": 0,
            "reply_to": Value::Null,
            "options": Value::Null,
            "reply_markup": Value::Null,
            "input_message_content": {
                "@type": "inputMessageDocument",
                "document": {
                    "@type": "inputFileLocal",
                    "path": local_path
                },
                "thumbnail": Value::Null,
                "disable_content_type_detection": false,
                "caption": {
                    "@type": "formattedText",
                    "text": "",
                    "entities": []
                }
            }
        })
    }
}
