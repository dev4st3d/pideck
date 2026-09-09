//! Real pseudo-terminal process boundary for the workspace terminal panel.
//!
//! The worker owns the PTY, shell process, and blocking I/O threads. GPUI only
//! exchanges bounded events and non-blocking control messages with it.
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use async_channel::{Receiver, Sender};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

// Reader backpressure bounds queued terminal output to 512 KiB.
const EVENT_CAPACITY: usize = 64;
const READ_BUFFER_BYTES: usize = 8 * 1024;
const COMMAND_CAPACITY: usize = 1_024;
pub const DEFAULT_ROWS: u16 = 16;
pub const DEFAULT_COLS: u16 = 120;
pub const MIN_ROWS: u16 = 4;
pub const MIN_COLS: u16 = 24;
pub const MAX_ROWS: u16 = 300;
pub const MAX_COLS: u16 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

impl TerminalSize {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows.clamp(MIN_ROWS, MAX_ROWS),
            cols: cols.clamp(MIN_COLS, MAX_COLS),
        }
    }

    fn as_pty(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self::new(DEFAULT_ROWS, DEFAULT_COLS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    Started {
        shell: String,
        process_id: Option<u32>,
    },
    Output(Vec<u8>),
    Exited {
        code: u32,
    },
    Error {
        summary: String,
    },
}

#[derive(Debug)]
enum TerminalCommand {
    Write(Vec<u8>),
    Resize(TerminalSize),
    Shutdown,
}

/// Send-only handle for a shell running inside the operating system PTY.
pub struct TerminalWorker {
    commands: mpsc::SyncSender<TerminalCommand>,
    events: Receiver<TerminalEvent>,
    shutdown_requested: Arc<AtomicBool>,
}

impl TerminalWorker {
    /// Starts worker setup on a background thread and returns immediately.
    pub fn spawn(working_directory: PathBuf, size: TerminalSize) -> Self {
        let (commands, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (event_tx, events) = async_channel::bounded(EVENT_CAPACITY);
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let worker_events = event_tx.clone();
        if thread::Builder::new()
            .name("pideck-terminal".to_owned())
            .spawn({
                let shutdown_requested = Arc::clone(&shutdown_requested);
                move || {
                    run_terminal(
                        working_directory,
                        size,
                        command_rx,
                        shutdown_requested,
                        worker_events,
                    )
                }
            })
            .is_err()
        {
            send_event(
                &event_tx,
                TerminalEvent::Error {
                    summary: "The terminal worker could not be started.".to_owned(),
                },
            );
        }
        Self {
            commands,
            events,
            shutdown_requested,
        }
    }

    pub fn events(&self) -> Receiver<TerminalEvent> {
        self.events.clone()
    }

    /// Writes raw keyboard or paste bytes directly to the PTY.
    pub fn write_bytes(&self, bytes: Vec<u8>) -> bool {
        self.commands
            .try_send(TerminalCommand::Write(bytes))
            .is_ok()
    }

    /// Writes a command exactly as typed, followed by a carriage return.
    #[cfg(test)]
    fn write_line(&self, line: &str) -> bool {
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\r');
        self.write_bytes(bytes)
    }

    pub fn resize(&self, size: TerminalSize) -> bool {
        self.commands
            .try_send(TerminalCommand::Resize(size))
            .is_ok()
    }
}

impl Drop for TerminalWorker {
    fn drop(&mut self) {
        self.shutdown_requested.store(true, Ordering::Release);
        let _ = self.commands.try_send(TerminalCommand::Shutdown);
    }
}

fn run_terminal(
    working_directory: PathBuf,
    size: TerminalSize,
    commands: mpsc::Receiver<TerminalCommand>,
    shutdown_requested: Arc<AtomicBool>,
    events: Sender<TerminalEvent>,
) {
    if !working_directory.is_dir() {
        send_event(
            &events,
            TerminalEvent::Error {
                summary: "The workspace folder is unavailable.".to_owned(),
            },
        );
        return;
    }

    let pty_system = native_pty_system();
    let Ok(pair) = pty_system.openpty(size.as_pty()) else {
        send_event(
            &events,
            TerminalEvent::Error {
                summary: "The operating system could not create a pseudo-terminal.".to_owned(),
            },
        );
        return;
    };

    let shell = default_shell_label();
    let mut command = CommandBuilder::new_default_prog();
    command.cwd(&working_directory);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");

    let Ok(mut child) = pair.slave.spawn_command(command) else {
        send_event(
            &events,
            TerminalEvent::Error {
                summary: format!("{shell} could not be started in this workspace."),
            },
        );
        return;
    };
    drop(pair.slave);

    let process_id = child.process_id();
    let mut killer = child.clone_killer();
    let Ok(mut reader) = pair.master.try_clone_reader() else {
        let _ = killer.kill();
        send_event(
            &events,
            TerminalEvent::Error {
                summary: "The terminal output stream could not be opened.".to_owned(),
            },
        );
        return;
    };
    let Ok(mut writer) = pair.master.take_writer() else {
        let _ = killer.kill();
        send_event(
            &events,
            TerminalEvent::Error {
                summary: "The terminal input stream could not be opened.".to_owned(),
            },
        );
        return;
    };

    send_event(&events, TerminalEvent::Started { shell, process_id });

    let reader_events = events.clone();
    let _ = thread::Builder::new()
        .name("pideck-terminal-output".to_owned())
        .spawn(move || {
            let mut buffer = [0_u8; READ_BUFFER_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        send_event(
                            &reader_events,
                            TerminalEvent::Output(buffer[..count].to_vec()),
                        );
                    }
                    Err(_) => {
                        send_event(
                            &reader_events,
                            TerminalEvent::Error {
                                summary: "The terminal output stream closed unexpectedly."
                                    .to_owned(),
                            },
                        );
                        break;
                    }
                }
            }
        });

    let wait_events = events.clone();
    let _ = thread::Builder::new()
        .name("pideck-terminal-wait".to_owned())
        .spawn(move || match child.wait() {
            Ok(status) => send_event(
                &wait_events,
                TerminalEvent::Exited {
                    code: status.exit_code(),
                },
            ),
            Err(_) => send_event(
                &wait_events,
                TerminalEvent::Error {
                    summary: "The terminal process status could not be read.".to_owned(),
                },
            ),
        });

    while !shutdown_requested.load(Ordering::Acquire) {
        let command = match commands.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match command {
            TerminalCommand::Write(bytes) => {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    send_event(
                        &events,
                        TerminalEvent::Error {
                            summary: "Input could not be sent to the terminal.".to_owned(),
                        },
                    );
                    break;
                }
            }
            TerminalCommand::Resize(size) => {
                if pair.master.resize(size.as_pty()).is_err() {
                    send_event(
                        &events,
                        TerminalEvent::Error {
                            summary: "The terminal could not be resized.".to_owned(),
                        },
                    );
                }
            }
            TerminalCommand::Shutdown => break,
        }
    }

    drop(writer);
    let _ = killer.kill();
}

fn send_event(events: &Sender<TerminalEvent>, event: TerminalEvent) {
    let _ = events.send_blocking(event);
}

fn default_shell_label() -> String {
    #[cfg(windows)]
    let shell = std::env::var_os("ComSpec")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cmd.exe"));

    #[cfg(not(windows))]
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));

    shell
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Shell")
        .to_owned()
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn terminal_size_is_bounded_for_platform_safety() {
        assert_eq!(
            TerminalSize::new(0, 0),
            TerminalSize::new(MIN_ROWS, MIN_COLS)
        );
        assert_eq!(
            TerminalSize::new(u16::MAX, u16::MAX),
            TerminalSize::new(MAX_ROWS, MAX_COLS)
        );
        assert_eq!(TerminalSize::default().rows, DEFAULT_ROWS);
        assert_eq!(TerminalSize::default().cols, DEFAULT_COLS);
    }

    #[test]
    fn default_shell_label_is_human_readable() {
        let label = default_shell_label();
        assert!(!label.is_empty());
        assert!(!label.contains(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn missing_workspace_reports_recoverable_error() {
        let (commands, command_rx) = mpsc::sync_channel(1);
        let (event_tx, events) = async_channel::bounded(2);
        drop(commands);
        run_terminal(
            PathBuf::from("definitely-missing-terminal-workspace"),
            TerminalSize::default(),
            command_rx,
            Arc::new(AtomicBool::new(false)),
            event_tx,
        );
        assert_eq!(
            events.recv_blocking().unwrap(),
            TerminalEvent::Error {
                summary: "The workspace folder is unavailable.".to_owned(),
            }
        );
    }

    #[cfg(windows)]
    fn wait_for_terminal_marker(
        worker: &TerminalWorker,
        engine: &mut super::super::terminal_engine::TerminalEngine,
        marker: &str,
        deadline: Instant,
    ) -> String {
        let events = worker.events();
        while Instant::now() < deadline {
            match events.try_recv() {
                Ok(TerminalEvent::Output(bytes)) => {
                    let response = engine.feed(&bytes);
                    if !response.is_empty() {
                        assert!(worker.write_bytes(response));
                    }
                    let text = engine.visible_text();
                    if text.lines().any(|line| line.trim() == marker) {
                        return text;
                    }
                }
                Ok(TerminalEvent::Error { summary }) => panic!("terminal failed: {summary}"),
                Ok(TerminalEvent::Started { .. } | TerminalEvent::Exited { .. }) => {}
                Err(async_channel::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(async_channel::TryRecvError::Closed) => break,
            }
        }
        panic!("The terminal did not render the expected output line: {marker}");
    }

    #[cfg(windows)]
    #[test]
    fn real_conpty_shell_accepts_input_and_streams_output() {
        let workspace = std::env::current_dir().unwrap();
        let worker = TerminalWorker::spawn(workspace, TerminalSize::new(8, 80));
        let mut engine = super::super::terminal_engine::TerminalEngine::new(8, 80);
        assert!(worker.write_line("echo PIDECK_TERMINAL_READY"));
        wait_for_terminal_marker(
            &worker,
            &mut engine,
            "PIDECK_TERMINAL_READY",
            Instant::now() + Duration::from_secs(15),
        );
        // Exercise a real PTY resize and subsequent output, using the same process.
        assert!(worker.resize(TerminalSize::new(12, 96)));
        engine.resize(12, 96);
        assert!(worker.write_line("echo PIDECK_TERMINAL_RESIZED"));
        wait_for_terminal_marker(
            &worker,
            &mut engine,
            "PIDECK_TERMINAL_RESIZED",
            Instant::now() + Duration::from_secs(15),
        );
        assert!(worker.write_line("exit"));
    }

    #[cfg(windows)]
    #[test]
    fn multiple_conpty_workers_keep_output_isolated() {
        let workspace = std::env::current_dir().unwrap();
        let first = TerminalWorker::spawn(workspace.clone(), TerminalSize::new(8, 80));
        let second = TerminalWorker::spawn(workspace, TerminalSize::new(8, 80));
        let mut first_engine = super::super::terminal_engine::TerminalEngine::new(8, 80);
        let mut second_engine = super::super::terminal_engine::TerminalEngine::new(8, 80);
        assert!(first.write_line("echo PIDECK_FIRST_TERMINAL"));
        assert!(second.write_line("echo PIDECK_SECOND_TERMINAL"));
        let first_text = wait_for_terminal_marker(
            &first,
            &mut first_engine,
            "PIDECK_FIRST_TERMINAL",
            Instant::now() + Duration::from_secs(15),
        );
        let second_text = wait_for_terminal_marker(
            &second,
            &mut second_engine,
            "PIDECK_SECOND_TERMINAL",
            Instant::now() + Duration::from_secs(15),
        );
        assert!(!first_text.contains("PIDECK_SECOND_TERMINAL"));
        assert!(!second_text.contains("PIDECK_FIRST_TERMINAL"));
        assert!(first.write_line("exit"));
        assert!(second.write_line("echo PIDECK_SECOND_STILL_RUNNING"));
        wait_for_terminal_marker(
            &second,
            &mut second_engine,
            "PIDECK_SECOND_STILL_RUNNING",
            Instant::now() + Duration::from_secs(15),
        );
        assert!(second.write_line("exit"));
    }
}
