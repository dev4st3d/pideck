use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    code: Option<i32>,
}

impl ExitStatus {
    #[cfg(test)]
    pub(crate) const fn from_code(code: Option<i32>) -> Self {
        Self { code }
    }

    pub fn code(self) -> Option<i32> {
        self.code
    }

    pub fn success(self) -> bool {
        self.code == Some(0)
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(formatter, "exit code {code}"),
            None => formatter.write_str("terminated without an exit code"),
        }
    }
}

pub(crate) struct SpawnedProcess {
    pub(crate) handle: ProcessHandle,
    pub(crate) stdin: Option<Box<dyn Write + Send>>,
    pub(crate) stdout: Option<Box<dyn Read + Send>>,
    pub(crate) stderr: Option<Box<dyn Read + Send>>,
}

pub(crate) struct ProcessHandle {
    inner: Mutex<ProcessInner>,
}

impl ProcessHandle {
    pub(crate) fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .try_wait()
    }

    pub(crate) fn wait_for(&self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .wait_for(timeout)
    }

    pub(crate) fn terminate(&self) -> io::Result<()> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .terminate()
    }
}

pub(crate) fn spawn_contained(
    executable: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    environment_overrides: &[(OsString, OsString)],
) -> io::Result<SpawnedProcess> {
    ProcessInner::spawn(
        executable,
        arguments,
        working_directory,
        environment_overrides,
    )
}

#[cfg(not(windows))]
struct ProcessInner {
    child: std::process::Child,
    process_group: i32,
}

#[cfg(not(windows))]
impl ProcessInner {
    fn spawn(
        executable: &Path,
        arguments: &[OsString],
        working_directory: &Path,
        environment_overrides: &[(OsString, OsString)],
    ) -> io::Result<SpawnedProcess> {
        use std::process::{Command, Stdio};

        let mut command = Command::new(executable);
        command
            .args(arguments)
            .current_dir(working_directory)
            .envs(environment_overrides.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        let mut child = command.spawn()?;
        let process_group = child.id() as i32;
        let stdin = child
            .stdin
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Write + Send>);
        let stdout = child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>);
        let stderr = child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>);

        Ok(SpawnedProcess {
            handle: ProcessHandle {
                inner: Mutex::new(Self {
                    child,
                    process_group,
                }),
            },
            stdin,
            stdout,
            stderr,
        })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait().map(|status| {
            status.map(|status| ExitStatus {
                code: status.code(),
            })
        })
    }

    fn wait_for(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }
            if started.elapsed() >= timeout {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn terminate(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            const SIGKILL: i32 = 9;
            unsafe extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }

            // A negative PID addresses the process group created before exec.
            let result = unsafe { kill(-self.process_group, SIGKILL) };
            if result == 0 || self.child.try_wait()?.is_some() {
                return Ok(());
            }
            return Err(io::Error::last_os_error());
        }
        #[cfg(not(unix))]
        {
            match self.child.kill() {
                Ok(()) => Ok(()),
                Err(error) if self.child.try_wait()?.is_some() => Ok(()),
                Err(error) => Err(error),
            }
        }
    }
}

#[cfg(not(windows))]
impl Drop for ProcessInner {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.terminate();
            let _ = self.child.wait();
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::{OsStr, OsString, c_void};
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
        InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
        UpdateProcThreadAttribute, WaitForSingleObject,
    };

    use super::{ExitStatus, ProcessHandle, SpawnedProcess};
    use std::sync::Mutex;

    pub(super) struct ProcessInner {
        process: OwnedHandle,
        job: OwnedHandle,
        exit_status: Option<ExitStatus>,
    }

    impl ProcessInner {
        pub(super) fn spawn(
            executable: &Path,
            arguments: &[OsString],
            working_directory: &Path,
            environment_overrides: &[(OsString, OsString)],
        ) -> io::Result<SpawnedProcess> {
            let job = create_kill_on_close_job().map_err(|error| context("create job", error))?;
            let (child_stdin, parent_stdin) = create_pipe_pair(PipeDirection::ParentWrites)
                .map_err(|error| context("create stdin pipe", error))?;
            let (parent_stdout, child_stdout) = create_pipe_pair(PipeDirection::ParentReads)
                .map_err(|error| context("create stdout pipe", error))?;
            let (parent_stderr, child_stderr) = create_pipe_pair(PipeDirection::ParentReads)
                .map_err(|error| context("create stderr pipe", error))?;

            let inherited_handles = [
                raw_handle(&child_stdin),
                raw_handle(&child_stdout),
                raw_handle(&child_stderr),
            ];
            let job_handle = raw_handle(&job);
            let mut attributes = ProcThreadAttributes::new(2)
                .map_err(|error| context("initialize process attributes", error))?;
            attributes
                .set_handle_list(&inherited_handles)
                .map_err(|error| context("set inherited pipe handles", error))?;
            attributes
                .set_job(&job_handle)
                .map_err(|error| context("set process job", error))?;

            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = raw_handle(&child_stdin);
            startup.StartupInfo.hStdOutput = raw_handle(&child_stdout);
            startup.StartupInfo.hStdError = raw_handle(&child_stderr);
            startup.lpAttributeList = attributes.as_ptr();

            let mut process_information: PROCESS_INFORMATION = unsafe { zeroed() };
            let application = wide_null(executable.as_os_str());
            let mut command_line = build_command_line(executable.as_os_str(), arguments);
            let current_directory = wide_null(working_directory.as_os_str());
            let mut environment = (!environment_overrides.is_empty())
                .then(|| build_environment_block(environment_overrides));
            let environment_pointer = environment
                .as_mut()
                .map_or(null_mut(), |block| block.as_mut_ptr().cast());
            let creation_flags = EXTENDED_STARTUPINFO_PRESENT
                | CREATE_NO_WINDOW
                | if environment.is_some() {
                    CREATE_UNICODE_ENVIRONMENT
                } else {
                    0
                };
            // SAFETY: Every pointer references initialized storage for the duration of the call.
            // The inherited-handle allowlist contains only the three child pipe ends, and the job
            // attribute assigns the process before its initial thread can run.
            let created = unsafe {
                CreateProcessW(
                    application.as_ptr(),
                    command_line.as_mut_ptr(),
                    null(),
                    null(),
                    1,
                    creation_flags,
                    environment_pointer,
                    current_directory.as_ptr(),
                    &startup.StartupInfo,
                    &mut process_information,
                )
            };
            if created == 0 {
                return Err(context("create process", io::Error::last_os_error()));
            }

            // SAFETY: CreateProcessW returned two newly owned, non-null handles.
            let process =
                unsafe { OwnedHandle::from_raw_handle(process_information.hProcess as RawHandle) };
            // SAFETY: The initial thread is already running and its handle is no longer needed.
            unsafe { CloseHandle(process_information.hThread) };
            drop(child_stdin);
            drop(child_stdout);
            drop(child_stderr);

            let stdin_file = File::from(parent_stdin);
            let stdout_file = File::from(parent_stdout);
            let stderr_file = File::from(parent_stderr);
            Ok(SpawnedProcess {
                handle: ProcessHandle {
                    inner: Mutex::new(Self {
                        process,
                        job,
                        exit_status: None,
                    }),
                },
                stdin: Some(Box::new(stdin_file) as Box<dyn Write + Send>),
                stdout: Some(Box::new(stdout_file) as Box<dyn Read + Send>),
                stderr: Some(Box::new(stderr_file) as Box<dyn Read + Send>),
            })
        }

        pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            if let Some(status) = self.exit_status {
                return Ok(Some(status));
            }
            // SAFETY: process is a valid process handle owned by this object.
            match unsafe { WaitForSingleObject(raw_handle(&self.process), 0) } {
                WAIT_TIMEOUT => Ok(None),
                WAIT_OBJECT_0 => {
                    let status = self.read_exit_status()?;
                    self.exit_status = Some(status);
                    Ok(Some(status))
                }
                _ => Err(io::Error::last_os_error()),
            }
        }

        pub(super) fn wait_for(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
            if let Some(status) = self.exit_status {
                return Ok(Some(status));
            }
            let milliseconds = timeout.as_millis().min((u32::MAX - 1) as u128) as u32;
            // SAFETY: process is a valid process handle owned by this object.
            match unsafe { WaitForSingleObject(raw_handle(&self.process), milliseconds) } {
                WAIT_TIMEOUT => Ok(None),
                WAIT_OBJECT_0 => {
                    let status = self.read_exit_status()?;
                    self.exit_status = Some(status);
                    Ok(Some(status))
                }
                _ => Err(io::Error::last_os_error()),
            }
        }

        pub(super) fn terminate(&mut self) -> io::Result<()> {
            // Terminate the job even when the root already exited: descendants may still hold
            // inherited resources and are part of the supervisor's ownership contract.
            // SAFETY: job is a valid job handle configured to contain the process tree.
            if unsafe { TerminateJobObject(raw_handle(&self.job), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn read_exit_status(&self) -> io::Result<ExitStatus> {
            let mut code = 0_u32;
            // SAFETY: process is valid and code points to writable storage.
            if unsafe { GetExitCodeProcess(raw_handle(&self.process), &mut code) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(ExitStatus {
                code: Some(code as i32),
            })
        }
    }

    impl Drop for ProcessInner {
        fn drop(&mut self) {
            if self.try_wait().ok().flatten().is_none() {
                let _ = self.terminate();
                let _ = self.wait_for(Duration::from_secs(2));
            }
        }
    }

    enum PipeDirection {
        ParentReads,
        ParentWrites,
    }

    fn create_pipe_pair(direction: PipeDirection) -> io::Result<(OwnedHandle, OwnedHandle)> {
        let mut read = null_mut();
        let mut write = null_mut();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        // SAFETY: output pointers and the security attributes are valid for this call.
        if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreatePipe returned two owned handles.
        let read = unsafe { OwnedHandle::from_raw_handle(read as RawHandle) };
        // SAFETY: CreatePipe returned two owned handles.
        let write = unsafe { OwnedHandle::from_raw_handle(write as RawHandle) };

        let parent = match direction {
            PipeDirection::ParentReads => &read,
            PipeDirection::ParentWrites => &write,
        };
        // SAFETY: parent is a valid pipe handle; clearing inheritance affects only this handle.
        if unsafe { SetHandleInformation(raw_handle(parent), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(match direction {
            PipeDirection::ParentReads => (read, write),
            PipeDirection::ParentWrites => (read, write),
        })
    }

    fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
        // SAFETY: null security/name pointers request an unnamed job with default security.
        let raw = unsafe { CreateJobObjectW(null(), null()) };
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned a newly owned handle.
        let job = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: limits points to a correctly sized initialized structure.
        if unsafe {
            SetInformationJobObject(
                raw_handle(&job),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    struct ProcThreadAttributes {
        storage: Vec<usize>,
    }

    impl ProcThreadAttributes {
        fn new(count: u32) -> io::Result<Self> {
            let mut bytes = 0_usize;
            // SAFETY: A null first call is the documented size query.
            unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes) };
            if bytes == 0 {
                return Err(io::Error::last_os_error());
            }
            let words = bytes.div_ceil(size_of::<usize>());
            let mut attributes = Self {
                storage: vec![0; words],
            };
            // SAFETY: storage is aligned and contains at least the requested byte count.
            if unsafe {
                InitializeProcThreadAttributeList(attributes.as_ptr(), count, 0, &mut bytes)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(attributes)
        }

        fn set_handle_list(&mut self, handles: &[HANDLE]) -> io::Result<()> {
            self.update(
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
            )
        }

        fn set_job(&mut self, job: &HANDLE) -> io::Result<()> {
            self.update(
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                (job as *const HANDLE).cast(),
                size_of::<HANDLE>(),
            )
        }

        fn update(
            &mut self,
            attribute: usize,
            value: *const c_void,
            size: usize,
        ) -> io::Result<()> {
            // SAFETY: the list was initialized and value points to initialized storage of size.
            if unsafe {
                UpdateProcThreadAttribute(
                    self.as_ptr(),
                    0,
                    attribute,
                    value,
                    size,
                    null_mut(),
                    null(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn as_ptr(&mut self) -> *mut c_void {
            self.storage.as_mut_ptr().cast()
        }
    }

    impl Drop for ProcThreadAttributes {
        fn drop(&mut self) {
            // SAFETY: successful construction initialized this list exactly once.
            unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
        }
    }

    fn context(operation: &str, error: io::Error) -> io::Error {
        io::Error::new(error.kind(), format!("{operation}: {error}"))
    }

    fn raw_handle(handle: &OwnedHandle) -> HANDLE {
        use std::os::windows::io::AsRawHandle;
        handle.as_raw_handle() as HANDLE
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    fn build_environment_block(overrides: &[(OsString, OsString)]) -> Vec<u16> {
        let mut variables = std::env::vars_os().collect::<Vec<_>>();
        for (override_name, override_value) in overrides {
            if let Some((_, value)) = variables.iter_mut().find(|(name, _)| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case(&override_name.to_string_lossy())
            }) {
                *value = override_value.clone();
            } else {
                variables.push((override_name.clone(), override_value.clone()));
            }
        }
        variables.sort_by_cached_key(|(name, _)| name.to_string_lossy().to_uppercase());

        let mut block = Vec::new();
        for (name, value) in variables {
            block.extend(name.encode_wide());
            block.push(b'=' as u16);
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }

    fn build_command_line(executable: &OsStr, arguments: &[OsString]) -> Vec<u16> {
        let mut command = Vec::new();
        append_quoted_argument(&mut command, executable);
        for argument in arguments {
            command.push(b' ' as u16);
            append_quoted_argument(&mut command, argument);
        }
        command.push(0);
        command
    }

    fn append_quoted_argument(command: &mut Vec<u16>, argument: &OsStr) {
        let encoded = argument.encode_wide().collect::<Vec<_>>();
        let needs_quotes = encoded.is_empty()
            || encoded
                .iter()
                .any(|character| matches!(*character, 0x20 | 0x09 | 0x22));
        if !needs_quotes {
            command.extend(encoded);
            return;
        }

        command.push(b'"' as u16);
        let mut backslashes = 0_usize;
        for character in encoded {
            if character == b'\\' as u16 {
                backslashes += 1;
                continue;
            }
            if character == b'"' as u16 {
                command.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
            } else {
                command.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
            }
            backslashes = 0;
            command.push(character);
        }
        command.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
        command.push(b'"' as u16);
    }
}

#[cfg(windows)]
use windows::ProcessInner;
