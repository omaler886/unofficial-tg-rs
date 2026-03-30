use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use std::{fmt, mem};

use libloading::Library;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tg_core::{TransferDirection, TransferPlan};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationState {
    WaitTdlibParameters,
    WaitEncryptionKey,
    WaitPhoneNumber,
    WaitCode,
    WaitPassword,
    WaitRegistration,
    WaitOtherDeviceConfirmation,
    Ready,
    LoggingOut,
    Closing,
    Closed,
    Unknown(String),
}

impl AuthorizationState {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl fmt::Display for AuthorizationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WaitTdlibParameters => write!(f, "authorizationStateWaitTdlibParameters"),
            Self::WaitEncryptionKey => write!(f, "authorizationStateWaitEncryptionKey"),
            Self::WaitPhoneNumber => write!(f, "authorizationStateWaitPhoneNumber"),
            Self::WaitCode => write!(f, "authorizationStateWaitCode"),
            Self::WaitPassword => write!(f, "authorizationStateWaitPassword"),
            Self::WaitRegistration => write!(f, "authorizationStateWaitRegistration"),
            Self::WaitOtherDeviceConfirmation => {
                write!(f, "authorizationStateWaitOtherDeviceConfirmation")
            }
            Self::Ready => write!(f, "authorizationStateReady"),
            Self::LoggingOut => write!(f, "authorizationStateLoggingOut"),
            Self::Closing => write!(f, "authorizationStateClosing"),
            Self::Closed => write!(f, "authorizationStateClosed"),
            Self::Unknown(value) => write!(f, "{value}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTdlibBridgeResult {
    pub library_path: PathBuf,
    pub authorization_state: AuthorizationState,
    pub priority: i32,
    pub plan: TransferPlan,
    pub primary_request: Value,
    pub primary_response: Option<Value>,
    pub follow_up_requests: Vec<Value>,
    pub follow_up_responses: Vec<Option<Value>>,
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
    #[error("tdlib request must be a JSON object")]
    NonObjectRequest,
    #[error("timed out waiting for TDLib response with extra {extra}")]
    ResponseTimeout { extra: String },
    #[error("tdlib session is not authorized: {state}")]
    AuthNotReady { state: AuthorizationState },
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

pub struct TdjsonSession {
    client: TdjsonClient,
    backlog: Vec<Value>,
    request_seq: u64,
    library_path: PathBuf,
}

impl TdjsonSession {
    pub fn connect(config: &TdlibBootstrapConfig) -> Result<Self, TdlibRuntimeError> {
        let path = discover_tdjson(config)?;
        let api = TdjsonApi::load(&path)?;
        let client = api.create_client();
        Ok(Self {
            client,
            backlog: Vec::new(),
            request_seq: 0,
            library_path: path,
        })
    }

    pub fn library_path(&self) -> &Path {
        &self.library_path
    }

    pub fn authorization_state(&mut self) -> Result<AuthorizationState, TdlibRuntimeError> {
        let request = tdlib_requests::get_authorization_state("authorization-state");
        let response = self.request(request, Duration::from_secs(2))?;
        Ok(authorization_state_from_value(
            response.as_ref().unwrap_or(&Value::Null),
        ))
    }

    pub fn request(
        &mut self,
        request: Value,
        timeout: Duration,
    ) -> Result<Option<Value>, TdlibRuntimeError> {
        let (extra, tagged) = self.tag_request(request)?;
        self.client.send(&tagged)?;
        self.wait_for_extra(&extra, timeout)
    }

    pub fn poll_updates(
        &mut self,
        timeout: Duration,
        max_messages: usize,
    ) -> Result<Vec<Value>, TdlibRuntimeError> {
        let mut updates = mem::take(&mut self.backlog);
        let mut fresh = self.client.receive_batch(timeout, max_messages)?;
        updates.append(&mut fresh);
        Ok(updates)
    }

    pub fn bridge_download(
        &mut self,
        file_id: i32,
        chat_id: i64,
        message_id: i64,
        plan: TransferPlan,
    ) -> Result<LiveTdlibBridgeResult, TdlibRuntimeError> {
        if plan.direction != TransferDirection::Download {
            return Err(TdlibRuntimeError::NonObjectRequest);
        }
        let authorization_state = self.authorization_state()?;
        if !authorization_state.is_ready() {
            return Err(TdlibRuntimeError::AuthNotReady {
                state: authorization_state,
            });
        }

        let priority = tdlib_priority_from_plan(&plan);
        let primary_request = tdlib_requests::download_file(file_id, priority, 0, 0, false);
        let primary_response = self.request(primary_request.clone(), Duration::from_secs(3))?;

        let follow_up_requests = vec![tdlib_requests::add_file_to_downloads(
            file_id, chat_id, message_id, priority,
        )];
        let follow_up_responses = follow_up_requests
            .iter()
            .cloned()
            .map(|request| self.request(request, Duration::from_secs(3)))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(LiveTdlibBridgeResult {
            library_path: self.library_path.clone(),
            authorization_state,
            priority,
            plan,
            primary_request,
            primary_response,
            follow_up_requests,
            follow_up_responses,
        })
    }

    pub fn bridge_upload(
        &mut self,
        local_path: &str,
        chat_id: i64,
        plan: TransferPlan,
    ) -> Result<LiveTdlibBridgeResult, TdlibRuntimeError> {
        if plan.direction != TransferDirection::Upload {
            return Err(TdlibRuntimeError::NonObjectRequest);
        }
        let authorization_state = self.authorization_state()?;
        if !authorization_state.is_ready() {
            return Err(TdlibRuntimeError::AuthNotReady {
                state: authorization_state,
            });
        }

        let priority = tdlib_priority_from_plan(&plan);
        let primary_request = tdlib_requests::preliminary_upload_file(local_path, priority);
        let primary_response = self.request(primary_request.clone(), Duration::from_secs(3))?;

        let follow_up_requests = vec![tdlib_requests::send_document_message(chat_id, local_path)];
        let follow_up_responses = follow_up_requests
            .iter()
            .cloned()
            .map(|request| self.request(request, Duration::from_secs(3)))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(LiveTdlibBridgeResult {
            library_path: self.library_path.clone(),
            authorization_state,
            priority,
            plan,
            primary_request,
            primary_response,
            follow_up_requests,
            follow_up_responses,
        })
    }

    fn tag_request(&mut self, mut request: Value) -> Result<(String, Value), TdlibRuntimeError> {
        let extra = format!("codex-{}", self.request_seq);
        self.request_seq += 1;
        let object = request
            .as_object_mut()
            .ok_or(TdlibRuntimeError::NonObjectRequest)?;
        object.insert("@extra".to_string(), Value::String(extra.clone()));
        Ok((extra, request))
    }

    fn wait_for_extra(
        &mut self,
        extra: &str,
        timeout: Duration,
    ) -> Result<Option<Value>, TdlibRuntimeError> {
        if let Some(index) = self
            .backlog
            .iter()
            .position(|message| message_extra(message) == Some(extra))
        {
            return Ok(Some(self.backlog.remove(index)));
        }

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = remaining.min(Duration::from_millis(200));
            match self.client.receive(wait)? {
                Some(message) if message_extra(&message) == Some(extra) => {
                    return Ok(Some(message));
                }
                Some(message) => self.backlog.push(message),
                None => {}
            }
        }
        Err(TdlibRuntimeError::ResponseTimeout {
            extra: extra.to_string(),
        })
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
            tdlib_requests::check_database_encryption_key(),
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

fn message_extra(value: &Value) -> Option<&str> {
    value.get("@extra").and_then(Value::as_str)
}

fn authorization_state_from_value(value: &Value) -> AuthorizationState {
    let current = if value.get("@type").and_then(Value::as_str) == Some("updateAuthorizationState")
    {
        value.get("authorization_state").unwrap_or(value)
    } else {
        value
    };

    match current.get("@type").and_then(Value::as_str) {
        Some("authorizationStateWaitTdlibParameters") => AuthorizationState::WaitTdlibParameters,
        Some("authorizationStateWaitEncryptionKey") => AuthorizationState::WaitEncryptionKey,
        Some("authorizationStateWaitPhoneNumber") => AuthorizationState::WaitPhoneNumber,
        Some("authorizationStateWaitCode") => AuthorizationState::WaitCode,
        Some("authorizationStateWaitPassword") => AuthorizationState::WaitPassword,
        Some("authorizationStateWaitRegistration") => AuthorizationState::WaitRegistration,
        Some("authorizationStateWaitOtherDeviceConfirmation") => {
            AuthorizationState::WaitOtherDeviceConfirmation
        }
        Some("authorizationStateReady") => AuthorizationState::Ready,
        Some("authorizationStateLoggingOut") => AuthorizationState::LoggingOut,
        Some("authorizationStateClosing") => AuthorizationState::Closing,
        Some("authorizationStateClosed") => AuthorizationState::Closed,
        Some(other) => AuthorizationState::Unknown(other.to_string()),
        None => AuthorizationState::Unknown("unknown".to_string()),
    }
}

pub fn tdlib_priority_from_plan(plan: &TransferPlan) -> i32 {
    let base = match plan.direction {
        TransferDirection::Download => 8,
        TransferDirection::Upload => 10,
    };
    (base + (plan.worker_count as i32 * 3) + plan.parallel_file_budget as i32).clamp(1, 32)
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

    pub fn check_database_encryption_key() -> Value {
        json!({
            "@type": "checkDatabaseEncryptionKey",
            "encryption_key": ""
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
            "file_type": {
                "@type": "fileTypeDocument"
            },
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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tg_core::TransferPlan;
    use uuid::Uuid;

    use super::{AuthorizationState, authorization_state_from_value, tdlib_priority_from_plan};

    #[test]
    fn authorization_state_parser_handles_direct_and_update_forms() {
        let direct = json!({ "@type": "authorizationStateReady" });
        let update = json!({
            "@type": "updateAuthorizationState",
            "authorization_state": { "@type": "authorizationStateWaitPhoneNumber" }
        });

        assert_eq!(
            authorization_state_from_value(&direct),
            AuthorizationState::Ready
        );
        assert_eq!(
            authorization_state_from_value(&update),
            AuthorizationState::WaitPhoneNumber
        );
    }

    #[test]
    fn tdlib_priority_stays_in_valid_range() {
        let plan = TransferPlan {
            job_id: Uuid::nil(),
            direction: tg_core::TransferDirection::Download,
            part_size: 1024 * 1024,
            total_parts: 1024,
            worker_count: 8,
            parallel_file_budget: 2,
            big_file_api: false,
            needs_md5_for_finalize: false,
            verify_hashes: true,
            allow_cdn: true,
            notes: Vec::new(),
        };

        let priority = tdlib_priority_from_plan(&plan);
        assert!((1..=32).contains(&priority));
    }
}
