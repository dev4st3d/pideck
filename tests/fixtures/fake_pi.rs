use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.first().is_some_and(|value| value == "--descendant") {
        descendant(Path::new(&arguments[1]));
        return;
    }

    let executable_directory = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .expect("fake executable directory");
    if arguments.iter().any(|value| value == "--version") {
        if executable_directory.join("probe-hang").exists() {
            spawn_descendant(&executable_directory);
            loop {
                thread::sleep(Duration::from_secs(1));
            }
        }
        let version = fs::read_to_string(executable_directory.join("fake-version.txt"))
            .unwrap_or_else(|_| "0.80.10".to_owned());
        println!("{}", version.trim());
        return;
    }
    if arguments.iter().any(|value| value == "--help") {
        if executable_directory.join("missing-capability").exists() {
            println!("--mode <mode> rpc");
        } else {
            println!(
                "--mode <mode> rpc --approve --no-approve --no-session \
                 --session <path|id> --session-id <id> --session-dir <dir> \
                 --no-extensions --extension <path> --no-skills --skill <path> \
                 --no-prompt-templates --prompt-template <path> --no-themes \
                 --theme <path> --no-context-files --offline --no-tools"
            );
        }
        return;
    }

    let cwd = env::current_dir().expect("fake cwd");
    fs::write(
        cwd.join("launch-args.txt"),
        arguments
            .iter()
            .map(|argument| argument.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("record launch arguments");
    if let Some(value) = env::var_os("FAKE_PI_ENVIRONMENT") {
        fs::write(cwd.join("launch-environment.txt"), value.to_string_lossy().as_bytes())
            .expect("record launch environment");
    }
    let mode = fs::read_to_string(cwd.join("fake-mode.txt"))
        .unwrap_or_else(|_| "normal".to_owned());

    match mode.trim() {
        "normal" => normal(&cwd),
        "exit-normal" => {
            println!("{{\"type\":\"fake_ready\"}}");
        }
        "fail" => {
            println!("{{\"type\":\"fake_output\"}}");
            eprintln!("synthetic child failure");
            std::process::exit(23);
        }
        "stderr-flood" => {
            let mut stderr = BufWriter::new(io::stderr().lock());
            for index in 0..20_000 {
                writeln!(stderr, "diagnostic line {index:05} xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
                    .expect("flood stderr");
            }
            writeln!(stderr, "Authorization: Bearer private-test-token")
                .expect("write sensitive diagnostic");
            stderr.flush().expect("flush stderr");
            normal(&cwd);
        }
        "early-eof" => {
            close_standard_handle(StandardHandle::Stdout);
            thread::sleep(Duration::from_secs(30));
        }
        "broken-stdin" => {
            close_standard_handle(StandardHandle::Stdin);
            println!("{{\"type\":\"fake_ready\"}}");
            thread::sleep(Duration::from_secs(30));
        }
        "ignore" => ignore_with_descendant(&cwd),
        "root-exit-descendant" => {
            spawn_descendant(&cwd);
            println!("{{\"type\":\"fake_ready\"}}");
        }
        "stdout-flood" => {
            let chunk = "x".repeat(32 * 1024);
            for _ in 0..128 {
                println!("{chunk}");
            }
            thread::sleep(Duration::from_secs(30));
        }
        mode if mode.starts_with("rpc-") => rpc_script(&cwd, mode),
        other => panic!("unknown fake mode: {other}"),
    }
}

fn normal(cwd: &Path) {
    println!("{{\"type\":\"fake_ready\"}}");
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).expect("read stdin");
    fs::write(cwd.join("stdin.txt"), input).expect("record stdin");
}

fn rpc_script(cwd: &Path, mode: &str) {
    let (sender, receiver) = mpsc::channel();
    let reader_cwd = cwd.to_path_buf();
    thread::spawn(move || {
        let mut input = BufReader::new(io::stdin());
        let mut line = String::new();
        loop {
            line.clear();
            match input.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let mut transcript = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(reader_cwd.join("rpc-input.jsonl"))
                        .expect("open RPC transcript");
                    transcript
                        .write_all(line.as_bytes())
                        .expect("record RPC input");
                    if let Some(record_type) = json_string_field(&line, "type") {
                        fs::write(
                            reader_cwd.join(format!("seen-{record_type}.txt")),
                            json_string_field(&line, "id").unwrap_or_default(),
                        )
                        .expect("write RPC marker");
                    }
                    if sender.send(line.clone()).is_err() {
                        return;
                    }
                }
            }
        }
    });

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let mut delayed_reads = Vec::new();
    let mut readiness_complete = false;
    let mut post_readiness_commands = 0_u32;

    while let Ok(line) = receiver.recv() {
        let Some(record_type) = json_string_field(&line, "type") else {
            continue;
        };
        if record_type == "extension_ui_response" {
            continue;
        }
        let id = json_string_field(&line, "id").unwrap_or_default();

        if !readiness_complete && record_type == "get_state" {
            let response = response_for(&record_type, &id);
            if mode == "rpc-readiness-missing" {
                continue;
            }
            if mode == "rpc-fragmented" {
                write_fragmented(&mut output, &response);
            } else {
                write_record(&mut output, &response);
            }
            readiness_complete = true;
            fs::write(cwd.join("readiness-complete.txt"), &id).expect("write readiness marker");
            if matches!(mode, "rpc-generation" | "rpc-normal") {
                let label = fs::read_to_string(cwd.join("generation-label.txt"))
                    .unwrap_or_else(|_| "default".to_owned());
                write_record(
                    &mut output,
                    &format!(
                        "{{\"type\":\"connection_marker\",\"label\":\"{}\"}}",
                        escape_json(label.trim())
                    ),
                );
                fs::write(cwd.join("generation-marker-sent.txt"), label.trim())
                    .expect("write generation marker signal");
            }
            continue;
        }

        post_readiness_commands = post_readiness_commands.saturating_add(1);
        match mode {
            "rpc-out-of-order" if is_read_command(&record_type) => {
                delayed_reads.push((record_type, id));
                if delayed_reads.len() == 2 {
                    write_record(&mut output, "{\"type\":\"agent_start\"}");
                    for (command, request_id) in delayed_reads.drain(..).rev() {
                        write_record(&mut output, &response_for(&command, &request_id));
                    }
                }
            }
            "rpc-mutation" if record_type == "set_auto_compaction" => {
                wait_for_marker(&cwd.join("release-first-mutation.txt"));
                write_record(&mut output, &response_for(&record_type, &id));
            }
            "rpc-bypass" if record_type == "set_auto_compaction" => {
                wait_for_marker(&cwd.join("release-first-mutation.txt"));
                write_record(&mut output, &response_for(&record_type, &id));
            }
            "rpc-timeout" if record_type == "set_auto_compaction" => {}
            "rpc-prompt-disconnect" if record_type == "prompt" => {
                std::process::exit(42);
            }
            "rpc-read-timeout" if record_type == "get_messages" => {}
            "rpc-late-read" if record_type == "get_messages" => {
                thread::sleep(Duration::from_millis(350));
                write_record(&mut output, &response_for(&record_type, &id));
                fs::write(cwd.join("late-response-sent.txt"), &id)
                    .expect("write late response marker");
            }
            "rpc-parse-error" if post_readiness_commands >= 2 => {
                write_record(&mut output, "{not-json}");
            }
            "rpc-exit" if post_readiness_commands >= 2 => {
                std::process::exit(41);
            }
            "rpc-early-eof" if post_readiness_commands >= 2 => {
                close_standard_handle(StandardHandle::Stdout);
                thread::sleep(Duration::from_secs(30));
            }
            "rpc-parse-error" | "rpc-exit" | "rpc-early-eof" => {}
            "rpc-missing-id" => {
                write_record(
                    &mut output,
                    &format!(
                        "{{\"type\":\"response\",\"command\":\"{}\",\"success\":true,\"data\":{{\"messages\":[]}}}}",
                        escape_json(&record_type)
                    ),
                );
            }
            "rpc-unknown-id" => {
                write_record(
                    &mut output,
                    &response_for(&record_type, "unknown-request-id"),
                );
            }
            "rpc-stderr" => {
                eprintln!("private synthetic stderr content");
                write_record(&mut output, &response_for(&record_type, &id));
            }
            "rpc-fragmented" => {
                write_record(&mut output, "{\"type\":\"agent_start\"}");
                write_fragmented(&mut output, &response_for(&record_type, &id));
            }
            "rpc-ignore-shutdown" if record_type == "abort" => {
                loop {
                    thread::sleep(Duration::from_secs(1));
                }
            }
            "rpc-writer-failure" if post_readiness_commands >= 2 => {
                close_standard_handle(StandardHandle::Stdin);
                write_record(&mut output, "{\"type\":\"writer_closed\"}");
                loop {
                    thread::sleep(Duration::from_secs(1));
                }
            }
            "rpc-writer-failure" => {}
            _ => write_record(&mut output, &response_for(&record_type, &id)),
        }
    }
}

fn response_for(command: &str, id: &str) -> String {
    let data = match command {
        "get_state" => Some(
            "{\"model\":{\"id\":\"fake-model\",\"name\":\"Fake Model\",\"api\":\"fake-api\",\"provider\":\"fake-provider\",\"baseUrl\":\"https://invalid.example\",\"reasoning\":false,\"input\":[\"text\"],\"cost\":{\"input\":0.0,\"output\":0.0,\"cacheRead\":0.0,\"cacheWrite\":0.0},\"contextWindow\":100000,\"maxTokens\":4096},\"thinkingLevel\":\"medium\",\"isStreaming\":false,\"isCompacting\":false,\"steeringMode\":\"all\",\"followUpMode\":\"one-at-a-time\",\"sessionId\":\"fake-session\",\"autoCompactionEnabled\":true,\"messageCount\":0,\"pendingMessageCount\":0}",
        ),
        "get_messages" => Some("{\"messages\":[]}"),
        "get_commands" => Some("{\"commands\":[]}"),
        "get_available_models" => Some(
            "{\"models\":[{\"id\":\"fake-model\",\"name\":\"Fake Model\",\"api\":\"fake-api\",\"provider\":\"fake-provider\",\"baseUrl\":\"https://invalid.example\",\"reasoning\":false,\"input\":[\"text\"],\"cost\":{\"input\":0.0,\"output\":0.0,\"cacheRead\":0.0,\"cacheWrite\":0.0},\"contextWindow\":100000,\"maxTokens\":4096}]}",
        ),
        "get_session_stats" => Some(
            "{\"sessionId\":\"fake-session\",\"userMessages\":0,\"assistantMessages\":0,\"toolCalls\":0,\"toolResults\":0,\"totalMessages\":0,\"tokens\":{\"input\":0,\"output\":0,\"cacheRead\":0,\"cacheWrite\":0,\"total\":0},\"cost\":0.0,\"contextUsage\":{\"tokens\":0,\"contextWindow\":100000,\"percent\":0.0}}",
        ),
        "get_entries" => Some("{\"entries\":[],\"leafId\":null}"),
        "get_tree" => Some("{\"tree\":[],\"leafId\":null}"),
        "get_last_assistant_text" => Some("{\"text\":null}"),
        "get_fork_messages" => Some("{\"messages\":[]}"),
        "new_session" | "switch_session" | "clone" => Some("{\"cancelled\":false}"),
        "fork" => Some("{\"text\":\"synthetic\",\"cancelled\":false}"),
        _ => None,
    };
    match data {
        Some(data) => format!(
            "{{\"type\":\"response\",\"id\":\"{}\",\"command\":\"{}\",\"success\":true,\"data\":{data}}}",
            escape_json(id),
            escape_json(command)
        ),
        None => format!(
            "{{\"type\":\"response\",\"id\":\"{}\",\"command\":\"{}\",\"success\":true}}",
            escape_json(id),
            escape_json(command)
        ),
    }
}

fn is_read_command(command: &str) -> bool {
    matches!(
        command,
        "get_state"
            | "get_messages"
            | "get_commands"
            | "get_available_models"
            | "get_entries"
            | "get_tree"
            | "get_last_assistant_text"
            | "get_fork_messages"
    )
}

fn write_record(output: &mut impl Write, record: &str) {
    writeln!(output, "{record}").expect("write RPC output");
    output.flush().expect("flush RPC output");
}

fn write_fragmented(output: &mut impl Write, record: &str) {
    let bytes = record.as_bytes();
    let first = bytes.len() / 3;
    let second = first * 2;
    output.write_all(&bytes[..first]).expect("write first fragment");
    output.flush().expect("flush first fragment");
    thread::yield_now();
    output
        .write_all(&bytes[first..second])
        .expect("write second fragment");
    output.flush().expect("flush second fragment");
    thread::yield_now();
    output
        .write_all(&bytes[second..])
        .expect("write final fragment");
    output.write_all(b"\n").expect("terminate fragmented record");
    output.flush().expect("flush final fragment");
}

fn json_string_field(line: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\"");
    let mut remaining = &line[line.find(&needle)? + needle.len()..];
    remaining = remaining.trim_start();
    remaining = remaining.strip_prefix(':')?.trim_start();
    remaining = remaining.strip_prefix('"')?;
    let mut value = String::new();
    let mut escaped = false;
    for character in remaining.chars() {
        if escaped {
            value.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(value);
        } else {
            value.push(character);
        }
    }
    None
}

fn escape_json(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn wait_for_marker(path: &Path) {
    while !path.exists() {
        thread::sleep(Duration::from_millis(2));
    }
}

fn ignore_with_descendant(cwd: &Path) {
    spawn_descendant(cwd);
    println!("{{\"type\":\"fake_ready\"}}");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn spawn_descendant(cwd: &Path) {
    let heartbeat = cwd.join("descendant-heartbeat.txt");
    let child = Command::new(env::current_exe().expect("fake executable"))
        .arg("--descendant")
        .arg(&heartbeat)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn descendant");
    fs::write(cwd.join("descendant-pid.txt"), child.id().to_string())
        .expect("record descendant pid");
    for _ in 0..100 {
        if heartbeat.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("descendant did not start");
}

fn descendant(heartbeat: &Path) {
    let mut counter = 0_u64;
    loop {
        counter = counter.saturating_add(1);
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(heartbeat)
            .expect("open heartbeat");
        writeln!(file, "{counter}").expect("write heartbeat");
        thread::sleep(Duration::from_millis(20));
    }
}

enum StandardHandle {
    Stdin,
    Stdout,
}

#[cfg(windows)]
fn close_standard_handle(handle: StandardHandle) {
    type Handle = *mut core::ffi::c_void;
    const STD_INPUT_HANDLE: u32 = -10_i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11_i32 as u32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(kind: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
    }

    let kind = match handle {
        StandardHandle::Stdin => STD_INPUT_HANDLE,
        StandardHandle::Stdout => STD_OUTPUT_HANDLE,
    };
    // SAFETY: The fake deliberately closes its inherited standard handle to exercise fault paths.
    unsafe {
        CloseHandle(GetStdHandle(kind));
    }
}

#[cfg(unix)]
fn close_standard_handle(handle: StandardHandle) {
    unsafe extern "C" {
        fn close(file_descriptor: i32) -> i32;
    }

    let descriptor = match handle {
        StandardHandle::Stdin => 0,
        StandardHandle::Stdout => 1,
    };
    // SAFETY: The fake deliberately closes its standard descriptor to exercise fault paths.
    unsafe {
        close(descriptor);
    }
}

#[cfg(not(any(windows, unix)))]
fn close_standard_handle(_handle: StandardHandle) {}
