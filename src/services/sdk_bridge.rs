//! Versioned stdio JSONL client for the Pi SDK sidecar bridge.

use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model_runtime::{AuthEvent, AuthMethod, ModelIdentity, ThinkingLevel};
use crate::orchestration::{OrchestrationActionRequest, OrchestrationSnapshot};
use crate::resource_center::ResourceInventorySnapshot;
use crate::services::pi_process::SUPPORTED_PI_VERSION;

const PROTOCOL_VERSION: u64 = 1;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const ORCHESTRATION_PIPE_ENV: &str = "PI_GUI_ORCHESTRATION_PIPE";

pub fn orchestration_adapter_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bridge/orchestration-adapter.mjs")
}

pub fn orchestration_endpoint(working_directory: &std::path::Path) -> String {
    let normalized = working_directory.to_string_lossy();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte.to_ascii_lowercase());
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if cfg!(windows) {
        format!(
            r"\\.\pipe\pi-gui-orchestration-{}-{hash:016x}",
            std::process::id()
        )
    } else {
        std::env::temp_dir()
            .join(format!(
                "pi-gui-orchestration-{}-{hash:016x}.sock",
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
}

impl SdkBridgeConfig {
    pub fn from_installation(
        installation: &crate::services::pi_process::PiInstallation,
        working_directory: PathBuf,
    ) -> Option<Self> {
        Some(Self {
            node: installation.executable.clone(),
            sdk_root: installation.sdk_package_root()?,
            script: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bridge/pi-bridge.mjs"),
            working_directory,
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
            Self::SetModelDefaults { .. } | Self::SetModelScope { .. } => {
                capabilities.model_settings
            }
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
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    pending: Mutex<HashMap<String, mpsc::Sender<Result<Value, BridgeError>>>>,
    next_id: AtomicU64,
    stopped: AtomicBool,
    healthy: AtomicBool,
    event_sender: Sender<BridgeEvent>,
}

#[derive(Clone)]
pub struct SdkBridgeClient {
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
        let mut child = Command::new(&config.node)
            .arg(&config.script)
            .arg(&config.sdk_root)
            .env(
                ORCHESTRATION_PIPE_ENV,
                orchestration_endpoint(&config.working_directory),
            )
            .current_dir(&config.working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| {
                BridgeError::new(
                    BridgeErrorKind::Unavailable,
                    "The Pi SDK bridge could not start.",
                )
            })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            BridgeError::new(
                BridgeErrorKind::Unavailable,
                "The bridge has no input pipe.",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            BridgeError::new(
                BridgeErrorKind::Unavailable,
                "The bridge has no output pipe.",
            )
        })?;
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut buffer = Vec::new();
                while reader.read_until(b'\n', &mut buffer).unwrap_or(0) > 0 {
                    buffer.clear();
                }
            });
        }
        let (event_sender, events) = async_channel::unbounded();
        let inner = Arc::new(BridgeInner {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(Some(stdin)),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            stopped: AtomicBool::new(false),
            healthy: AtomicBool::new(true),
            event_sender,
        });
        spawn_reader(stdout, Arc::clone(&inner));
        let provisional = Self {
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
        let (sender, receiver) = mpsc::channel();
        self.inner
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.clone(), sender);
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
                Err(BridgeError::new(
                    BridgeErrorKind::Timeout,
                    "The bridge operation is still cancelling.",
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
        write_record(
            &self.inner,
            &CancelRecord {
                version: PROTOCOL_VERSION,
                record_type: "cancel",
                id: &id,
                target_id,
            },
        )
    }

    pub fn stop(&self) {
        if self.inner.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        self.inner.healthy.store(false, Ordering::Release);
        self.inner
            .stdin
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(mut child) = self
            .inner
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        fail_pending(&self.inner, "The Pi SDK bridge stopped.");
    }
}

impl Drop for BridgeInner {
    fn drop(&mut self) {
        if let Some(mut child) = self
            .child
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn write_record<T: Serialize>(inner: &BridgeInner, record: &T) -> Result<(), BridgeError> {
    let mut bytes = serde_json::to_vec(record).map_err(|_| {
        BridgeError::new(BridgeErrorKind::Protocol, "Bridge request encoding failed.")
    })?;
    bytes.push(b'\n');
    let mut guard = inner
        .stdin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let stdin = guard.as_mut().ok_or_else(|| {
        BridgeError::new(BridgeErrorKind::Disconnected, "The bridge input is closed.")
    })?;
    let result = stdin
        .write_all(&bytes)
        .and_then(|_| stdin.flush())
        .map_err(|_| BridgeError::new(BridgeErrorKind::Disconnected, "The bridge input failed."));
    if result.is_err() {
        inner.healthy.store(false, Ordering::Release);
    }
    result
}

fn spawn_reader(stdout: impl std::io::Read + Send + 'static, inner: Arc<BridgeInner>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(_) if buffer.len() > MAX_RECORD_BYTES => continue,
                Ok(_) => {}
            }
            let Ok(value) = serde_json::from_slice::<Value>(&buffer) else {
                continue;
            };
            if value.get("type").and_then(Value::as_str) == Some("event") {
                if let Ok(event) = serde_json::from_value::<BridgeEvent>(value) {
                    let _ = inner.event_sender.send_blocking(event);
                }
                continue;
            }
            let Ok(response) = serde_json::from_value::<ResponseRecord>(value) else {
                continue;
            };
            if response.version != PROTOCOL_VERSION || response.record_type != "response" {
                continue;
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
        inner.healthy.store(false, Ordering::Release);
        inner
            .stdin
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        fail_pending(&inner, "The Pi SDK bridge disconnected.");
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
    pub fn spawn(working_directory: PathBuf) -> Self {
        let (commands, command_receiver) = mpsc::channel();
        let (result_sender, results) = async_channel::unbounded();
        thread::spawn(move || bridge_worker(working_directory, command_receiver, result_sender));
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
        let _ = self.commands.send(BridgeWorkerCommand::Shutdown);
    }
}

fn bridge_worker(
    working_directory: PathBuf,
    commands: mpsc::Receiver<BridgeWorkerCommand>,
    results: Sender<BridgeWorkerResult>,
) {
    let (internal_sender, internal_receiver) = mpsc::channel();
    let mut client = start_discovered_bridge(&working_directory);
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
        if client.as_ref().is_ok_and(|active| !active.is_healthy()) {
            if let Ok(active) = &client {
                active.stop();
            }
            client = Err(BridgeError::new(
                BridgeErrorKind::Disconnected,
                "The Pi SDK bridge disconnected.",
            ));
            event_receiver = None;
            reconnect_due.get_or_insert_with(Instant::now);
        }
        if reconnect_due.is_some_and(|due| Instant::now() >= due) {
            client = start_discovered_bridge(&working_directory);
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
            while let Ok(event) = events.try_recv() {
                let _ = results.send_blocking(BridgeWorkerResult::Event(event));
            }
        }
        while let Ok(result) = internal_receiver.try_recv() {
            let _ = results.send_blocking(result);
        }
        let command = match commands.recv_timeout(Duration::from_millis(10)) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => BridgeWorkerCommand::Shutdown,
        };
        match command {
            BridgeWorkerCommand::Execute { id, command } => {
                let Err(unavailable) = client.as_ref() else {
                    let active = client
                        .as_ref()
                        .expect("matched successful bridge client")
                        .clone();
                    let internal = internal_sender.clone();
                    thread::spawn(move || {
                        let result = active.call_with_id(
                            command.clone(),
                            format!("operation-{id}"),
                            Duration::from_secs(300),
                        );
                        let _ = internal.send(BridgeWorkerResult::Completed {
                            id,
                            command,
                            result,
                        });
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
                client = start_discovered_bridge(&working_directory);
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

fn start_discovered_bridge(
    working_directory: &std::path::Path,
) -> Result<SdkBridgeClient, BridgeError> {
    let installation =
        crate::services::pi_process::discover_and_probe(None, Duration::from_secs(5)).map_err(
            |_| {
                BridgeError::new(
                    BridgeErrorKind::Unavailable,
                    "The compatible Pi SDK bridge is unavailable.",
                )
            },
        )?;
    let config = SdkBridgeConfig::from_installation(&installation, working_directory.to_path_buf())
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
