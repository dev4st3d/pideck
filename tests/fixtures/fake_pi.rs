use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
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
                 --theme <path> --no-context-files"
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
        other => panic!("unknown fake mode: {other}"),
    }
}

fn normal(cwd: &Path) {
    println!("{{\"type\":\"fake_ready\"}}");
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).expect("read stdin");
    fs::write(cwd.join("stdin.txt"), input).expect("record stdin");
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
    // SAFETY: The fake deliberately closes its inherited standard handle to exercise EOF paths.
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
    // SAFETY: The fake deliberately closes its standard descriptor to exercise EOF paths.
    unsafe {
        close(descriptor);
    }
}

#[cfg(not(any(windows, unix)))]
fn close_standard_handle(_handle: StandardHandle) {}
