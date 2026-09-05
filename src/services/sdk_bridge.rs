//! Versioned stdio JSONL client for the Pi SDK sidecar bridge.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model_runtime::{AuthEvent, AuthMethod, ModelIdentity, ThinkingLevel};
use crate::orchestration::{OrchestrationActionRequest, OrchestrationSnapshot};
use crate::resource_center::ResourceInventorySnapshot;
use crate::services::pi_process::{ProcessHandle, SUPPORTED_PI_VERSION, spawn_contained};

const PROTOCOL_VERSION: u64 = 1;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_BRIDGE_EVENTS: usize = 64;
const MAX_PENDING_REQUESTS: usize = 64;
const MAX_BUFFERED_WRITES: usize = 4 * MAX_RECORD_BYTES;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const BRIDGE_ENTRYPOINT: &str = "pi-bridge.mjs";
const ORCHESTRATION_ADAPTER: &str = "orchestration-adapter.mjs";
const EMBEDDED_BRIDGE_FILES: [(&str, &[u8]); 6] = [
    (
        BRIDGE_ENTRYPOINT,
        include_bytes!("../../bridge/pi-bridge.mjs"),
    ),
    ("jsonl.mjs", include_bytes!("../../bridge/jsonl.mjs")),
    ("pi-contract.mjs", include_bytes!("../../bridge/pi-contract.mjs")),
    (
        "pi-settings.mjs",
        include_bytes!("../../bridge/pi-settings.mjs"),
    ),
    (
        ORCHESTRATION_ADAPTER,
        include_bytes!("../../bridge/orchestration-adapter.mjs"),
    ),
    (
        "orchestration-core.mjs",
        include_bytes!("../../bridge/orchestration-core.mjs"),
    ),
];
pub const ORCHESTRATION_PIPE_ENV: &str = "PI_GUI_ORCHESTRATION_PIPE";
static NEXT_ORCHESTRATION_INSTANCE: AtomicU64 = AtomicU64::new(1);

pub fn orchestration_adapter_path() -> PathBuf {
    embedded_bridge_path(ORCHESTRATION_ADAPTER).unwrap_or_default()
}

fn embedded_bridge_path(file_name: &str) -> Option<PathBuf> {
    materialize_embedded_bridge()
        .ok()
        .map(|directory| directory.join(file_name))
}

fn materialize_embedded_bridge() -> std::io::Result<PathBuf> {
    // Node's ESM loader and Pi's extension loader both require filesystem paths.
    // Revalidate on every launch so temp-directory cleanup cannot break reconnects.
    let directory = std::env::temp_dir()
        .join("pideck")
        .join(format!("bridge-{:016x}", embedded_bridge_fingerprint()));
    fs::create_dir_all(&directory)?;

    for &(file_name, contents) in &EMBEDDED_BRIDGE_FILES {
        let path = directory.join(file_name);
        if fs::read(&path).ok().as_deref() != Some(contents) {
            fs::write(path, contents)?;
        }
    }

    Ok(directory)
}

fn embedded_bridge_fingerprint() -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &(file_name, contents) in &EMBEDDED_BRIDGE_FILES {
        for byte in file_name
            .bytes()
            .chain(std::iter::once(0))
            .chain(contents.iter().copied())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

pub fn allocate_orchestration_endpoint(working_directory: &std::path::Path) -> String {
    orchestration_endpoint(
        working_directory,
        NEXT_ORCHESTRATION_INSTANCE.fetch_add(1, Ordering::Relaxed),
    )
}

pub fn orchestration_endpoint(working_directory: &std::path::Path, instance: u64) -> String {
    let normalized = working_directory.to_string_lossy();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte.to_ascii_lowercase());
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if cfg!(windows) {
        format!(
            r"\\.\pipe\pi-gui-orchestration-{}-{hash:016x}-{instance:016x}",
            std::process::id()
        )
    } else {
        std::env::temp_dir()
            .join(format!(
                "pi-gui-orchestration-{}-{hash:016x}-{instance:016x}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct SensitiveValue(String);

impl SensitiveValue {
    pub fn new(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Debug for SensitiveValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

#[derive(Debug, Clone)]
pub struct SdkBridgeConfig {
    pub node: PathBuf,
    pub sdk_root: PathBuf,
    pub script: PathBuf,
    pub working_directory: PathBuf,
    pub orchestration_endpoint: String,
}

impl SdkBridgeConfig {
    pub fn from_installation(
        installation: &crate::services::pi_process::PiInstallation,
        working_directory: PathBuf,
        orchestration_endpoint: String,
    ) -> Option<Self> {
        Some(Self {
            node: installation.executable.clone(),
            sdk_root: installation.sdk_package_root()?,
            script: embedded_bridge_path(BRIDGE_ENTRYPOINT)?,
            working_directory,
            orchestration_endpoint,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCapabilities {
    pub navigate_tree: bool,
    pub branch_summary: bool,
    pub labels: bool,
    pub jsonl_import: bool,
    pub jsonl_export: bool,
    pub session_list: bool,
    pub model_runtime: bool,
    pub provider_auth: bool,
    pub model_settings: bool,
    pub resource_inventory: bool,
    pub resource_reload: bool,
    pub active_tool_state: bool,
    pub resource_settings: bool,
    pub package_mutations: bool,
    pub orchestration: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeHello {
    pub protocol_version: u64,
    pub sdk_version: String,
    pub capabilities: BridgeCapabilities,
    #[serde(default)]
    pub transport: String,
    #[serde(default)]
    pub ownership: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeErrorKind {
    Unavailable,
    Protocol,
    Rejected,
    Timeout,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeError {
    pub kind: BridgeErrorKind,
    pub code: Option<String>,
    pub summary: String,
}

impl BridgeError {
    fn new(kind: BridgeErrorKind, summary: impl Into<String>) -> Self {
        Self {
            kind,
            code: None,
            summary: summary.into(),
        }
    }

    fn rejected(code: Option<String>, summary: impl Into<String>) -> Self {
        Self {
            kind: BridgeErrorKind::Rejected,
            code,
            summary: summary.into(),
        }
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.summary.fmt(formatter)
    }
}

impl std::error::Error for BridgeError {}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum BridgeCommand {
    Hello,
    NavigateTree {
        #[serde(rename = "sessionPath")]
        session_path: String,
        cwd: String,
        #[serde(rename = "targetId")]
        target_id: String,
        summarize: bool,
        #[serde(rename = "customInstructions", skip_serializing_if = "Option::is_none")]
        custom_instructions: Option<String>,
        #[serde(rename = "replaceInstructions")]
        replace_instructions: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    SetLabel {
        #[serde(rename = "sessionPath")]
        session_path: String,
        cwd: String,
        #[serde(rename = "targetId")]
        target_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    ExportJsonl {
        #[serde(rename = "sessionPath")]
        session_path: String,
        cwd: String,
        #[serde(rename = "outputPath", skip_serializing_if = "Option::is_none")]
        output_path: Option<String>,
    },
    ImportJsonl {
        #[serde(rename = "inputPath")]
        input_path: String,
        cwd: String,
        #[serde(rename = "sessionDir")]
        session_dir: String,
    },
    GetModelRuntime,
    RefreshModels,
    LoginProvider {
        #[serde(rename = "operationId")]
        operation_id: u64,
        provider: String,
        #[serde(rename = "authType")]
        auth_type: AuthMethod,
    },
    AuthRespond {
        #[serde(rename = "operationId")]
        operation_id: u64,
        #[serde(rename = "promptId")]
        prompt_id: String,
        value: SensitiveValue,
    },
    LogoutProvider {
        provider: String,
    },
    SetModelDefaults {
        model: Option<ModelIdentity>,
        thinking: Option<ThinkingLevel>,
    },
    SetModelScope {
        models: Vec<ModelIdentity>,
    },
    SetPiSetting {
        key: String,
        value: Value,
    },
    GetResourceInventory,
    ReloadResources,
    SetSkillCommandsEnabled {
        enabled: bool,
    },
    SetResourceTheme {
        theme: String,
    },
    GetOrchestrationSnapshot {
        #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    OrchestrationAction {
        action: OrchestrationActionRequest,
    },
}

impl BridgeCommand {
    fn supported_by(&self, capabilities: &BridgeCapabilities) -> bool {
        match self {
            Self::Hello => true,
            Self::NavigateTree { .. } => capabilities.navigate_tree,
            Self::SetLabel { .. } => capabilities.labels,
            Self::ExportJsonl { .. } => capabilities.jsonl_export,
            Self::ImportJsonl { .. } => capabilities.jsonl_import,
            Self::GetModelRuntime | Self::RefreshModels => capabilities.model_runtime,
            Self::LoginProvider { .. } | Self::AuthRespond { .. } | Self::LogoutProvider { .. } => {
                capabilities.provider_auth
            }
            Self::SetModelDefaults { .. }
            | Self::SetModelScope { .. }
            | Self::SetPiSetting { .. } => capabilities.model_settings,
            Self::GetResourceInventory => capabilities.resource_inventory,
            Self::ReloadResources => capabilities.resource_reload,
            Self::SetSkillCommandsEnabled { .. } | Self::SetResourceTheme { .. } => {
                capabilities.resource_settings
            }
            Self::GetOrchestrationSnapshot { .. } | Self::OrchestrationAction { .. } => {
                capabilities.orchestration
            }
        }
    }
}

#[derive(Serialize)]
struct RequestRecord<'a> {
    version: u64,
    #[serde(rename = "type")]
    record_type: &'static str,
    id: &'a str,
    command: &'a str,
    params: Value,
}

#[derive(Serialize)]
struct CancelRecord<'a> {
    version: u64,
    #[serde(rename = "type")]
    record_type: &'static str,
    id: &'a str,
    #[serde(rename = "targetId")]
    target_id: &'a str,
}

#[derive(Deserialize)]
struct ResponseRecord {
    version: u64,
    #[serde(rename = "type")]
    record_type: String,
    id: String,
    ok: bool,
    result: Option<Value>,
    error: Option<BridgeWireError>,
}

#[derive(Deserialize)]
struct BridgeWireError {
    #[serde(default)]
    code: Option<String>,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ResourceEvent {
    ResourceProgress {
        operation: String,
        phase: String,
        message: String,
    },
    ResourcesChanged {
        generation: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum OrchestrationEvent {
    OrchestrationSnapshot {
        snapshot: Box<OrchestrationSnapshot>,
    },
    OrchestrationDisconnected,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum BridgeEvent {
    Auth(AuthEvent),
    Resource(ResourceEvent),
    Orchestration(OrchestrationEvent),
}

struct BridgeInner {
    child: Mutex<Option<ProcessHandle>>,
    outgoing: Sender<Vec<u8>>,
    buffered_writes: AtomicUsize,
    pending: Mutex<HashMap<String, mpsc::Sender<Result<Value, BridgeError>>>>,
    next_id: AtomicU64,
    stopped: AtomicBool,
    healthy: AtomicBool,
    event_sender: Sender<BridgeEvent>,
}

// Reader/writer threads own the transport, not its lifetime. The final client
// owner closes the child even while a read or write is blocked in a pipe.
struct BridgeLifetime(Weak<BridgeInner>);

impl Drop for BridgeLifetime {
    fn drop(&mut self) {
        if let Some(inner) = self.0.upgrade() {
            stop_bridge(&inner, "The Pi SDK bridge owner closed.");
        }
    }
}

#[derive(Clone)]
pub struct SdkBridgeClient {
    lifetime: Arc<BridgeLifetime>,
    inner: Arc<BridgeInner>,
    hello: BridgeHello,
    events: Receiver<BridgeEvent>,
}

impl SdkBridgeClient {
    pub fn start(config: SdkBridgeConfig) -> Result<Self, BridgeError> {
        if !config.script.is_file() || !config.sdk_root.is_dir() {
            return Err(BridgeError::new(
                BridgeErrorKind::Unavailable,
                "The compatible Pi SDK bridge is unavailable.",
            ));
        }
        let mut process = spawn_contained(
            &config.node,
            &[config.script.into_os_string(), config.sdk_root.into_os_string()],
            &config.working_directory,
            &[(ORCHESTRATION_PIPE_ENV.into(), config.orchestration_endpoint.into())],
        )
        .map_err(|_| {
            BridgeError::new(BridgeErrorKind::Unavailable, "The Pi SDK bridge could not start.")
        })?;
        let (Some(stdin), Some(stdout)) = (process.stdin.take(), process.stdout.take()) else {
            let _ = process.handle.terminate();
            let _ = process.handle.wait_for(Duration::from_secs(3));
            return Err(BridgeError::new(
                BridgeErrorKind::Unavailable,
                "The Pi SDK bridge did not provide its standard pipes.",
            ));
        };
        if let Some(mut stderr) = process.stderr.take() {
            thread::spawn(move || {
                // Discard diagnostics with constant space, including a child that
                // writes indefinitely without a newline. Never retain secrets.
                let mut buffer = [0_u8; 8192];
                while stderr.read(&mut buffer).unwrap_or(0) > 0 {}
            });
        }
        let (event_sender, events) = async_channel::bounded(MAX_BRIDGE_EVENTS);
        let (outgoing, outgoing_receiver) = async_channel::bounded(MAX_PENDING_REQUESTS);
        let inner = Arc::new(BridgeInner {
            child: Mutex::new(Some(process.handle)),
            outgoing,
            buffered_writes: AtomicUsize::new(0),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            stopped: AtomicBool::new(false),
            healthy: AtomicBool::new(true),
            event_sender,
        });
        spawn_reader(stdout, Arc::clone(&inner));
        spawn_writer(stdin, outgoing_receiver, Arc::clone(&inner));
        let provisional = Self {
            lifetime: Arc::new(BridgeLifetime(Arc::downgrade(&inner))),
            inner,
            hello: BridgeHello {
                protocol_version: 0,
                sdk_version: String::new(),
                capabilities: BridgeCapabilities::default(),
                transport: String::new(),
                ownership: String::new(),
            },
            events: events.clone(),
        };
        let hello_value = match provisional.call(BridgeCommand::Hello, Duration::from_secs(10)) {
            Ok(value) => value,
            Err(error) => {
                provisional.stop();
                return Err(error);
            }
        };
        let hello: BridgeHello = match serde_json::from_value(hello_value) {
            Ok(hello) => hello,
            Err(_) => {
                provisional.stop();
                return Err(BridgeError::new(
                    BridgeErrorKind::Protocol,
                    "The bridge hello was invalid.",
                ));
            }
        };
        if hello.protocol_version != PROTOCOL_VERSION {
            provisional.stop();
            return Err(BridgeError::new(
                BridgeErrorKind::Protocol,
                "The Pi SDK bridge protocol is incompatible.",
            ));
        }
        if hello.sdk_version != SUPPORTED_PI_VERSION {
            provisional.stop();
            return Err(BridgeError::new(
                BridgeErrorKind::Protocol,
                "The Pi SDK bridge version is incompatible.",
            ));
        }
        Ok(Self {
            lifetime: provisional.lifetime,
            inner: provisional.inner,
            hello,
            events,
        })
    }

    pub fn hello(&self) -> &BridgeHello {
        &self.hello
    }

    pub fn events(&self) -> Receiver<BridgeEvent> {
        self.events.clone()
    }

    fn is_healthy(&self) -> bool {
        self.inner.healthy.load(Ordering::Acquire)
    }

    pub fn call_default(&self, command: BridgeCommand) -> Result<Value, BridgeError> {
        self.call(command, DEFAULT_TIMEOUT)
    }

    pub fn call(&self, command: BridgeCommand, timeout: Duration) -> Result<Value, BridgeError> {
        let id = format!(
            "bridge-{}",
            self.inner.next_id.fetch_add(1, Ordering::Relaxed)
        );
        self.call_with_id(command, id, timeout)
    }

    pub fn call_with_id(
        &self,
        command: BridgeCommand,
        id: String,
        timeout: Duration,
    ) -> Result<Value, BridgeError> {
        if !command.supported_by(&self.hello.capabilities) {
            return Err(BridgeError::rejected(
                Some("unsupported_capability".to_owned()),
                "The negotiated bridge does not support this operation.",
            ));
        }
        let value = serde_json::to_value(&command).map_err(|_| {
            BridgeError::new(BridgeErrorKind::Protocol, "Bridge request encoding failed.")
        })?;
        let command_name = value
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let params = value
            .as_object()
            .map(|object| {
                let mut object = object.clone();
                object.remove("command");
                Value::Object(object)
            })
            .unwrap_or(Value::Null);
        let record = RequestRecord {
            version: PROTOCOL_VERSION,
            record_type: "request",
            id: &id,
            command: command_name,
            params,
        };
        if id.is_empty() || id.len() > 256 || timeout.is_zero() {
            return Err(BridgeError::new(BridgeErrorKind::Protocol, "Invalid bridge request identity or timeout."));
        }
        let (sender, receiver) = mpsc::channel();
        {
            let mut pending = self.inner.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if self.inner.stopped.load(Ordering::Acquire) {
                return Err(BridgeError::new(BridgeErrorKind::Disconnected, "The Pi SDK bridge stopped."));
            }
            if pending.contains_key(&id) {
                return Err(BridgeError::new(BridgeErrorKind::Protocol, "A bridge request with this ID is already active."));
            }
            if pending.len() >= MAX_PENDING_REQUESTS {
                return Err(BridgeError::rejected(Some("busy".to_owned()), "The Pi SDK bridge is busy. Finish or cancel an operation first."));
            }
            pending.insert(id.clone(), sender);
        }
        if let Err(error) = write_record(&self.inner, &record) {
            self.inner
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&id);
            return Err(error);
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.inner
                    .pending
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&id);
                let _ = self.cancel(&id);
                self.stop();
                Err(BridgeError::new(
                    BridgeErrorKind::Timeout,
                    "The bridge operation timed out and its process was stopped. Its outcome is unknown; it was not replayed.",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(BridgeError::new(
                BridgeErrorKind::Disconnected,
                "The Pi SDK bridge disconnected.",
            )),
        }
    }

    pub fn cancel(&self, target_id: &str) -> Result<(), BridgeError> {
        let id = format!(
            "bridge-cancel-{}",
            self.inner.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let result = write_record(
            &self.inner,
            &CancelRecord {
                version: PROTOCOL_VERSION,
                record_type: "cancel",
                id: &id,
                target_id,
            },
        );
        if result.is_err() {
            self.stop();
        }
        result
    }

    pub fn stop(&self) {
        stop_bridge(&self.inner, "The Pi SDK bridge stopped.");
    }
}

fn stop_bridge(inner: &BridgeInner, summary: &str) {
    if inner.stopped.swap(true, Ordering::AcqRel) {
        return;
    }
    inner.healthy.store(false, Ordering::Release);
    inner.outgoing.close();
    // Terminate the contained process tree to interrupt a blocked pipe writer.
    // The coordinator never holds or waits for the writer's input handle.
    if let Some(child) = inner.child.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take() {
        let _ = child.terminate();
        let _ = child.wait_for(Duration::from_secs(3));
    }
    inner.event_sender.close();
    fail_pending(inner, summary);
}

impl Drop for BridgeInner {
    fn drop(&mut self) {
        if let Some(child) = self.child.get_mut().unwrap_or_else(|poisoned| poisoned.into_inner()).take() {
            let _ = child.terminate();
            let _ = child.wait_for(Duration::from_secs(3));
        }
    }
}

fn write_record<T: Serialize>(inner: &BridgeInner, record: &T) -> Result<(), BridgeError> {
    let mut bytes = serde_json::to_vec(record).map_err(|_| {
        BridgeError::new(BridgeErrorKind::Protocol, "Bridge request encoding failed.")
    })?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(BridgeError::new(BridgeErrorKind::Protocol, "The bridge request exceeds the 1 MiB record limit."));
    }
    if inner.stopped.load(Ordering::Acquire) {
        return Err(BridgeError::new(BridgeErrorKind::Disconnected, "The Pi SDK bridge stopped."));
    }
    bytes.push(b'\n');
    let byte_count = bytes.len();
    if inner.buffered_writes.fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
        used.checked_add(byte_count).filter(|total| *total <= MAX_BUFFERED_WRITES)
    }).is_err() {
        return Err(BridgeError::rejected(Some("busy".to_owned()), "The bridge input queue is full. The request was not sent."));
    }
    if inner.outgoing.try_send(bytes).is_err() {
        inner.buffered_writes.fetch_sub(byte_count, Ordering::AcqRel);
        return Err(BridgeError::rejected(Some("busy".to_owned()), "The bridge input is closed or full. The request was not sent."));
    }
    Ok(())
}

fn spawn_writer(
    mut stdin: Box<dyn Write + Send>,
    outgoing: Receiver<Vec<u8>>,
    inner: Arc<BridgeInner>,
) {
    thread::spawn(move || {
        while let Ok(bytes) = outgoing.recv_blocking() {
            if inner.stopped.load(Ordering::Acquire) {
                break;
            }
            let result = stdin.write_all(&bytes).and_then(|_| stdin.flush());
            inner.buffered_writes.fetch_sub(bytes.len(), Ordering::AcqRel);
            if result.is_err() {
                stop_bridge(&inner, "The bridge input failed. No operation was replayed.");
                break;
            }
        }
    });
}

/// The read limit is applied before allocation, including an unterminated
/// record. Only LF delimits records; CR is stripped only as its optional suffix.
fn read_bridge_record(reader: &mut impl BufRead, buffer: &mut Vec<u8>) -> io::Result<usize> {
    buffer.clear();
    let count = reader.take((MAX_RECORD_BYTES + 2) as u64).read_until(b'\n', buffer)?;
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
    }
    if buffer.last() == Some(&b'\r') {
        buffer.pop();
    }
    if buffer.len() > MAX_RECORD_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Bridge record too large"));
    }
    Ok(count)
}

fn spawn_reader(stdout: impl Read + Send + 'static, inner: Arc<BridgeInner>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buffer = Vec::new();
        loop {
            match read_bridge_record(&mut reader, &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let Ok(value) = serde_json::from_slice::<Value>(&buffer) else {
                break;
            };
            if value.get("version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
                break;
            }
            if value.get("type").and_then(Value::as_str) == Some("event") {
                if let Ok(event) = serde_json::from_value::<BridgeEvent>(value) {
                    // Never block the response reader behind a stalled UI. A
                    // saturated bridge is a visible failure, not an auth hang.
                    if inner.event_sender.try_send(event).is_err() {
                        break;
                    }
                }
                continue;
            }
            let Ok(response) = serde_json::from_value::<ResponseRecord>(value) else {
                break;
            };
            if response.version != PROTOCOL_VERSION || response.record_type != "response" {
                break;
            }
            let sender = inner
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&response.id);
            let Some(sender) = sender else { continue };
            let result = if response.ok {
                Ok(response.result.unwrap_or(Value::Null))
            } else {
                Err(BridgeError::rejected(
                    response.error.as_ref().and_then(|error| error.code.clone()),
                    response
                        .error
                        .map(|error| error.message)
                        .unwrap_or_else(|| "The bridge operation was rejected.".to_owned()),
                ))
            };
            let _ = sender.send(result);
        }
        stop_bridge(&inner, "The Pi SDK bridge disconnected or exceeded a transport limit. Reconnect; no operation was replayed.");
    });
}

fn fail_pending(inner: &BridgeInner, summary: &str) {
    let pending = std::mem::take(
        &mut *inner
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    );
    for sender in pending.into_values() {
        let _ = sender.send(Err(BridgeError::new(
            BridgeErrorKind::Disconnected,
            summary,
        )));
    }
}

#[derive(Debug, Clone)]
pub enum BridgeWorkerResult {
    Capabilities(Result<BridgeHello, BridgeError>),
    Event(BridgeEvent),
    Completed {
        id: u64,
        command: BridgeCommand,
        result: Result<Value, BridgeError>,
    },
}

enum BridgeWorkerCommand {
    Execute { id: u64, command: BridgeCommand },
    Cancel { id: u64 },
    Restart,
    Shutdown,
}

pub struct SdkBridgeWorker {
    commands: mpsc::Sender<BridgeWorkerCommand>,
    results: Receiver<BridgeWorkerResult>,
}

impl SdkBridgeWorker {
    pub fn spawn(working_directory: PathBuf, orchestration_endpoint: String) -> Self {
        let (commands, command_receiver) = mpsc::channel();
        let (result_sender, results) = async_channel::bounded(MAX_BRIDGE_EVENTS);
        thread::spawn(move || {
            bridge_worker(
                working_directory,
                orchestration_endpoint,
                command_receiver,
                result_sender,
            )
        });
        Self { commands, results }
    }

    pub fn results(&self) -> Receiver<BridgeWorkerResult> {
        self.results.clone()
    }

    pub fn execute(&self, id: u64, command: BridgeCommand) -> bool {
        self.commands
            .send(BridgeWorkerCommand::Execute { id, command })
            .is_ok()
    }

    pub fn cancel(&self, id: u64) -> bool {
        self.commands
            .send(BridgeWorkerCommand::Cancel { id })
            .is_ok()
    }

    pub fn restart(&self) -> bool {
        self.commands.send(BridgeWorkerCommand::Restart).is_ok()
    }
}

impl Drop for SdkBridgeWorker {
    fn drop(&mut self) {
        self.results.close();
        let _ = self.commands.send(BridgeWorkerCommand::Shutdown);
    }
}

fn bridge_worker(
    working_directory: PathBuf,
    orchestration_endpoint: String,
    commands: mpsc::Receiver<BridgeWorkerCommand>,
    results: Sender<BridgeWorkerResult>,
) {
    let (internal_sender, internal_receiver) = mpsc::sync_channel(MAX_PENDING_REQUESTS);
    let mut generation = 1_u64;
    let mut in_flight = HashMap::<u64, BridgeCommand>::new();
    let mut client = start_discovered_bridge(&working_directory, &orchestration_endpoint);
    let mut event_receiver = client.as_ref().ok().map(SdkBridgeClient::events);
    let mut reconnect_delay = Duration::from_secs(1);
    let mut reconnect_due = client.is_err().then(|| Instant::now() + reconnect_delay);
    let _ = results.send_blocking(BridgeWorkerResult::Capabilities(
        client
            .as_ref()
            .map(|client| client.hello().clone())
            .map_err(Clone::clone),
    ));
    loop {
        if results.is_closed() {
            if let Ok(active) = &client {
                active.stop();
            }
            break;
        }
        if client.as_ref().is_ok_and(|active| !active.is_healthy()) {
            if let Ok(active) = &client {
                active.stop();
            }
            generation = generation.saturating_add(1);
            fail_worker_operations(&mut in_flight, &results);
            client = Err(BridgeError::new(
                BridgeErrorKind::Disconnected,
                "The Pi SDK bridge disconnected.",
            ));
            event_receiver = None;
            reconnect_due.get_or_insert_with(Instant::now);
        }
        if reconnect_due.is_some_and(|due| Instant::now() >= due) {
            client = start_discovered_bridge(&working_directory, &orchestration_endpoint);
            event_receiver = client.as_ref().ok().map(SdkBridgeClient::events);
            let _ = results.send_blocking(BridgeWorkerResult::Capabilities(
                client
                    .as_ref()
                    .map(|client| client.hello().clone())
                    .map_err(Clone::clone),
            ));
            if client.is_ok() {
                reconnect_due = None;
                reconnect_delay = Duration::from_secs(1);
            } else {
                reconnect_due = Some(Instant::now() + reconnect_delay);
                reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(15));
            }
        }
        if let Some(events) = &event_receiver {
            for _ in 0..32 {
                let Ok(event) = events.try_recv() else { break };
                if results.send_blocking(BridgeWorkerResult::Event(event)).is_err() {
                    break;
                }
            }
        }
        for _ in 0..32 {
            let Ok((completed_generation, result)) = internal_receiver.try_recv() else { break };
            if completed_generation != generation {
                continue;
            }
            if let BridgeWorkerResult::Completed { id, .. } = &result {
                in_flight.remove(id);
            }
            if results.send_blocking(result).is_err() {
                break;
            }
        }
        let command = match commands.recv_timeout(Duration::from_millis(10)) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => BridgeWorkerCommand::Shutdown,
        };
        match command {
            BridgeWorkerCommand::Execute { id, command } => {
                if in_flight.contains_key(&id) {
                    // Never give one request identity two terminal responses.
                    continue;
                }
                if in_flight.len() >= MAX_PENDING_REQUESTS {
                    let _ = results.send_blocking(BridgeWorkerResult::Completed {
                        id,
                        command,
                        result: Err(BridgeError::rejected(Some("busy".to_owned()), "The bridge is busy. Finish or cancel an operation first.")),
                    });
                    continue;
                }
                let Err(unavailable) = client.as_ref() else {
                    let active = client
                        .as_ref()
                        .expect("matched successful bridge client")
                        .clone();
                    let internal = internal_sender.clone();
                    let operation_generation = generation;
                    in_flight.insert(id, command.clone());
                    thread::spawn(move || {
                        let result = active.call_with_id(
                            command.clone(),
                            format!("operation-{id}"),
                            Duration::from_secs(300),
                        );
                        let _ = internal.send((operation_generation, BridgeWorkerResult::Completed {
                            id,
                            command,
                            result,
                        }));
                    });
                    continue;
                };
                {
                    let _ = results.send_blocking(BridgeWorkerResult::Completed {
                        id,
                        command,
                        result: Err(unavailable.clone()),
                    });
                    continue;
                }
            }
            BridgeWorkerCommand::Cancel { id } => {
                if let Ok(active) = &client {
                    let _ = active.cancel(&format!("operation-{id}"));
                }
            }
            BridgeWorkerCommand::Restart => {
                if let Ok(active) = &client {
                    active.stop();
                }
                generation = generation.saturating_add(1);
                fail_worker_operations(&mut in_flight, &results);
                client = start_discovered_bridge(&working_directory, &orchestration_endpoint);
                event_receiver = client.as_ref().ok().map(SdkBridgeClient::events);
                reconnect_delay = Duration::from_secs(1);
                reconnect_due = client.is_err().then(|| Instant::now() + reconnect_delay);
                let _ = results.send_blocking(BridgeWorkerResult::Capabilities(
                    client
                        .as_ref()
                        .map(|client| client.hello().clone())
                        .map_err(Clone::clone),
                ));
            }
            BridgeWorkerCommand::Shutdown => {
                if let Ok(active) = &client {
                    active.stop();
                }
                break;
            }
        }
    }
}

fn fail_worker_operations(
    in_flight: &mut HashMap<u64, BridgeCommand>,
    results: &Sender<BridgeWorkerResult>,
) {
    for (id, command) in in_flight.drain() {
        let _ = results.send_blocking(BridgeWorkerResult::Completed {
            id,
            command,
            result: Err(BridgeError::new(
                BridgeErrorKind::Disconnected,
                "The bridge disconnected before the operation completed. Its outcome is unknown; it was not replayed.",
            )),
        });
    }
}

fn start_discovered_bridge(
    working_directory: &std::path::Path,
    orchestration_endpoint: &str,
) -> Result<SdkBridgeClient, BridgeError> {
    let installation = crate::services::pi_process::discover_and_probe(
        None,
        crate::services::pi_process::DEFAULT_PROBE_TIMEOUT,
    )
    .map_err(|_| {
        BridgeError::new(
            BridgeErrorKind::Unavailable,
            "The compatible Pi SDK bridge is unavailable.",
        )
    })?;
    let config = SdkBridgeConfig::from_installation(
        &installation,
        working_directory.to_path_buf(),
        orchestration_endpoint.to_owned(),
    )
    .ok_or_else(|| {
        BridgeError::new(
            BridgeErrorKind::Unavailable,
            "This Pi installation does not expose the SDK bridge.",
        )
    })?;
    SdkBridgeClient::start(config)
}

pub fn decode_resource_snapshot(value: Value) -> Result<ResourceInventorySnapshot, BridgeError> {
    serde_json::from_value(value).map_err(|_| {
        BridgeError::new(
            BridgeErrorKind::Protocol,
            "The bridge returned an invalid resource inventory.",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_reader_limits_unterminated_output_before_growing() {
        let input = vec![b'x'; MAX_RECORD_BYTES * 8];
        let mut reader = io::Cursor::new(input);
        let mut bytes = Vec::new();
        assert!(read_bridge_record(&mut reader, &mut bytes).is_err());
        assert_eq!(reader.position(), (MAX_RECORD_BYTES + 2) as u64);
        assert!(bytes.len() <= MAX_RECORD_BYTES + 2);
    }

    #[test]
    fn bridge_reader_accepts_exact_limit_crlf_and_unicode_separators() {
        let mut input = vec![b'x'; MAX_RECORD_BYTES];
        input.extend_from_slice(b"\r\n");
        input.extend_from_slice("a\u{2028}b\u{2029}c\nlast".as_bytes());
        let mut reader = io::Cursor::new(input);
        let mut bytes = Vec::new();
        assert_eq!(read_bridge_record(&mut reader, &mut bytes).unwrap(), MAX_RECORD_BYTES + 2);
        assert_eq!(bytes.len(), MAX_RECORD_BYTES);
        read_bridge_record(&mut reader, &mut bytes).unwrap();
        assert_eq!(String::from_utf8(bytes.clone()).unwrap(), "a\u{2028}b\u{2029}c");
        read_bridge_record(&mut reader, &mut bytes).unwrap();
        assert_eq!(bytes, b"last");
        assert_eq!(read_bridge_record(&mut reader, &mut bytes).unwrap(), 0);
    }

    #[test]
    fn embedded_bridge_materializes_every_runtime_module() {
        let directory = materialize_embedded_bridge().expect("materialize embedded bridge");

        for &(file_name, contents) in &EMBEDDED_BRIDGE_FILES {
            assert_eq!(
                fs::read(directory.join(file_name)).expect("read materialized bridge module"),
                contents
            );
        }
        assert_eq!(
            orchestration_adapter_path(),
            directory.join(ORCHESTRATION_ADAPTER)
        );
    }
}
