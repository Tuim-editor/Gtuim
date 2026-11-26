//! Safe, resource-limited local execution engine and runtime toolchain detector
//! for daily coding practice submissions.
//!
//! Security & resource boundary (local unprivileged execution):
//! - Each submission runs in an isolated, mode-0700 temporary directory that is
//!   removed via RAII on completion, failure, timeout, or cancellation.
//! - Untrusted execution receives a sanitized environment (`env_clear()` plus only
//!   `PATH`, temp `HOME`/`TMPDIR`, `LANG`, and `GTUIM_RESULTS_FILE`).
//! - Child processes run in their own session/process group (`setsid`), allowing
//!   the entire spawned process tree to be terminated with `SIGKILL` on timeout or cancel.
//! - OS `setrlimit` caps CPU time, file write size, open FDs, core dump size (0),
//!   and virtual/heap memory (plus runtime-native heap caps for Node/JVM/Go).
//! - Stdout and stderr are drained on bounded reader threads (64 KiB cap each) to
//!   prevent pipe deadlocks and memory exhaustion from print floods.
//! - Structured test results are written to a dedicated file (`GTUIM_RESULTS_FILE`),
//!   keeping user stdout/debug prints cleanly separated from grading.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};

use serde::Deserialize;

use super::catalog::{
    Exercise, ExerciseSignature, PracticeLanguage, TestInput,
};
use super::editor::MAX_SOURCE_BYTES;

static RUN_DIR_SEQ: AtomicU64 = AtomicU64::new(1);

const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_RESULTS_FILE_BYTES: usize = 256 * 1024;
const DEFAULT_COMPILE_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TsRunnerKind {
    None,
    NodeNative,
    Tsx,
    Bun,
    Deno,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolchainStatus {
    pub language: PracticeLanguage,
    pub available: bool,
    pub tool_label: String,
    pub binary_path: Option<PathBuf>,
    pub secondary_path: Option<PathBuf>,
    pub version: Option<String>,
    pub unavailable_reason: Option<String>,
    pub ts_runner: TsRunnerKind,
}

#[derive(Clone, Debug, Default)]
pub struct ToolchainRegistry {
    statuses: HashMap<PracticeLanguage, ToolchainStatus>,
}

impl ToolchainRegistry {
    /// Fast, non-spawning PATH scan for immediate initial UI rendering.
    pub fn detect_fast() -> Self {
        let mut statuses = HashMap::new();
        for &lang in PracticeLanguage::ALL {
            statuses.insert(lang, detect_language_fast(lang));
        }
        Self { statuses }
    }

    /// Full probe (intended for background thread) verifying versions and Node TS support.
    pub fn detect_full() -> Self {
        let mut statuses = HashMap::new();
        for &lang in PracticeLanguage::ALL {
            statuses.insert(lang, detect_language_full(lang));
        }
        Self { statuses }
    }

    pub fn get(&self, lang: PracticeLanguage) -> ToolchainStatus {
        self.statuses
            .get(&lang)
            .cloned()
            .unwrap_or_else(|| detect_language_fast(lang))
    }

    pub fn is_available(&self, lang: PracticeLanguage) -> bool {
        self.statuses.get(&lang).is_some_and(|s| s.available)
    }

    pub fn available_count(&self) -> usize {
        self.statuses.values().filter(|s| s.available).count()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RunLimits {
    pub compile_timeout: Duration,
    pub exec_timeout: Duration,
    pub exec_cpu_secs: u64,
    pub compile_cpu_secs: u64,
    pub exec_memory_bytes: u64,
    pub exec_fsize_bytes: u64,
    pub compile_fsize_bytes: u64,
    pub exec_nofile: u64,
    pub compile_nofile: u64,
    pub max_output_bytes: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            compile_timeout: DEFAULT_COMPILE_TIMEOUT,
            exec_timeout: DEFAULT_EXEC_TIMEOUT,
            exec_cpu_secs: 4,
            compile_cpu_secs: 20,
            exec_memory_bytes: 512 * 1024 * 1024,
            exec_fsize_bytes: 2 * 1024 * 1024,
            compile_fsize_bytes: 64 * 1024 * 1024,
            exec_nofile: 64,
            compile_nofile: 256,
            max_output_bytes: MAX_OUTPUT_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmissionVerdict {
    Accepted,
    WrongAnswer,
    CompileError,
    RuntimeError,
    TimeLimitExceeded,
    ToolchainUnavailable,
    Cancelled,
    InternalError,
}

impl SubmissionVerdict {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Accepted => "Accepted",
            Self::WrongAnswer => "Wrong Answer",
            Self::CompileError => "Compile Error",
            Self::RuntimeError => "Runtime Error",
            Self::TimeLimitExceeded => "Time Limit Exceeded",
            Self::ToolchainUnavailable => "Toolchain Unavailable",
            Self::Cancelled => "Cancelled",
            Self::InternalError => "Runner Error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseOutcome {
    pub index: usize,
    pub hidden: bool,
    pub passed: bool,
    pub input_display: String,
    pub expected_display: String,
    pub actual_display: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SubmissionReport {
    pub language: PracticeLanguage,
    pub exercise_id: String,
    pub verdict: SubmissionVerdict,
    pub summary: String,
    pub elapsed_ms: u64,
    pub passed_count: usize,
    pub total_count: usize,
    pub cases: Vec<CaseOutcome>,
    pub stdout: String,
    pub stderr: String,
    pub output_truncated: bool,
}

/// Evaluate a user's solution against all test cases of `exercise` under `limits`.
/// Safe to invoke on a background worker thread; honors `cancel` promptly.
pub fn evaluate_submission(
    exercise: &Exercise,
    language: PracticeLanguage,
    source: &str,
    toolchain: &ToolchainStatus,
    limits: RunLimits,
    cancel: &Arc<AtomicBool>,
) -> SubmissionReport {
    let total_count = exercise.tests.len();
    let started = Instant::now();

    if cancel.load(Ordering::Relaxed) {
        return simple_report(
            exercise,
            language,
            SubmissionVerdict::Cancelled,
            "Submission cancelled.".into(),
            started,
            String::new(),
            String::new(),
            false,
        );
    }

    if !toolchain.available {
        let reason = toolchain
            .unavailable_reason
            .clone()
            .unwrap_or_else(|| language.install_hint().to_string());
        return simple_report(
            exercise,
            language,
            SubmissionVerdict::ToolchainUnavailable,
            reason,
            started,
            String::new(),
            String::new(),
            false,
        );
    }

    if source.len() > MAX_SOURCE_BYTES {
        return simple_report(
            exercise,
            language,
            SubmissionVerdict::CompileError,
            format!(
                "Source exceeds maximum size limit ({} bytes > {} bytes).",
                source.len(),
                MAX_SOURCE_BYTES
            ),
            started,
            String::new(),
            String::new(),
            false,
        );
    }

    let workspace = match IsolatedRunDir::create() {
        Ok(dir) => dir,
        Err(err) => {
            return simple_report(
                exercise,
                language,
                SubmissionVerdict::InternalError,
                format!("Failed to create isolated run workspace: {err}"),
                started,
                String::new(),
                String::new(),
                false,
            );
        }
    };

    let results_path = workspace.path().join("results.jsonl");

    // Materialize user source + test harness, compile if needed, and build run command.
    let exec_spec = match prepare_execution(
        workspace.path(),
        exercise,
        language,
        source,
        toolchain,
        limits,
        cancel,
    ) {
        Ok(spec) => spec,
        Err(PrepFailure::CompileError { message, stderr }) => {
            return simple_report(
                exercise,
                language,
                SubmissionVerdict::CompileError,
                message,
                started,
                String::new(),
                stderr,
                false,
            );
        }
        Err(PrepFailure::TimedOut(msg)) => {
            return simple_report(
                exercise,
                language,
                SubmissionVerdict::TimeLimitExceeded,
                msg,
                started,
                String::new(),
                String::new(),
                false,
            );
        }
        Err(PrepFailure::Cancelled) => {
            return simple_report(
                exercise,
                language,
                SubmissionVerdict::Cancelled,
                "Submission cancelled during build.".into(),
                started,
                String::new(),
                String::new(),
                false,
            );
        }
        Err(PrepFailure::Internal(msg)) => {
            return simple_report(
                exercise,
                language,
                SubmissionVerdict::InternalError,
                msg,
                started,
                String::new(),
                String::new(),
                false,
            );
        }
    };

    // Execute untrusted solution + harness in isolated dir with sanitized env & OS limits.
    let mut cmd = Command::new(&exec_spec.program);
    cmd.args(&exec_spec.args)
        .current_dir(workspace.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();

    if let Ok(path_env) = std::env::var("PATH") {
        cmd.env("PATH", path_env);
    }
    cmd.env("HOME", workspace.path());
    cmd.env("TMPDIR", workspace.path());
    cmd.env("LANG", "C.UTF-8");
    cmd.env("LC_ALL", "C.UTF-8");
    cmd.env("GTUIM_RESULTS_FILE", &results_path);
    for (k, v) in &exec_spec.extra_env {
        cmd.env(k, v);
    }

    let rlimit_mode = RlimitProfile {
        cpu_secs: limits.exec_cpu_secs,
        fsize_bytes: limits.exec_fsize_bytes,
        nofile: limits.exec_nofile,
        as_bytes: if exec_spec.apply_rlimit_as {
            Some(limits.exec_memory_bytes)
        } else {
            None
        },
        data_bytes: None,
    };

    let outcome = run_command_bounded(cmd, limits.exec_timeout, limits.max_output_bytes, rlimit_mode, cancel);
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let run = match outcome {
        Ok(r) => r,
        Err(err) => {
            return simple_report(
                exercise,
                language,
                SubmissionVerdict::InternalError,
                format!("Failed to launch runtime: {err}"),
                started,
                String::new(),
                String::new(),
                false,
            );
        }
    };

    if run.cancelled {
        return SubmissionReport {
            language,
            exercise_id: exercise.id.to_string(),
            verdict: SubmissionVerdict::Cancelled,
            summary: "Submission cancelled.".into(),
            elapsed_ms,
            passed_count: 0,
            total_count,
            cases: Vec::new(),
            stdout: run.stdout,
            stderr: run.stderr,
            output_truncated: run.truncated,
        };
    }

    let parsed_rows = read_results_jsonl(&results_path);
    let (cases, passed_count) = grade_cases(exercise, &parsed_rows);

    if run.timed_out || run.signal == Some(libc::SIGXCPU) {
        return SubmissionReport {
            language,
            exercise_id: exercise.id.to_string(),
            verdict: SubmissionVerdict::TimeLimitExceeded,
            summary: format!(
                "Execution exceeded local time limit ({:.1}s wall / {}s CPU). Process group terminated.",
                limits.exec_timeout.as_secs_f32(),
                limits.exec_cpu_secs
            ),
            elapsed_ms,
            passed_count,
            total_count,
            cases,
            stdout: run.stdout,
            stderr: run.stderr,
            output_truncated: run.truncated,
        };
    }

    if !run.success {
        let detail = if let Some(sig) = run.signal {
            format!("Process terminated by signal {sig} ({passed_count}/{total_count} cases completed).")
        } else if let Some(code) = run.exit_code {
            format!("Process exited with status {code} ({passed_count}/{total_count} cases completed).")
        } else {
            format!("Runtime failed ({passed_count}/{total_count} cases completed).")
        };
        return SubmissionReport {
            language,
            exercise_id: exercise.id.to_string(),
            verdict: SubmissionVerdict::RuntimeError,
            summary: detail,
            elapsed_ms,
            passed_count,
            total_count,
            cases,
            stdout: run.stdout,
            stderr: run.stderr,
            output_truncated: run.truncated,
        };
    }

    if parsed_rows.is_empty() && total_count > 0 {
        return SubmissionReport {
            language,
            exercise_id: exercise.id.to_string(),
            verdict: SubmissionVerdict::RuntimeError,
            summary: "No test results were produced. Ensure `solve` is defined with the expected signature.".into(),
            elapsed_ms,
            passed_count: 0,
            total_count,
            cases,
            stdout: run.stdout,
            stderr: run.stderr,
            output_truncated: run.truncated,
        };
    }

    let any_case_err = cases.iter().any(|c| c.error.is_some());
    let verdict = if passed_count == total_count {
        SubmissionVerdict::Accepted
    } else if any_case_err {
        SubmissionVerdict::RuntimeError
    } else {
        SubmissionVerdict::WrongAnswer
    };

    let summary = match verdict {
        SubmissionVerdict::Accepted => {
            format!("All {passed_count}/{total_count} test cases passed in {elapsed_ms} ms.")
        }
        SubmissionVerdict::WrongAnswer => {
            format!("{passed_count}/{total_count} test cases passed.")
        }
        SubmissionVerdict::RuntimeError => {
            format!("Runtime error during test cases ({passed_count}/{total_count} passed).")
        }
        _ => verdict.label().to_string(),
    };

    SubmissionReport {
        language,
        exercise_id: exercise.id.to_string(),
        verdict,
        summary,
        elapsed_ms,
        passed_count,
        total_count,
        cases,
        stdout: run.stdout,
        stderr: run.stderr,
        output_truncated: run.truncated,
    }
}

fn simple_report(
    exercise: &Exercise,
    language: PracticeLanguage,
    verdict: SubmissionVerdict,
    summary: String,
    started: Instant,
    stdout: String,
    stderr: String,
    output_truncated: bool,
) -> SubmissionReport {
    SubmissionReport {
        language,
        exercise_id: exercise.id.to_string(),
        verdict,
        summary,
        elapsed_ms: started.elapsed().as_millis() as u64,
        passed_count: 0,
        total_count: exercise.tests.len(),
        cases: Vec::new(),
        stdout,
        stderr,
        output_truncated,
    }
}

// ── Isolated Run Workspace (RAII cleanup) ───────────────────────────────────

struct IsolatedRunDir {
    path: PathBuf,
}

impl IsolatedRunDir {
    fn create() -> io::Result<Self> {
        let seq = RUN_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "gtuim-practice-run-{}-{seq}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        #[cfg(unix)]
        {
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
        }
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for IsolatedRunDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ── Bounded Subprocess Execution with Process-Group Kill & OS Rlimits ───────

#[derive(Clone, Copy)]
struct RlimitProfile {
    cpu_secs: u64,
    fsize_bytes: u64,
    nofile: u64,
    as_bytes: Option<u64>,
    data_bytes: Option<u64>,
}

struct BoundedRunOutput {
    success: bool,
    exit_code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    cancelled: bool,
    stdout: String,
    stderr: String,
    truncated: bool,
}

fn run_command_bounded(
    mut cmd: Command,
    timeout: Duration,
    max_output_bytes: usize,
    rlimits: RlimitProfile,
    cancel: &Arc<AtomicBool>,
) -> io::Result<BoundedRunOutput> {
    #[cfg(target_os = "linux")]
    let parent_pid = unsafe { libc::getpid() };

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(move || {
            // Isolate into a new session / process group so the entire spawned tree
            // can be killed atomically via kill(-pgid, SIGKILL).
            libc::setsid();
            // Ensure the child dies immediately if the host/supervising process exits
            // or is killed unexpectedly.
            #[cfg(target_os = "linux")]
            {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                if libc::getppid() != parent_pid {
                    libc::_exit(128 + libc::SIGKILL);
                }
            }
            set_rlimit(libc::RLIMIT_CORE, 0);
            set_rlimit(libc::RLIMIT_CPU, rlimits.cpu_secs);
            set_rlimit(libc::RLIMIT_FSIZE, rlimits.fsize_bytes);
            set_rlimit(libc::RLIMIT_NOFILE, rlimits.nofile);
            if let Some(bytes) = rlimits.as_bytes {
                set_rlimit(libc::RLIMIT_AS, bytes);
            }
            if let Some(bytes) = rlimits.data_bytes {
                set_rlimit(libc::RLIMIT_DATA, bytes);
            }
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;
    let pid = child.id();

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_done = Arc::new(AtomicBool::new(stdout_pipe.is_none()));
    let stderr_done = Arc::new(AtomicBool::new(stderr_pipe.is_none()));

    let out_flag = stdout_done.clone();
    let stdout_thread = thread::spawn(move || {
        let res = read_bounded_stream(stdout_pipe, max_output_bytes);
        out_flag.store(true, Ordering::Release);
        res
    });
    let err_flag = stderr_done.clone();
    let stderr_thread = thread::spawn(move || {
        let res = read_bounded_stream(stderr_pipe, max_output_bytes);
        err_flag.store(true, Ordering::Release);
        res
    });

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut cancelled = false;
    let mut exit_status = None;

    // Monitor both the direct child AND any descendants holding stdout/stderr pipes
    // under the exact same wall-clock deadline and cancellation flag.
    loop {
        if exit_status.is_none() {
            if let Some(st) = child.try_wait()? {
                exit_status = Some(st);
            }
        }
        let pipes_closed =
            stdout_done.load(Ordering::Acquire) && stderr_done.load(Ordering::Acquire);
        if exit_status.is_some() && pipes_closed {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            kill_process_group(pid);
            if exit_status.is_none() {
                exit_status = Some(child.wait()?);
            }
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            kill_process_group(pid);
            if exit_status.is_none() {
                exit_status = Some(child.wait()?);
            }
            break;
        }
        thread::sleep(Duration::from_millis(8));
    }

    // Always clean up any surviving descendants in the process group on every exit path
    // (including when the direct child exited normally after spawning detached children).
    kill_process_group(pid);

    let status = match exit_status {
        Some(st) => st,
        None => child.wait()?,
    };

    let (stdout_bytes, out_trunc) = stdout_thread.join().unwrap_or_default();
    let (stderr_bytes, err_trunc) = stderr_thread.join().unwrap_or_default();

    #[cfg(unix)]
    let signal = status.signal();
    #[cfg(not(unix))]
    let signal = None;

    Ok(BoundedRunOutput {
        success: status.success(),
        exit_code: status.code(),
        signal,
        timed_out,
        cancelled,
        stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
        truncated: out_trunc || err_trunc,
    })
}

#[cfg(unix)]
// glibc uses an unsigned enum for resource IDs; Darwin and other Unix
// platforms expose an int. Keep the function signature native to libc.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type RlimitResource = libc::__rlimit_resource_t;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type RlimitResource = libc::c_int;

unsafe fn set_rlimit(resource: RlimitResource, limit: u64) {
    let mut current = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if libc::getrlimit(resource, &mut current) == 0 {
        let clamped = if current.rlim_max == libc::RLIM_INFINITY {
            limit
        } else {
            limit.min(current.rlim_max)
        };
        let desired = libc::rlimit {
            rlim_cur: clamped,
            rlim_max: clamped,
        };
        let _ = libc::setrlimit(resource, &desired);
    }
}

fn kill_process_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        let pgid = -(pid as libc::pid_t);
        let _ = libc::kill(pgid, libc::SIGKILL);
        let _ = libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

fn read_bounded_stream<R: Read>(stream: Option<R>, max_bytes: usize) -> (Vec<u8>, bool) {
    let Some(mut reader) = stream else {
        return (Vec::new(), false);
    };
    let mut out = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if out.len() < max_bytes {
                    let keep = (max_bytes - out.len()).min(n);
                    out.extend_from_slice(&buf[..keep]);
                    if keep < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (out, truncated)
}

// ── Results Parsing & Grading ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RawCaseResult {
    i: usize,
    val: Option<String>,
    err: Option<String>,
}

fn read_results_jsonl(path: &Path) -> HashMap<usize, RawCaseResult> {
    let mut map = HashMap::new();
    let Ok(file) = fs::File::open(path) else {
        return map;
    };
    let mut content = String::new();
    let _ = file
        .take(MAX_RESULTS_FILE_BYTES as u64)
        .read_to_string(&mut content);
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(row) = serde_json::from_str::<RawCaseResult>(trimmed) {
            map.insert(row.i, row);
        }
    }
    map
}

fn grade_cases(
    exercise: &Exercise,
    rows: &HashMap<usize, RawCaseResult>,
) -> (Vec<CaseOutcome>, usize) {
    let mut outcomes = Vec::with_capacity(exercise.tests.len());
    let mut passed_count = 0usize;

    for (idx, tc) in exercise.tests.iter().enumerate() {
        let expected_canon = tc.expected.canonical();
        let expected_display = tc.expected.display();
        let input_display = tc.input.display();

        if let Some(row) = rows.get(&idx) {
            if let Some(err) = &row.err {
                outcomes.push(CaseOutcome {
                    index: idx,
                    hidden: tc.hidden,
                    passed: false,
                    input_display,
                    expected_display,
                    actual_display: "<error>".into(),
                    error: Some(err.clone()),
                });
            } else {
                let actual = row.val.clone().unwrap_or_default();
                let passed = actual == expected_canon;
                if passed {
                    passed_count += 1;
                }
                outcomes.push(CaseOutcome {
                    index: idx,
                    hidden: tc.hidden,
                    passed,
                    input_display,
                    expected_display,
                    actual_display: actual,
                    error: None,
                });
            }
        } else {
            outcomes.push(CaseOutcome {
                index: idx,
                hidden: tc.hidden,
                passed: false,
                input_display,
                expected_display,
                actual_display: "<not run>".into(),
                error: Some("Case did not produce a result".into()),
            });
        }
    }

    (outcomes, passed_count)
}

// ── Toolchain Detection ─────────────────────────────────────────────────────

fn find_in_path(names: &[&str]) -> Option<(String, PathBuf)> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for &name in names {
            let candidate = dir.join(name);
            if is_executable(&candidate) {
                return Some((name.to_string(), candidate));
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn probe_version_line(bin: &Path, args: &[&str]) -> Option<String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = run_command_bounded(
        cmd,
        Duration::from_secs(2),
        4096,
        RlimitProfile {
            cpu_secs: 3,
            fsize_bytes: 1024 * 1024,
            nofile: 64,
            as_bytes: None,
            data_bytes: None,
        },
        &cancel,
    )
    .ok()?;
    if !out.success {
        return None;
    }
    let combined = if !out.stdout.trim().is_empty() {
        out.stdout
    } else {
        out.stderr
    };
    combined
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

fn parse_node_major_minor(version_line: &str) -> Option<(u32, u32)> {
    let trimmed = version_line.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    Some((major, minor))
}

fn detect_language_fast(lang: PracticeLanguage) -> ToolchainStatus {
    match lang {
        PracticeLanguage::Python => match find_in_path(&["python3", "python"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "python3", lang.install_hint()),
        },
        PracticeLanguage::JavaScript => match find_in_path(&["node"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "node", lang.install_hint()),
        },
        PracticeLanguage::TypeScript => {
            if let Some((name, path)) = find_in_path(&["node"]) {
                available_status(lang, name, path, None, None, TsRunnerKind::NodeNative)
            } else if let Some((name, path)) = find_in_path(&["tsx"]) {
                available_status(lang, name, path, None, None, TsRunnerKind::Tsx)
            } else if let Some((name, path)) = find_in_path(&["bun"]) {
                available_status(lang, name, path, None, None, TsRunnerKind::Bun)
            } else if let Some((name, path)) = find_in_path(&["deno"]) {
                available_status(lang, name, path, None, None, TsRunnerKind::Deno)
            } else {
                unavailable_status(lang, "node / tsx", lang.install_hint())
            }
        }
        PracticeLanguage::Rust => match find_in_path(&["rustc"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "rustc", lang.install_hint()),
        },
        PracticeLanguage::C => match find_in_path(&["cc", "gcc", "clang"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "cc / gcc / clang", lang.install_hint()),
        },
        PracticeLanguage::Cpp => match find_in_path(&["g++", "clang++", "c++"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "g++ / clang++", lang.install_hint()),
        },
        PracticeLanguage::Zig => match find_in_path(&["zig"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "zig", lang.install_hint()),
        },
        PracticeLanguage::Go => match find_in_path(&["go"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "go", lang.install_hint()),
        },
        PracticeLanguage::Java => match (find_in_path(&["javac"]), find_in_path(&["java"])) {
            (Some((_, javac)), Some((_, java))) => {
                available_status(lang, "javac + java".into(), javac, Some(java), None, TsRunnerKind::None)
            }
            _ => unavailable_status(lang, "javac + java", lang.install_hint()),
        },
        PracticeLanguage::Ruby => match find_in_path(&["ruby"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "ruby", lang.install_hint()),
        },
        PracticeLanguage::Lua => match find_in_path(&["lua", "luajit"]) {
            Some((name, path)) => available_status(lang, name, path, None, None, TsRunnerKind::None),
            None => unavailable_status(lang, "lua", lang.install_hint()),
        },
    }
}

fn detect_language_full(lang: PracticeLanguage) -> ToolchainStatus {
    match lang {
        PracticeLanguage::Python => {
            if let Some((name, path)) = find_in_path(&["python3", "python"]) {
                if let Some(ver) = probe_version_line(&path, &["--version"]) {
                    if ver.contains("Python 3") {
                        return available_status(lang, name, path, None, Some(ver), TsRunnerKind::None);
                    }
                }
            }
            unavailable_status(lang, "python3", lang.install_hint())
        }
        PracticeLanguage::JavaScript => {
            if let Some((name, path)) = find_in_path(&["node"]) {
                if let Some(ver) = probe_version_line(&path, &["--version"]) {
                    return available_status(lang, name, path, None, Some(format!("Node {ver}")), TsRunnerKind::None);
                }
            }
            unavailable_status(lang, "node", lang.install_hint())
        }
        PracticeLanguage::TypeScript => {
            if let Some((_, node_path)) = find_in_path(&["node"]) {
                if let Some(ver) = probe_version_line(&node_path, &["--version"]) {
                    if let Some((maj, min)) = parse_node_major_minor(&ver) {
                        if maj > 22 || (maj == 22 && min >= 6) {
                            return available_status(
                                lang,
                                "node (native TS)".into(),
                                node_path,
                                None,
                                Some(format!("Node {ver}")),
                                TsRunnerKind::NodeNative,
                            );
                        }
                    }
                }
            }
            if let Some((name, path)) = find_in_path(&["tsx"]) {
                let ver = probe_version_line(&path, &["--version"]);
                return available_status(lang, name, path, None, ver, TsRunnerKind::Tsx);
            }
            if let Some((name, path)) = find_in_path(&["bun"]) {
                let ver = probe_version_line(&path, &["--version"]);
                return available_status(lang, name, path, None, ver, TsRunnerKind::Bun);
            }
            if let Some((name, path)) = find_in_path(&["deno"]) {
                let ver = probe_version_line(&path, &["--version"]);
                return available_status(lang, name, path, None, ver, TsRunnerKind::Deno);
            }
            unavailable_status(lang, "node >=22.6 / tsx", lang.install_hint())
        }
        PracticeLanguage::Rust => {
            if let Some((name, path)) = find_in_path(&["rustc"]) {
                if let Some(ver) = probe_version_line(&path, &["--version"]) {
                    return available_status(lang, name, path, None, Some(ver), TsRunnerKind::None);
                }
            }
            unavailable_status(lang, "rustc", lang.install_hint())
        }
        PracticeLanguage::C => {
            if let Some((name, path)) = find_in_path(&["cc", "gcc", "clang"]) {
                if let Some(ver) = probe_version_line(&path, &["--version"]) {
                    return available_status(lang, name, path, None, Some(ver), TsRunnerKind::None);
                }
            }
            unavailable_status(lang, "cc / gcc / clang", lang.install_hint())
        }
        PracticeLanguage::Cpp => {
            if let Some((name, path)) = find_in_path(&["g++", "clang++", "c++"]) {
                if let Some(ver) = probe_version_line(&path, &["--version"]) {
                    return available_status(lang, name, path, None, Some(ver), TsRunnerKind::None);
                }
            }
            unavailable_status(lang, "g++ / clang++", lang.install_hint())
        }
        PracticeLanguage::Zig => {
            if let Some((name, path)) = find_in_path(&["zig"]) {
                if let Some(ver) = probe_version_line(&path, &["version"]) {
                    return available_status(
                        lang,
                        name,
                        path,
                        None,
                        Some(format!("Zig {ver}")),
                        TsRunnerKind::None,
                    );
                }
            }
            unavailable_status(lang, "zig", lang.install_hint())
        }
        PracticeLanguage::Go => {
            if let Some((name, path)) = find_in_path(&["go"]) {
                if let Some(ver) = probe_version_line(&path, &["version"]) {
                    return available_status(lang, name, path, None, Some(ver), TsRunnerKind::None);
                }
            }
            unavailable_status(lang, "go", lang.install_hint())
        }
        PracticeLanguage::Java => {
            if let (Some((_, javac)), Some((_, java))) =
                (find_in_path(&["javac"]), find_in_path(&["java"]))
            {
                if let Some(ver) = probe_version_line(&javac, &["-version"]) {
                    return available_status(
                        lang,
                        "javac + java".into(),
                        javac,
                        Some(java),
                        Some(ver),
                        TsRunnerKind::None,
                    );
                }
            }
            unavailable_status(lang, "javac + java", lang.install_hint())
        }
        PracticeLanguage::Ruby => {
            if let Some((name, path)) = find_in_path(&["ruby"]) {
                if let Some(ver) = probe_version_line(&path, &["--version"]) {
                    return available_status(lang, name, path, None, Some(ver), TsRunnerKind::None);
                }
            }
            unavailable_status(lang, "ruby", lang.install_hint())
        }
        PracticeLanguage::Lua => {
            if let Some((name, path)) = find_in_path(&["lua", "luajit"]) {
                if let Some(ver) = probe_version_line(&path, &["-v"]) {
                    return available_status(lang, name, path, None, Some(ver), TsRunnerKind::None);
                }
            }
            unavailable_status(lang, "lua", lang.install_hint())
        }
    }
}

fn available_status(
    language: PracticeLanguage,
    tool_label: String,
    binary_path: PathBuf,
    secondary_path: Option<PathBuf>,
    version: Option<String>,
    ts_runner: TsRunnerKind,
) -> ToolchainStatus {
    ToolchainStatus {
        language,
        available: true,
        tool_label,
        binary_path: Some(binary_path),
        secondary_path,
        version,
        unavailable_reason: None,
        ts_runner,
    }
}

fn unavailable_status(
    language: PracticeLanguage,
    tool_label: &str,
    hint: &str,
) -> ToolchainStatus {
    ToolchainStatus {
        language,
        available: false,
        tool_label: tool_label.to_string(),
        binary_path: None,
        secondary_path: None,
        version: None,
        unavailable_reason: Some(format!(
            "{} toolchain (`{tool_label}`) was not found on PATH. {hint}",
            language.label()
        )),
        ts_runner: TsRunnerKind::None,
    }
}

// ── Per-Language Harness Generation & Compilation ───────────────────────────

struct ExecutionSpec {
    program: PathBuf,
    args: Vec<String>,
    extra_env: Vec<(String, String)>,
    /// Use `RLIMIT_AS` for native/compact runtimes; use `RLIMIT_DATA` + runtime heap cap
    /// for V8 (Node/Bun/Deno), JVM (Java), and Go which reserve multi-GB virtual cages.
    apply_rlimit_as: bool,
}

enum PrepFailure {
    CompileError { message: String, stderr: String },
    TimedOut(String),
    Cancelled,
    Internal(String),
}

fn prepare_execution(
    dir: &Path,
    exercise: &Exercise,
    language: PracticeLanguage,
    source: &str,
    toolchain: &ToolchainStatus,
    limits: RunLimits,
    cancel: &Arc<AtomicBool>,
) -> Result<ExecutionSpec, PrepFailure> {
    let bin = toolchain
        .binary_path
        .clone()
        .ok_or_else(|| PrepFailure::Internal("Missing resolved tool binary".into()))?;

    match language {
        PracticeLanguage::Python => {
            write_text(&dir.join("solution.py"), source)?;
            let harness = build_python_harness(exercise);
            write_text(&dir.join("harness.py"), &harness)?;
            Ok(ExecutionSpec {
                program: bin,
                args: vec!["-I".into(), "-B".into(), "harness.py".into()],
                extra_env: Vec::new(),
                apply_rlimit_as: true,
            })
        }
        PracticeLanguage::JavaScript => {
            let sol = format!(
                "{source}\n;module.exports = {{ solve: (typeof solve === 'function' ? solve : undefined) }};\n"
            );
            write_text(&dir.join("solution.js"), &sol)?;
            let harness = build_js_harness(exercise, false);
            write_text(&dir.join("harness.js"), &harness)?;
            Ok(ExecutionSpec {
                program: bin,
                args: vec!["--max-old-space-size=192".into(), "harness.js".into()],
                extra_env: Vec::new(),
                apply_rlimit_as: false,
            })
        }
        PracticeLanguage::TypeScript => {
            let sol = format!("{source}\nexport {{ solve }};\n");
            write_text(&dir.join("solution.mts"), &sol)?;
            let harness = build_js_harness(exercise, true);
            write_text(&dir.join("harness.mts"), &harness)?;
            let args = match toolchain.ts_runner {
                TsRunnerKind::NodeNative => {
                    vec!["--max-old-space-size=192".into(), "harness.mts".into()]
                }
                TsRunnerKind::Tsx | TsRunnerKind::Bun => vec!["harness.mts".into()],
                TsRunnerKind::Deno => vec![
                    "run".into(),
                    "--allow-read=.".into(),
                    "--allow-write=.".into(),
                    "--allow-env=GTUIM_RESULTS_FILE".into(),
                    "harness.mts".into(),
                ],
                TsRunnerKind::None => {
                    return Err(PrepFailure::Internal("No TypeScript runner available".into()));
                }
            };
            Ok(ExecutionSpec {
                program: bin,
                args,
                extra_env: Vec::new(),
                apply_rlimit_as: false,
            })
        }
        PracticeLanguage::Rust => {
            write_text(&dir.join("solution.rs"), source)?;
            let harness = build_rust_harness(exercise);
            write_text(&dir.join("harness.rs"), &harness)?;
            let runner_bin = dir.join("runner");
            let mut cmd = Command::new(&bin);
            cmd.args([
                "--edition",
                "2021",
                "-C",
                "opt-level=1",
                "harness.rs",
                "-o",
                "runner",
            ])
            .current_dir(dir);
            run_compile_step(cmd, limits, cancel, "Rust compilation failed")?;
            Ok(ExecutionSpec {
                program: runner_bin,
                args: Vec::new(),
                extra_env: Vec::new(),
                apply_rlimit_as: true,
            })
        }
        PracticeLanguage::C => {
            write_text(&dir.join("solution.c"), source)?;
            let harness = build_c_harness(exercise);
            write_text(&dir.join("harness.c"), &harness)?;
            let runner_bin = dir.join("runner");
            let mut cmd = Command::new(&bin);
            cmd.args([
                "-O1",
                "-std=c11",
                "-Wall",
                "solution.c",
                "harness.c",
                "-o",
                "runner",
                "-lm",
            ])
            .current_dir(dir);
            run_compile_step(cmd, limits, cancel, "C compilation failed")?;
            Ok(ExecutionSpec {
                program: runner_bin,
                args: Vec::new(),
                extra_env: Vec::new(),
                apply_rlimit_as: true,
            })
        }
        PracticeLanguage::Cpp => {
            write_text(&dir.join("solution.cpp"), source)?;
            let harness = build_cpp_harness(exercise);
            write_text(&dir.join("harness.cpp"), &harness)?;
            let runner_bin = dir.join("runner");
            let mut cmd = Command::new(&bin);
            cmd.args([
                "-O1",
                "-std=c++17",
                "-Wall",
                "solution.cpp",
                "harness.cpp",
                "-o",
                "runner",
            ])
            .current_dir(dir);
            run_compile_step(cmd, limits, cancel, "C++ compilation failed")?;
            Ok(ExecutionSpec {
                program: runner_bin,
                args: Vec::new(),
                extra_env: Vec::new(),
                apply_rlimit_as: true,
            })
        }
        PracticeLanguage::Zig => {
            write_text(&dir.join("solution.zig"), source)?;
            let harness = build_zig_harness(exercise);
            write_text(&dir.join("harness.zig"), &harness)?;
            let global_cache = std::env::temp_dir().join(format!(
                "gtuim-practice-zig-cache-{}",
                std::process::id()
            ));
            let local_cache = dir.join("zig-cache");
            let _ = fs::create_dir_all(&global_cache);
            let _ = fs::create_dir_all(&local_cache);
            #[cfg(unix)]
            {
                let _ = fs::set_permissions(&global_cache, fs::Permissions::from_mode(0o700));
            }
            let runner_bin = dir.join("runner");
            let mut cmd = Command::new(&bin);
            cmd.args([
                "build-exe",
                "harness.zig",
                "-femit-bin=runner",
                "--color",
                "off",
            ])
            .current_dir(dir)
            .env("ZIG_GLOBAL_CACHE_DIR", &global_cache)
            .env("ZIG_LOCAL_CACHE_DIR", &local_cache);
            run_compile_step(cmd, limits, cancel, "Zig compilation failed")?;
            Ok(ExecutionSpec {
                program: runner_bin,
                args: Vec::new(),
                extra_env: Vec::new(),
                apply_rlimit_as: true,
            })
        }
        PracticeLanguage::Go => {
            write_text(&dir.join("go.mod"), "module practice\n\ngo 1.20\n")?;
            write_text(&dir.join("solution.go"), source)?;
            let harness = build_go_harness(exercise);
            write_text(&dir.join("main_harness.go"), &harness)?;
            let gocache = dir.join("gocache");
            let gopath = dir.join("gopath");
            let _ = fs::create_dir_all(&gocache);
            let _ = fs::create_dir_all(&gopath);
            let runner_bin = dir.join("runner");
            let mut cmd = Command::new(&bin);
            cmd.args(["build", "-o", "runner", "."])
                .current_dir(dir)
                .env("GOCACHE", &gocache)
                .env("GOPATH", &gopath)
                .env("GOPROXY", "off");
            run_compile_step(cmd, limits, cancel, "Go build failed")?;
            Ok(ExecutionSpec {
                program: runner_bin,
                args: Vec::new(),
                extra_env: vec![("GOMEMLIMIT".into(), "192MiB".into())],
                apply_rlimit_as: false,
            })
        }
        PracticeLanguage::Java => {
            let java_bin = toolchain
                .secondary_path
                .clone()
                .ok_or_else(|| PrepFailure::Internal("Missing `java` binary".into()))?;
            write_text(&dir.join("Solution.java"), source)?;
            let harness = build_java_harness(exercise);
            write_text(&dir.join("Main.java"), &harness)?;
            let mut cmd = Command::new(&bin);
            cmd.args(["Solution.java", "Main.java"]).current_dir(dir);
            run_compile_step(cmd, limits, cancel, "Java compilation failed")?;
            Ok(ExecutionSpec {
                program: java_bin,
                args: vec![
                    "-Xmx192m".into(),
                    "-XX:+ExitOnOutOfMemoryError".into(),
                    "-cp".into(),
                    ".".into(),
                    "Main".into(),
                ],
                extra_env: Vec::new(),
                apply_rlimit_as: false,
            })
        }
        PracticeLanguage::Ruby => {
            write_text(&dir.join("solution.rb"), source)?;
            let harness = build_ruby_harness(exercise);
            write_text(&dir.join("harness.rb"), &harness)?;
            Ok(ExecutionSpec {
                program: bin,
                args: vec!["--disable-gems".into(), "harness.rb".into()],
                extra_env: Vec::new(),
                apply_rlimit_as: true,
            })
        }
        PracticeLanguage::Lua => {
            write_text(&dir.join("solution.lua"), source)?;
            let harness = build_lua_harness(exercise);
            write_text(&dir.join("harness.lua"), &harness)?;
            Ok(ExecutionSpec {
                program: bin,
                args: vec!["harness.lua".into()],
                extra_env: Vec::new(),
                apply_rlimit_as: true,
            })
        }
    }
}

fn write_text(path: &Path, content: &str) -> Result<(), PrepFailure> {
    fs::write(path, content).map_err(|e| PrepFailure::Internal(format!("Failed writing {}: {e}", path.display())))
}

fn run_compile_step(
    mut cmd: Command,
    limits: RunLimits,
    cancel: &Arc<AtomicBool>,
    error_title: &str,
) -> Result<(), PrepFailure> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let run = run_command_bounded(
        cmd,
        limits.compile_timeout,
        limits.max_output_bytes,
        RlimitProfile {
            cpu_secs: limits.compile_cpu_secs,
            fsize_bytes: limits.compile_fsize_bytes,
            nofile: limits.compile_nofile,
            as_bytes: None,
            data_bytes: None,
        },
        cancel,
    )
    .map_err(|e| PrepFailure::Internal(format!("Failed to invoke compiler: {e}")))?;

    if run.cancelled {
        return Err(PrepFailure::Cancelled);
    }
    if run.timed_out {
        return Err(PrepFailure::TimedOut(format!(
            "Compilation exceeded {:.0}s limit.",
            limits.compile_timeout.as_secs_f32()
        )));
    }
    if !run.success {
        let diag = if !run.stderr.trim().is_empty() {
            run.stderr
        } else {
            run.stdout
        };
        return Err(PrepFailure::CompileError {
            message: error_title.to_string(),
            stderr: diag,
        });
    }
    Ok(())
}

// ── Harness Builders ────────────────────────────────────────────────────────

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn build_python_harness(ex: &Exercise) -> String {
    let mut out = String::from(
        r#"import json, os, sys
sys.path.insert(0, '.')
import solution

def fmt_val(sig, v):
    if sig == 'bool':
        if not isinstance(v, bool):
            raise TypeError(f'expected bool, got {type(v).__name__}')
        return 'true' if v else 'false'
    if sig == 'int':
        if isinstance(v, bool) or not isinstance(v, int):
            raise TypeError(f'expected int, got {type(v).__name__}')
        return str(v)
    if sig == 'str':
        if not isinstance(v, str):
            raise TypeError(f'expected str, got {type(v).__name__}')
        return json.dumps(v, ensure_ascii=False)
    if sig == 'vec':
        if not isinstance(v, (list, tuple)):
            raise TypeError(f'expected list[int], got {type(v).__name__}')
        items = []
        for x in v:
            if isinstance(x, bool) or not isinstance(x, int):
                raise TypeError('list elements must be int')
            items.append(str(x))
        return '[' + ','.join(items) + ']'
    if sig == 'pair':
        if not isinstance(v, (list, tuple)) or len(v) != 2:
            raise TypeError('expected 2-element pair [i, j]')
        a, b = v[0], v[1]
        if isinstance(a, bool) or isinstance(b, bool) or not isinstance(a, int) or not isinstance(b, int):
            raise TypeError('pair elements must be int')
        return f'[{a},{b}]'
    raise ValueError('unknown signature')

res_path = os.environ['GTUIM_RESULTS_FILE']
with open(res_path, 'w', encoding='utf-8') as f:
"#,
    );
    let ret_kind = signature_ret_kind(ex.signature);
    for (idx, tc) in ex.tests.iter().enumerate() {
        let call = match &tc.input {
            TestInput::Int(n) => format!("solution.solve({n})"),
            TestInput::Str(s) => format!("solution.solve({})", json_str(s)),
            TestInput::TwoStrs(a, b) => format!("solution.solve({}, {})", json_str(a), json_str(b)),
            TestInput::IntVec(v) => format!("solution.solve({v:?})"),
            TestInput::IntVecAndInt(v, k) => format!("solution.solve({v:?}, {k})"),
        };
        out.push_str(&format!(
            "    try:\n        __v = {call}\n        f.write(json.dumps({{'i': {idx}, 'val': fmt_val('{ret_kind}', __v), 'err': None}}) + '\\n')\n    except Exception as __e:\n        f.write(json.dumps({{'i': {idx}, 'val': None, 'err': f'{{type(__e).__name__}}: {{__e}}'}} ) + '\\n')\n    f.flush()\n"
        ));
    }
    out
}

fn build_js_harness(ex: &Exercise, is_ts_esm: bool) -> String {
    let mut out = if is_ts_esm {
        String::from(
            "import * as fs from 'node:fs';\n\
             import { solve } from './solution.mts';\n",
        )
    } else {
        String::from(
            "const fs = require('node:fs');\n\
             const { solve } = require('./solution.js');\n",
        )
    };
    out.push_str(
        "function fmtVal(sig: any, v: any) {\n\
         \x20 if (sig === 'bool') {\n\
         \x20   if (typeof v !== 'boolean') throw new TypeError('expected boolean');\n\
         \x20   return v ? 'true' : 'false';\n\
         \x20 }\n\
         \x20 if (sig === 'int') {\n\
         \x20   if (typeof v !== 'number' || !Number.isInteger(v)) throw new TypeError('expected integer number');\n\
         \x20   return String(v);\n\
         \x20 }\n\
         \x20 if (sig === 'str') {\n\
         \x20   if (typeof v !== 'string') throw new TypeError('expected string');\n\
         \x20   return JSON.stringify(v);\n\
         \x20 }\n\
         \x20 if (sig === 'vec') {\n\
         \x20   if (!Array.isArray(v)) throw new TypeError('expected number[]');\n\
         \x20   const parts = [];\n\
         \x20   for (const x of v) {\n\
         \x20     if (typeof x !== 'number' || !Number.isInteger(x)) throw new TypeError('expected integer elements');\n\
         \x20     parts.push(String(x));\n\
         \x20   }\n\
         \x20   return '[' + parts.join(',') + ']';\n\
         \x20 }\n\
         \x20 if (sig === 'pair') {\n\
         \x20   if (!Array.isArray(v) || v.length !== 2) throw new TypeError('expected [i, j] pair');\n\
         \x20   if (!Number.isInteger(v[0]) || !Number.isInteger(v[1])) throw new TypeError('pair elements must be integers');\n\
         \x20   return '[' + String(v[0]) + ',' + String(v[1]) + ']';\n\
         \x20 }\n\
         \x20 throw new Error('unknown signature');\n\
         }\n",
    );
    if !is_ts_esm {
        out = out.replace("function fmtVal(sig: any, v: any)", "function fmtVal(sig, v)");
    }
    out.push_str(
        "const resPath = process.env.GTUIM_RESULTS_FILE;\n\
         const fd = fs.openSync(resPath, 'w');\n\
         function record(obj) {\n\
         \x20 fs.writeSync(fd, JSON.stringify(obj) + '\\n');\n\
         }\n",
    );
    if is_ts_esm {
        out = out
            .replace("const resPath = process.env.GTUIM_RESULTS_FILE;", "const resPath = process.env.GTUIM_RESULTS_FILE!;")
            .replace("function record(obj)", "function record(obj: unknown)");
    }
    let ret_kind = signature_ret_kind(ex.signature);
    for (idx, tc) in ex.tests.iter().enumerate() {
        let call = match &tc.input {
            TestInput::Int(n) => format!("solve({n})"),
            TestInput::Str(s) => format!("solve({})", json_str(s)),
            TestInput::TwoStrs(a, b) => format!("solve({}, {})", json_str(a), json_str(b)),
            TestInput::IntVec(v) => format!("solve({v:?})"),
            TestInput::IntVecAndInt(v, k) => format!("solve({v:?}, {k})"),
        };
        out.push_str(&format!(
            "try {{\n\
             \x20 const __v = {call};\n\
             \x20 record({{ i: {idx}, val: fmtVal('{ret_kind}', __v), err: null }});\n\
             }} catch (__e) {{\n\
             \x20 record({{ i: {idx}, val: null, err: String(__e) }});\n\
             }}\n"
        ));
    }
    out.push_str("fs.closeSync(fd);\n");
    out
}

fn build_rust_harness(ex: &Exercise) -> String {
    let mut out = String::from(
        "mod solution;\n\
         use std::fs::File;\n\
         use std::io::Write;\n\
         use std::panic::{catch_unwind, AssertUnwindSafe};\n\
         \n\
         fn json_escape(s: &str) -> String {\n\
             let mut out = String::with_capacity(s.len() + 2);\n\
             out.push('\"');\n\
             for ch in s.chars() {\n\
                 match ch {\n\
                     '\"' => out.push_str(\"\\\\\\\"\"),\n\
                     '\\\\' => out.push_str(\"\\\\\\\\\"),\n\
                     '\\n' => out.push_str(\"\\\\n\"),\n\
                     '\\r' => out.push_str(\"\\\\r\"),\n\
                     '\\t' => out.push_str(\"\\\\t\"),\n\
                     c if (c as u32) < 0x20 => out.push_str(&format!(\"\\\\u{:04x}\", c as u32)),\n\
                     c => out.push(c),\n\
                 }\n\
             }\n\
             out.push('\"');\n\
             out\n\
         }\n\
         \n\
         fn write_ok(f: &mut File, i: usize, val: &str) {\n\
             let _ = writeln!(f, \"{{\\\"i\\\":{},\\\"val\\\":{},\\\"err\\\":null}}\", i, json_escape(val));\n\
             let _ = f.flush();\n\
         }\n\
         \n\
         fn write_err(f: &mut File, i: usize, err: &str) {\n\
             let _ = writeln!(f, \"{{\\\"i\\\":{},\\\"val\\\":null,\\\"err\\\":{}}}\", i, json_escape(err));\n\
             let _ = f.flush();\n\
         }\n\
         \n\
         fn main() {\n\
             let path = std::env::var(\"GTUIM_RESULTS_FILE\").expect(\"results file\");\n\
             let mut f = File::create(path).expect(\"create results\");\n",
    );

    for (idx, tc) in ex.tests.iter().enumerate() {
        let expr = match &tc.input {
            TestInput::Int(n) => format!("solution::solve({n}i64)"),
            TestInput::Str(s) => format!("solution::solve({})", json_str(s)),
            TestInput::TwoStrs(a, b) => format!("solution::solve({}, {})", json_str(a), json_str(b)),
            TestInput::IntVec(v) => {
                let elems: Vec<String> = v.iter().map(|x| format!("{x}i64")).collect();
                format!("solution::solve(&[{}])", elems.join(", "))
            }
            TestInput::IntVecAndInt(v, k) => {
                let elems: Vec<String> = v.iter().map(|x| format!("{x}i64")).collect();
                format!("solution::solve(&[{}], {k}i64)", elems.join(", "))
            }
        };
        let fmt_expr = match ex.signature {
            ExerciseSignature::IntToInt
            | ExerciseSignature::StrToInt
            | ExerciseSignature::IntVecToInt
            | ExerciseSignature::IntVecIntToInt => "v.to_string()".to_string(),
            ExerciseSignature::StrToBool | ExerciseSignature::TwoStrsToBool => {
                "if v { \"true\".to_string() } else { \"false\".to_string() }".to_string()
            }
            ExerciseSignature::StrToStr => "json_escape(&v)".to_string(),
            ExerciseSignature::IntVecToIntVec => {
                "format!(\"[{}]\", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(\",\"))"
                    .to_string()
            }
            ExerciseSignature::IntVecIntToPair => "format!(\"[{},{}]\", v.0, v.1)".to_string(),
        };
        out.push_str(&format!(
            "    match catch_unwind(AssertUnwindSafe(|| {{ let v = {expr}; {fmt_expr} }})) {{\n\
             \x20       Ok(s) => write_ok(&mut f, {idx}, &s),\n\
             \x20       Err(_) => write_err(&mut f, {idx}, \"panic during execution\"),\n\
             \x20   }}\n"
        ));
    }
    out.push_str("}\n");
    out
}

fn build_c_harness(ex: &Exercise) -> String {
    let proto = match ex.signature {
        ExerciseSignature::IntToInt => "extern long long solve(long long n);",
        ExerciseSignature::StrToBool => "extern bool solve(const char *s);",
        ExerciseSignature::StrToInt => "extern long long solve(const char *s);",
        ExerciseSignature::StrToStr => "extern void solve(const char *s, char *out, size_t out_cap);",
        ExerciseSignature::TwoStrsToBool => "extern bool solve(const char *a, const char *b);",
        ExerciseSignature::IntVecToInt => "extern long long solve(const long long *nums, size_t len);",
        ExerciseSignature::IntVecIntToInt => {
            "extern long long solve(const long long *nums, size_t len, long long k);"
        }
        ExerciseSignature::IntVecToIntVec => {
            "extern size_t solve(const long long *nums, size_t len, long long *out);"
        }
        ExerciseSignature::IntVecIntToPair => {
            "extern void solve(const long long *nums, size_t len, long long k, long long *out_a, long long *out_b);"
        }
    };

    let mut out = format!(
        "#include <stdbool.h>\n\
         #include <stddef.h>\n\
         #include <stdio.h>\n\
         #include <stdlib.h>\n\
         #include <string.h>\n\
         \n\
         {proto}\n\
         \n\
         static void write_json_str(FILE *f, const char *s) {{\n\
             fputc('\"', f);\n\
             for (const unsigned char *p = (const unsigned char *)s; *p; ++p) {{\n\
                 unsigned char c = *p;\n\
                 if (c == '\"') fputs(\"\\\\\\\"\", f);\n\
                 else if (c == '\\\\') fputs(\"\\\\\\\\\", f);\n\
                 else if (c == '\\n') fputs(\"\\\\n\", f);\n\
                 else if (c == '\\r') fputs(\"\\\\r\", f);\n\
                 else if (c == '\\t') fputs(\"\\\\t\", f);\n\
                 else if (c < 0x20) fprintf(f, \"\\\\u%04x\", c);\n\
                 else fputc(c, f);\n\
             }}\n\
             fputc('\"', f);\n\
         }}\n\
         \n\
         static void write_record(FILE *f, size_t idx, const char *val) {{\n\
             fprintf(f, \"{{\\\"i\\\":%zu,\\\"val\\\":\", idx);\n\
             write_json_str(f, val);\n\
             fputs(\",\\\"err\\\":null}}\\n\", f);\n\
             fflush(f);\n\
         }}\n\
         \n\
         int main(void) {{\n\
             const char *path = getenv(\"GTUIM_RESULTS_FILE\");\n\
             if (!path) return 2;\n\
             FILE *f = fopen(path, \"w\");\n\
             if (!f) return 3;\n"
    );

    for (idx, tc) in ex.tests.iter().enumerate() {
        out.push_str("    {\n");
        match (&ex.signature, &tc.input) {
            (ExerciseSignature::IntToInt, TestInput::Int(n)) => {
                out.push_str(&format!(
                    "        long long v = solve({n}LL);\n\
                     \x20       char buf[64]; snprintf(buf, sizeof(buf), \"%lld\", v);\n\
                     \x20       write_record(f, {idx}, buf);\n"
                ));
            }
            (ExerciseSignature::StrToBool, TestInput::Str(s)) => {
                out.push_str(&format!(
                    "        bool v = solve({});\n\
                     \x20       write_record(f, {idx}, v ? \"true\" : \"false\");\n",
                    json_str(s)
                ));
            }
            (ExerciseSignature::StrToInt, TestInput::Str(s)) => {
                out.push_str(&format!(
                    "        long long v = solve({});\n\
                     \x20       char buf[64]; snprintf(buf, sizeof(buf), \"%lld\", v);\n\
                     \x20       write_record(f, {idx}, buf);\n",
                    json_str(s)
                ));
            }
            (ExerciseSignature::StrToStr, TestInput::Str(s)) => {
                out.push_str(&format!(
                    "        char out_buf[65536]; memset(out_buf, 0, sizeof(out_buf));\n\
                     \x20       solve({}, out_buf, sizeof(out_buf));\n\
                     \x20       out_buf[sizeof(out_buf) - 1] = '\\0';\n\
                     \x20       fprintf(f, \"{{\\\"i\\\":{idx},\\\"val\\\":\");\n\
                     \x20       char esc_buf[131072]; size_t pos = 0;\n\
                     \x20       esc_buf[pos++] = '\"';\n\
                     \x20       for (const unsigned char *p = (const unsigned char *)out_buf; *p && pos + 8 < sizeof(esc_buf); ++p) {{\n\
                     \x20           unsigned char c = *p;\n\
                     \x20           if (c == '\"') {{ esc_buf[pos++]='\\\\'; esc_buf[pos++]='\"'; }}\n\
                     \x20           else if (c == '\\\\') {{ esc_buf[pos++]='\\\\'; esc_buf[pos++]='\\\\'; }}\n\
                     \x20           else if (c == '\\n') {{ esc_buf[pos++]='\\\\'; esc_buf[pos++]='n'; }}\n\
                     \x20           else if (c == '\\r') {{ esc_buf[pos++]='\\\\'; esc_buf[pos++]='r'; }}\n\
                     \x20           else if (c == '\\t') {{ esc_buf[pos++]='\\\\'; esc_buf[pos++]='t'; }}\n\
                     \x20           else esc_buf[pos++] = (char)c;\n\
                     \x20       }}\n\
                     \x20       esc_buf[pos++] = '\"'; esc_buf[pos] = '\\0';\n\
                     \x20       write_json_str(f, esc_buf);\n\
                     \x20       fputs(\",\\\"err\\\":null}}\\n\", f); fflush(f);\n",
                    json_str(s)
                ));
            }
            (ExerciseSignature::TwoStrsToBool, TestInput::TwoStrs(a, b)) => {
                out.push_str(&format!(
                    "        bool v = solve({}, {});\n\
                     \x20       write_record(f, {idx}, v ? \"true\" : \"false\");\n",
                    json_str(a),
                    json_str(b)
                ));
            }
            (ExerciseSignature::IntVecToInt, TestInput::IntVec(v)) => {
                out.push_str(&c_array_decl("arr", v));
                out.push_str(&format!(
                    "        long long r = solve(arr, {});\n\
                     \x20       char buf[64]; snprintf(buf, sizeof(buf), \"%lld\", r);\n\
                     \x20       write_record(f, {idx}, buf);\n",
                    v.len()
                ));
            }
            (ExerciseSignature::IntVecIntToInt, TestInput::IntVecAndInt(v, k)) => {
                out.push_str(&c_array_decl("arr", v));
                out.push_str(&format!(
                    "        long long r = solve(arr, {}, {k}LL);\n\
                     \x20       char buf[64]; snprintf(buf, sizeof(buf), \"%lld\", r);\n\
                     \x20       write_record(f, {idx}, buf);\n",
                    v.len()
                ));
            }
            (ExerciseSignature::IntVecToIntVec, TestInput::IntVec(v)) => {
                out.push_str(&c_array_decl("arr", v));
                let cap = v.len().max(1) + 64;
                out.push_str(&format!(
                    "        long long out_arr[{cap}]; memset(out_arr, 0, sizeof(out_arr));\n\
                     \x20       size_t out_len = solve(arr, {}, out_arr);\n\
                     \x20       if (out_len > {cap}) out_len = {cap};\n\
                     \x20       char buf[65536]; size_t pos = 0;\n\
                     \x20       buf[pos++] = '[';\n\
                     \x20       for (size_t j = 0; j < out_len && pos + 32 < sizeof(buf); ++j) {{\n\
                     \x20           if (j > 0) buf[pos++] = ',';\n\
                     \x20           pos += (size_t)snprintf(buf + pos, sizeof(buf) - pos, \"%lld\", out_arr[j]);\n\
                     \x20       }}\n\
                     \x20       buf[pos++] = ']'; buf[pos] = '\\0';\n\
                     \x20       write_record(f, {idx}, buf);\n",
                    v.len()
                ));
            }
            (ExerciseSignature::IntVecIntToPair, TestInput::IntVecAndInt(v, k)) => {
                out.push_str(&c_array_decl("arr", v));
                out.push_str(&format!(
                    "        long long oa = 0, ob = 0;\n\
                     \x20       solve(arr, {}, {k}LL, &oa, &ob);\n\
                     \x20       char buf[96]; snprintf(buf, sizeof(buf), \"[%lld,%lld]\", oa, ob);\n\
                     \x20       write_record(f, {idx}, buf);\n",
                    v.len()
                ));
            }
            _ => {}
        }
        out.push_str("    }\n");
    }

    out.push_str("    fclose(f);\n    return 0;\n}\n");
    out
}

fn c_array_decl(name: &str, v: &[i64]) -> String {
    if v.is_empty() {
        format!("        long long {name}[1] = {{0}};\n")
    } else {
        let items: Vec<String> = v.iter().map(|x| format!("{x}LL")).collect();
        format!("        long long {name}[{}] = {{{}}};\n", v.len(), items.join(", "))
    }
}

fn build_cpp_harness(ex: &Exercise) -> String {
    let proto = match ex.signature {
        ExerciseSignature::IntToInt => "long long solve(long long n);",
        ExerciseSignature::StrToBool => "bool solve(const std::string& s);",
        ExerciseSignature::StrToInt => "long long solve(const std::string& s);",
        ExerciseSignature::StrToStr => "std::string solve(const std::string& s);",
        ExerciseSignature::TwoStrsToBool => "bool solve(const std::string& a, const std::string& b);",
        ExerciseSignature::IntVecToInt => "long long solve(const std::vector<long long>& nums);",
        ExerciseSignature::IntVecIntToInt => {
            "long long solve(const std::vector<long long>& nums, long long k);"
        }
        ExerciseSignature::IntVecToIntVec => {
            "std::vector<long long> solve(const std::vector<long long>& nums);"
        }
        ExerciseSignature::IntVecIntToPair => {
            "std::pair<long long, long long> solve(const std::vector<long long>& nums, long long k);"
        }
    };

    let mut out = format!(
        "#include <cstdlib>\n\
         #include <exception>\n\
         #include <fstream>\n\
         #include <iomanip>\n\
         #include <sstream>\n\
         #include <string>\n\
         #include <utility>\n\
         #include <vector>\n\
         \n\
         {proto}\n\
         \n\
         static std::string json_escape(const std::string& s) {{\n\
             std::ostringstream o;\n\
             o << '\"';\n\
             for (unsigned char c : s) {{\n\
                 switch (c) {{\n\
                     case '\"': o << \"\\\\\\\"\"; break;\n\
                     case '\\\\': o << \"\\\\\\\\\"; break;\n\
                     case '\\n': o << \"\\\\n\"; break;\n\
                     case '\\r': o << \"\\\\r\"; break;\n\
                     case '\\t': o << \"\\\\t\"; break;\n\
                     default:\n\
                         if (c < 0x20) o << \"\\\\u\" << std::hex << std::setw(4) << std::setfill('0') << (int)c << std::dec;\n\
                         else o << (char)c;\n\
                 }}\n\
             }}\n\
             o << '\"';\n\
             return o.str();\n\
         }}\n\
         \n\
         static void write_ok(std::ofstream& f, size_t idx, const std::string& val) {{\n\
             f << \"{{\\\"i\\\":\" << idx << \",\\\"val\\\":\" << json_escape(val) << \",\\\"err\\\":null}}\\n\";\n\
             f.flush();\n\
         }}\n\
         \n\
         static void write_err(std::ofstream& f, size_t idx, const std::string& err) {{\n\
             f << \"{{\\\"i\\\":\" << idx << \",\\\"val\\\":null,\\\"err\\\":\" << json_escape(err) << \"}}\\n\";\n\
             f.flush();\n\
         }}\n\
         \n\
         int main() {{\n\
             const char* path = std::getenv(\"GTUIM_RESULTS_FILE\");\n\
             if (!path) return 2;\n\
             std::ofstream f(path);\n\
             if (!f) return 3;\n"
    );

    for (idx, tc) in ex.tests.iter().enumerate() {
        let call = match &tc.input {
            TestInput::Int(n) => format!("solve({n}LL)"),
            TestInput::Str(s) => format!("solve(std::string({}))", json_str(s)),
            TestInput::TwoStrs(a, b) => {
                format!("solve(std::string({}), std::string({}))", json_str(a), json_str(b))
            }
            TestInput::IntVec(v) => {
                let items: Vec<String> = v.iter().map(|x| format!("{x}LL")).collect();
                format!("solve(std::vector<long long>{{{}}})", items.join(", "))
            }
            TestInput::IntVecAndInt(v, k) => {
                let items: Vec<String> = v.iter().map(|x| format!("{x}LL")).collect();
                format!("solve(std::vector<long long>{{{}}}, {k}LL)", items.join(", "))
            }
        };
        let fmt_body = match ex.signature {
            ExerciseSignature::IntToInt
            | ExerciseSignature::StrToInt
            | ExerciseSignature::IntVecToInt
            | ExerciseSignature::IntVecIntToInt => {
                format!("auto v = {call}; write_ok(f, {idx}, std::to_string(v));")
            }
            ExerciseSignature::StrToBool | ExerciseSignature::TwoStrsToBool => {
                format!("bool v = {call}; write_ok(f, {idx}, v ? \"true\" : \"false\");")
            }
            ExerciseSignature::StrToStr => {
                format!("std::string v = {call}; write_ok(f, {idx}, json_escape(v));")
            }
            ExerciseSignature::IntVecToIntVec => format!(
                "auto v = {call}; std::ostringstream oss; oss << '[';\n\
                 \x20       for (size_t j = 0; j < v.size(); ++j) {{ if (j) oss << ','; oss << v[j]; }}\n\
                 \x20       oss << ']'; write_ok(f, {idx}, oss.str());"
            ),
            ExerciseSignature::IntVecIntToPair => format!(
                "auto v = {call}; write_ok(f, {idx}, \"[\" + std::to_string(v.first) + \",\" + std::to_string(v.second) + \"]\");"
            ),
        };
        out.push_str(&format!(
            "    try {{\n        {fmt_body}\n    }} catch (const std::exception& e) {{\n        write_err(f, {idx}, e.what());\n    }} catch (...) {{\n        write_err(f, {idx}, \"unknown C++ exception\");\n    }}\n"
        ));
    }
    out.push_str("    return 0;\n}\n");
    out
}

fn build_zig_harness(ex: &Exercise) -> String {
    let mut out = String::from(
        r#"const std = @import("std");
const solution = @import("solution.zig");

fn appendJsonEscaped(buf: *std.ArrayList(u8), alloc: std.mem.Allocator, s: []const u8) !void {
    try buf.append(alloc, '"');
    for (s) |c| {
        switch (c) {
            '"' => try buf.appendSlice(alloc, "\\\""),
            '\\' => try buf.appendSlice(alloc, "\\\\"),
            '\n' => try buf.appendSlice(alloc, "\\n"),
            '\r' => try buf.appendSlice(alloc, "\\r"),
            '\t' => try buf.appendSlice(alloc, "\\t"),
            else => {
                if (c < 0x20) {
                    var tmp: [6]u8 = undefined;
                    const esc = std.fmt.bufPrint(&tmp, "\\u{x:0>4}", .{c}) catch unreachable;
                    try buf.appendSlice(alloc, esc);
                } else {
                    try buf.append(alloc, c);
                }
            },
        }
    }
    try buf.append(alloc, '"');
}

fn writeOk(io: std.Io, f: *std.Io.File, alloc: std.mem.Allocator, idx: usize, val: []const u8) !void {
    var line: std.ArrayList(u8) = .empty;
    defer line.deinit(alloc);
    var ibuf: [32]u8 = undefined;
    const is = std.fmt.bufPrint(&ibuf, "{d}", .{idx}) catch unreachable;
    try line.appendSlice(alloc, "{\"i\":");
    try line.appendSlice(alloc, is);
    try line.appendSlice(alloc, ",\"val\":");
    try appendJsonEscaped(&line, alloc, val);
    try line.appendSlice(alloc, ",\"err\":null}\n");
    try f.writeStreamingAll(io, line.items);
}

fn writeErr(io: std.Io, f: *std.Io.File, alloc: std.mem.Allocator, idx: usize, msg: []const u8) !void {
    var line: std.ArrayList(u8) = .empty;
    defer line.deinit(alloc);
    var ibuf: [32]u8 = undefined;
    const is = std.fmt.bufPrint(&ibuf, "{d}", .{idx}) catch unreachable;
    try line.appendSlice(alloc, "{\"i\":");
    try line.appendSlice(alloc, is);
    try line.appendSlice(alloc, ",\"val\":null,\"err\":");
    try appendJsonEscaped(&line, alloc, msg);
    try line.appendSlice(alloc, "}\n");
    try f.writeStreamingAll(io, line.items);
}

fn formatInt(alloc: std.mem.Allocator, v: i64) ![]u8 {
    return std.fmt.allocPrint(alloc, "{d}", .{v});
}

fn formatVec(alloc: std.mem.Allocator, v: []const i64) ![]u8 {
    var buf: std.ArrayList(u8) = .empty;
    errdefer buf.deinit(alloc);
    try buf.append(alloc, '[');
    for (v, 0..) |x, j| {
        if (j > 0) try buf.append(alloc, ',');
        var tmp: [32]u8 = undefined;
        const s = std.fmt.bufPrint(&tmp, "{d}", .{x}) catch unreachable;
        try buf.appendSlice(alloc, s);
    }
    try buf.append(alloc, ']');
    return buf.toOwnedSlice(alloc);
}

pub fn main(init: std.process.Init) !void {
    const io = init.io;
    const base_alloc = std.heap.page_allocator;
    const path = init.environ_map.get("GTUIM_RESULTS_FILE") orelse return error.MissingResultsEnv;
    var f = try std.Io.Dir.createFileAbsolute(io, path, .{});
    defer f.close(io);
    _ = &writeErr;
    _ = &formatInt;
    _ = &formatVec;
"#,
    );

    for (idx, tc) in ex.tests.iter().enumerate() {
        out.push_str("    {\n        var arena = std.heap.ArenaAllocator.init(base_alloc);\n        defer arena.deinit();\n        const a = arena.allocator();\n");
        match (&ex.signature, &tc.input) {
            (ExerciseSignature::IntToInt, TestInput::Int(n)) => {
                out.push_str(&format!(
                    "        const v: i64 = solution.solve(@as(i64, {n}));\n        const s = try formatInt(a, v);\n        try writeOk(io, &f, a, {idx}, s);\n"
                ));
            }
            (ExerciseSignature::StrToBool, TestInput::Str(s)) => {
                out.push_str(&format!(
                    "        const v: bool = solution.solve({});\n        try writeOk(io, &f, a, {idx}, if (v) \"true\" else \"false\");\n",
                    json_str(s)
                ));
            }
            (ExerciseSignature::StrToInt, TestInput::Str(s)) => {
                out.push_str(&format!(
                    "        const v: i64 = solution.solve({});\n        const s = try formatInt(a, v);\n        try writeOk(io, &f, a, {idx}, s);\n",
                    json_str(s)
                ));
            }
            (ExerciseSignature::StrToStr, TestInput::Str(s)) => {
                out.push_str(&format!(
                    "        if (solution.solve(a, {})) |res| {{\n            var esc: std.ArrayList(u8) = .empty;\n            try appendJsonEscaped(&esc, a, res);\n            try writeOk(io, &f, a, {idx}, esc.items);\n        }} else |err| {{\n            try writeErr(io, &f, a, {idx}, @errorName(err));\n        }}\n",
                    json_str(s)
                ));
            }
            (ExerciseSignature::TwoStrsToBool, TestInput::TwoStrs(a, b)) => {
                out.push_str(&format!(
                    "        const v: bool = solution.solve({}, {});\n        try writeOk(io, &f, a, {idx}, if (v) \"true\" else \"false\");\n",
                    json_str(a),
                    json_str(b)
                ));
            }
            (ExerciseSignature::IntVecToInt, TestInput::IntVec(v)) => {
                out.push_str(&zig_slice_decl("arr", v));
                out.push_str(&format!(
                    "        const r: i64 = solution.solve(&arr);\n        const s = try formatInt(a, r);\n        try writeOk(io, &f, a, {idx}, s);\n"
                ));
            }
            (ExerciseSignature::IntVecIntToInt, TestInput::IntVecAndInt(v, k)) => {
                out.push_str(&zig_slice_decl("arr", v));
                out.push_str(&format!(
                    "        const r: i64 = solution.solve(&arr, @as(i64, {k}));\n        const s = try formatInt(a, r);\n        try writeOk(io, &f, a, {idx}, s);\n"
                ));
            }
            (ExerciseSignature::IntVecToIntVec, TestInput::IntVec(v)) => {
                out.push_str(&zig_slice_decl("arr", v));
                out.push_str(&format!(
                    "        if (solution.solve(a, &arr)) |res| {{\n            const s = try formatVec(a, res);\n            try writeOk(io, &f, a, {idx}, s);\n        }} else |err| {{\n            try writeErr(io, &f, a, {idx}, @errorName(err));\n        }}\n"
                ));
            }
            (ExerciseSignature::IntVecIntToPair, TestInput::IntVecAndInt(v, k)) => {
                out.push_str(&zig_slice_decl("arr", v));
                out.push_str(&format!(
                    "        const p: [2]i64 = solution.solve(&arr, @as(i64, {k}));\n        const s = try std.fmt.allocPrint(a, \"[{{d}},{{d}}]\", .{{ p[0], p[1] }});\n        try writeOk(io, &f, a, {idx}, s);\n"
                ));
            }
            _ => {}
        }
        out.push_str("    }\n");
    }

    out.push_str("}\n");
    out
}

fn zig_slice_decl(name: &str, v: &[i64]) -> String {
    let items: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    format!("        const {name} = [_]i64{{{}}};\n", items.join(", "))
}

fn build_go_harness(ex: &Exercise) -> String {
    let mut out = String::from(
        "package main\n\
         import (\n\
             \"encoding/json\"\n\
             \"fmt\"\n\
             \"os\"\n\
             \"strconv\"\n\
             \"strings\"\n\
         )\n\
         type gtuimRow struct {\n\
             I   int     `json:\"i\"`\n\
             Val *string `json:\"val\"`\n\
             Err *string `json:\"err\"`\n\
         }\n\
         func gtuimWrite(f *os.File, idx int, fn func() string) {\n\
             var row gtuimRow\n\
             row.I = idx\n\
             func() {\n\
                 defer func() {\n\
                     if r := recover(); r != nil {\n\
                         msg := fmt.Sprintf(\"panic: %v\", r)\n\
                         row.Err = &msg\n\
                     }\n\
                 }()\n\
                 v := fn()\n\
                 row.Val = &v\n\
             }()\n\
             b, _ := json.Marshal(row)\n\
             _, _ = f.Write(b)\n\
             _, _ = f.WriteString(\"\\n\")\n\
             _ = f.Sync()\n\
         }\n\
         func gtuimFmtVec(v []int64) string {\n\
             parts := make([]string, len(v))\n\
             for i, x := range v { parts[i] = strconv.FormatInt(x, 10) }\n\
             return \"[\" + strings.Join(parts, \",\") + \"]\"\n\
         }\n\
         func main() {\n\
             f, err := os.Create(os.Getenv(\"GTUIM_RESULTS_FILE\"))\n\
             if err != nil { os.Exit(2) }\n\
             defer f.Close()\n\
             _ = gtuimFmtVec\n",
    );
    for (idx, tc) in ex.tests.iter().enumerate() {
        let call = match &tc.input {
            TestInput::Int(n) => format!("solve(int64({n}))"),
            TestInput::Str(s) => format!("solve({})", json_str(s)),
            TestInput::TwoStrs(a, b) => format!("solve({}, {})", json_str(a), json_str(b)),
            TestInput::IntVec(v) => {
                let items: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                format!("solve([]int64{{{}}})", items.join(", "))
            }
            TestInput::IntVecAndInt(v, k) => {
                let items: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                format!("solve([]int64{{{}}}, int64({k}))", items.join(", "))
            }
        };
        let body = match ex.signature {
            ExerciseSignature::IntToInt
            | ExerciseSignature::StrToInt
            | ExerciseSignature::IntVecToInt
            | ExerciseSignature::IntVecIntToInt => format!("return strconv.FormatInt({call}, 10)"),
            ExerciseSignature::StrToBool | ExerciseSignature::TwoStrsToBool => {
                format!("if {call} {{ return \"true\" }}; return \"false\"")
            }
            ExerciseSignature::StrToStr => {
                format!("b, _ := json.Marshal({call}); return string(b)")
            }
            ExerciseSignature::IntVecToIntVec => format!("return gtuimFmtVec({call})"),
            ExerciseSignature::IntVecIntToPair => {
                format!("p := {call}; return fmt.Sprintf(\"[%d,%d]\", p[0], p[1])")
            }
        };
        out.push_str(&format!(
            "    gtuimWrite(f, {idx}, func() string {{ {body} }})\n"
        ));
    }
    out.push_str("}\n");
    out
}

fn build_java_harness(ex: &Exercise) -> String {
    let mut out = String::from(
        "import java.io.*;\n\
         public class Main {\n\
             static String jsonEsc(String s) {\n\
                 if (s == null) return \"null\";\n\
                 StringBuilder sb = new StringBuilder();\n\
                 sb.append('\"');\n\
                 for (int i = 0; i < s.length(); i++) {\n\
                     char c = s.charAt(i);\n\
                     if (c == '\"') sb.append(\"\\\\\\\"\");\n\
                     else if (c == '\\\\') sb.append(\"\\\\\\\\\");\n\
                     else if (c == '\\n') sb.append(\"\\\\n\");\n\
                     else if (c == '\\r') sb.append(\"\\\\r\");\n\
                     else if (c == '\\t') sb.append(\"\\\\t\");\n\
                     else if (c < 0x20) sb.append(String.format(\"\\\\u%04x\", (int) c));\n\
                     else sb.append(c);\n\
                 }\n\
                 sb.append('\"');\n\
                 return sb.toString();\n\
             }\n\
             static String fmtArr(long[] arr) {\n\
                 if (arr == null) throw new IllegalStateException(\"returned null array\");\n\
                 StringBuilder sb = new StringBuilder(\"[\");\n\
                 for (int i = 0; i < arr.length; i++) {\n\
                     if (i > 0) sb.append(',');\n\
                     sb.append(arr[i]);\n\
                 }\n\
                 sb.append(']');\n\
                 return sb.toString();\n\
             }\n\
             static void ok(PrintWriter pw, int idx, String val) {\n\
                 pw.println(\"{\\\"i\\\":\" + idx + \",\\\"val\\\":\" + jsonEsc(val) + \",\\\"err\\\":null}\");\n\
                 pw.flush();\n\
             }\n\
             static void err(PrintWriter pw, int idx, Throwable t) {\n\
                 pw.println(\"{\\\"i\\\":\" + idx + \",\\\"val\\\":null,\\\"err\\\":\" + jsonEsc(t.toString()) + \"}\");\n\
                 pw.flush();\n\
             }\n\
             public static void main(String[] args) throws Exception {\n\
                 try (PrintWriter pw = new PrintWriter(new FileWriter(System.getenv(\"GTUIM_RESULTS_FILE\")))) {\n",
    );
    for (idx, tc) in ex.tests.iter().enumerate() {
        let call = match &tc.input {
            TestInput::Int(n) => format!("Solution.solve({n}L)"),
            TestInput::Str(s) => format!("Solution.solve({})", json_str(s)),
            TestInput::TwoStrs(a, b) => format!("Solution.solve({}, {})", json_str(a), json_str(b)),
            TestInput::IntVec(v) => {
                let items: Vec<String> = v.iter().map(|x| format!("{x}L")).collect();
                format!("Solution.solve(new long[]{{{}}})", items.join(", "))
            }
            TestInput::IntVecAndInt(v, k) => {
                let items: Vec<String> = v.iter().map(|x| format!("{x}L")).collect();
                format!("Solution.solve(new long[]{{{}}}, {k}L)", items.join(", "))
            }
        };
        let body = match ex.signature {
            ExerciseSignature::IntToInt
            | ExerciseSignature::StrToInt
            | ExerciseSignature::IntVecToInt
            | ExerciseSignature::IntVecIntToInt => format!("ok(pw, {idx}, Long.toString({call}));"),
            ExerciseSignature::StrToBool | ExerciseSignature::TwoStrsToBool => {
                format!("ok(pw, {idx}, {call} ? \"true\" : \"false\");")
            }
            ExerciseSignature::StrToStr => {
                format!("String s = {call}; if (s == null) throw new NullPointerException(\"returned null\"); ok(pw, {idx}, jsonEsc(s));")
            }
            ExerciseSignature::IntVecToIntVec => format!("ok(pw, {idx}, fmtArr({call}));"),
            ExerciseSignature::IntVecIntToPair => format!(
                "long[] p = {call}; if (p == null || p.length != 2) throw new IllegalStateException(\"expected length-2 array\"); ok(pw, {idx}, \"[\" + p[0] + \",\" + p[1] + \"]\");"
            ),
        };
        out.push_str(&format!(
            "            try {{ {body} }} catch (Throwable t) {{ err(pw, {idx}, t); }}\n"
        ));
    }
    out.push_str("        }\n    }\n}\n");
    out
}

fn build_ruby_harness(ex: &Exercise) -> String {
    let mut out = String::from(
        "require 'json'\n\
         require_relative 'solution'\n\
         def fmt_val(sig, v)\n\
           case sig\n\
           when 'bool'\n\
             raise TypeError, \"expected true/false, got #{v.class}\" unless v == true || v == false\n\
             v ? 'true' : 'false'\n\
           when 'int'\n\
             raise TypeError, \"expected Integer, got #{v.class}\" unless v.is_a?(Integer)\n\
             v.to_s\n\
           when 'str'\n\
             raise TypeError, \"expected String, got #{v.class}\" unless v.is_a?(String)\n\
             JSON.generate(v)\n\
           when 'vec'\n\
             raise TypeError, \"expected Array, got #{v.class}\" unless v.is_a?(Array)\n\
             '[' + v.map { |x| raise TypeError, 'expected Integer element' unless x.is_a?(Integer); x.to_s }.join(',') + ']'\n\
           when 'pair'\n\
             raise TypeError, 'expected 2-element Array' unless v.is_a?(Array) && v.length == 2\n\
             raise TypeError, 'expected Integer elements' unless v[0].is_a?(Integer) && v[1].is_a?(Integer)\n\
             \"[#{v[0]},#{v[1]}]\"\n\
           end\n\
         end\n\
         File.open(ENV.fetch('GTUIM_RESULTS_FILE'), 'w') do |f|\n",
    );
    let ret_kind = signature_ret_kind(ex.signature);
    for (idx, tc) in ex.tests.iter().enumerate() {
        let call = match &tc.input {
            TestInput::Int(n) => format!("solve({n})"),
            TestInput::Str(s) => format!("solve({})", json_str(s)),
            TestInput::TwoStrs(a, b) => format!("solve({}, {})", json_str(a), json_str(b)),
            TestInput::IntVec(v) => format!("solve({v:?})"),
            TestInput::IntVecAndInt(v, k) => format!("solve({v:?}, {k})"),
        };
        out.push_str(&format!(
            "  begin\n\
             \x20   v = {call}\n\
             \x20   f.puts(JSON.generate({{ i: {idx}, val: fmt_val('{ret_kind}', v), err: nil }}))\n\
             \x20 rescue Exception => e\n\
             \x20   f.puts(JSON.generate({{ i: {idx}, val: nil, err: \"#{{e.class}}: #{{e.message}}\" }}))\n\
             \x20 end\n\
             \x20 f.flush\n"
        ));
    }
    out.push_str("end\n");
    out
}

fn build_lua_harness(ex: &Exercise) -> String {
    let mut out = String::from(
        "dofile('solution.lua')\n\
         local function json_esc(s)\n\
           local map = { ['\\\\'] = '\\\\\\\\', ['\"'] = '\\\\\"', ['\\n'] = '\\\\n', ['\\r'] = '\\\\r', ['\\t'] = '\\\\t' }\n\
           return '\"' .. tostring(s):gsub('[\\\\\"\\n\\r\\t]', map) .. '\"'\n\
         end\n\
         local function fmt_int(x)\n\
           if type(x) ~= 'number' then error('expected number, got ' .. type(x)) end\n\
           if math.floor(x) ~= x then error('expected integer') end\n\
           return string.format('%.0f', x)\n\
         end\n\
         local function fmt_val(sig, v)\n\
           if sig == 'bool' then\n\
             if type(v) ~= 'boolean' then error('expected boolean, got ' .. type(v)) end\n\
             return v and 'true' or 'false'\n\
           elseif sig == 'int' then\n\
             return fmt_int(v)\n\
           elseif sig == 'str' then\n\
             if type(v) ~= 'string' then error('expected string, got ' .. type(v)) end\n\
             return json_esc(v)\n\
           elseif sig == 'vec' then\n\
             if type(v) ~= 'table' then error('expected table, got ' .. type(v)) end\n\
             local parts = {}\n\
             for i = 1, #v do parts[i] = fmt_int(v[i]) end\n\
             return '[' .. table.concat(parts, ',') .. ']'\n\
           elseif sig == 'pair' then\n\
             if type(v) ~= 'table' or #v ~= 2 then error('expected 2-element table') end\n\
             return '[' .. fmt_int(v[1]) .. ',' .. fmt_int(v[2]) .. ']'\n\
           end\n\
         end\n\
         local f = assert(io.open(os.getenv('GTUIM_RESULTS_FILE'), 'w'))\n\
         local function run_case(idx, sig, fn)\n\
           local ok, res = pcall(fn)\n\
           if ok then\n\
             local ok2, formatted = pcall(fmt_val, sig, res)\n\
             if ok2 then\n\
               f:write('{\"i\":' .. idx .. ',\"val\":' .. json_esc(formatted) .. ',\"err\":null}\\n')\n\
             else\n\
               f:write('{\"i\":' .. idx .. ',\"val\":null,\"err\":' .. json_esc(formatted) .. '}\\n')\n\
             end\n\
           else\n\
             f:write('{\"i\":' .. idx .. ',\"val\":null,\"err\":' .. json_esc(res) .. '}\\n')\n\
           end\n\
           f:flush()\n\
         end\n",
    );
    let ret_kind = signature_ret_kind(ex.signature);
    for (idx, tc) in ex.tests.iter().enumerate() {
        let call = match &tc.input {
            TestInput::Int(n) => format!("solve({n})"),
            TestInput::Str(s) => format!("solve({})", json_str(s)),
            TestInput::TwoStrs(a, b) => format!("solve({}, {})", json_str(a), json_str(b)),
            TestInput::IntVec(v) => {
                let items: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                format!("solve({{{}}})", items.join(", "))
            }
            TestInput::IntVecAndInt(v, k) => {
                let items: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                format!("solve({{{}}}, {k})", items.join(", "))
            }
        };
        out.push_str(&format!(
            "run_case({idx}, '{ret_kind}', function() return {call} end)\n"
        ));
    }
    out.push_str("f:close()\n");
    out
}

const fn signature_ret_kind(sig: ExerciseSignature) -> &'static str {
    match sig {
        ExerciseSignature::IntToInt
        | ExerciseSignature::StrToInt
        | ExerciseSignature::IntVecToInt
        | ExerciseSignature::IntVecIntToInt => "int",
        ExerciseSignature::StrToBool | ExerciseSignature::TwoStrsToBool => "bool",
        ExerciseSignature::StrToStr => "str",
        ExerciseSignature::IntVecToIntVec => "vec",
        ExerciseSignature::IntVecIntToPair => "pair",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::practice::catalog::all_exercises;

    fn python_reference_solution(id: &str) -> &'static str {
        match id {
            "valid-parentheses" => r#"
def solve(s: str) -> bool:
    st = []
    pairs = {')': '(', ']': '[', '}': '{'}
    for c in s:
        if c in '([{':
            st.append(c)
        else:
            if not st or st[-1] != pairs[c]:
                return False
            st.pop()
    return not st
"#,
            "two-sum" => r#"
def solve(nums: list[int], k: int) -> list[int]:
    seen = {}
    for i, x in enumerate(nums):
        need = k - x
        if need in seen:
            return [seen[need], i]
        seen[x] = i
    return [0, 0]
"#,
            "valid-anagram" => r#"
def solve(a: str, b: str) -> bool:
    return sorted(a) == sorted(b)
"#,
            "move-zeroes" => r#"
def solve(nums: list[int]) -> list[int]:
    nz = [x for x in nums if x != 0]
    return nz + [0] * (len(nums) - len(nz))
"#,
            "best-stock-profit" => r#"
def solve(nums: list[int]) -> int:
    best = 0
    low = None
    for p in nums:
        if low is None or p < low:
            low = p
        elif p - low > best:
            best = p - low
    return best
"#,
            "digital-root" => r#"
def solve(n: int) -> int:
    if n == 0:
        return 0
    return 1 + (n - 1) % 9
"#,
            "longest-unique-substr" => r#"
def solve(s: str) -> int:
    last = {}
    left = 0
    best = 0
    for right, ch in enumerate(s):
        if ch in last and last[ch] >= left:
            left = last[ch] + 1
        last[ch] = right
        best = max(best, right - left + 1)
    return best
"#,
            "max-subarray-sum" => r#"
def solve(nums: list[int]) -> int:
    best = cur = nums[0]
    for x in nums[1:]:
        cur = max(x, cur + x)
        best = max(best, cur)
    return best
"#,
            "product-except-self" => r#"
def solve(nums: list[int]) -> list[int]:
    n = len(nums)
    out = [1] * n
    p = 1
    for i in range(n):
        out[i] = p
        p *= nums[i]
    s = 1
    for i in range(n - 1, -1, -1):
        out[i] *= s
        s *= nums[i]
    return out
"#,
            "run-length-encode" => r#"
def solve(s: str) -> str:
    if not s:
        return ""
    out = []
    cur = s[0]
    cnt = 1
    for ch in s[1:]:
        if ch == cur:
            cnt += 1
        else:
            out.append(f"{cur}{cnt}")
            cur = ch
            cnt = 1
    out.append(f"{cur}{cnt}")
    return "".join(out)
"#,
            "subarray-sum-k" => r#"
def solve(nums: list[int], k: int) -> int:
    counts = {0: 1}
    total = 0
    ans = 0
    for x in nums:
        total += x
        ans += counts.get(total - k, 0)
        counts[total] = counts.get(total, 0) + 1
    return ans
"#,
            "camel-to-snake" => r#"
def solve(s: str) -> str:
    out = []
    n = len(s)
    for i, ch in enumerate(s):
        if ch.isupper():
            prev_lower_or_digit = i > 0 and (s[i - 1].islower() or s[i - 1].isdigit())
            next_lower = i + 1 < n and s[i + 1].islower()
            prev_upper = i > 0 and s[i - 1].isupper()
            if i > 0 and (prev_lower_or_digit or (prev_upper and next_lower)):
                out.append('_')
            out.append(ch.lower())
        else:
            out.append(ch)
    return "".join(out)
"#,
            "coin-change-min" => r#"
def solve(nums: list[int], k: int) -> int:
    inf = k + 1
    dp = [inf] * (k + 1)
    dp[0] = 0
    for amt in range(1, k + 1):
        for c in nums:
            if c <= amt and dp[amt - c] + 1 < dp[amt]:
                dp[amt] = dp[amt - c] + 1
    return -1 if dp[k] > k else dp[k]
"#,
            "trapping-rain-water" => r#"
def solve(nums: list[int]) -> int:
    l, r = 0, len(nums) - 1
    lm, rm = 0, 0
    ans = 0
    while l < r:
        if nums[l] <= nums[r]:
            lm = max(lm, nums[l])
            ans += lm - nums[l]
            l += 1
        else:
            rm = max(rm, nums[r])
            ans += rm - nums[r]
            r -= 1
    return ans
"#,
            "next-greater-circular" => r#"
def solve(nums: list[int]) -> list[int]:
    n = len(nums)
    res = [-1] * n
    st = []
    for i in range(2 * n):
        cur = nums[i % n]
        while st and nums[st[-1]] < cur:
            res[st.pop()] = cur
        if i < n:
            st.append(i)
    return res
"#,
            "longest-valid-parens" => r#"
def solve(s: str) -> int:
    st = [-1]
    best = 0
    for i, ch in enumerate(s):
        if ch == '(':
            st.append(i)
        else:
            st.pop()
            if not st:
                st.append(i)
            else:
                best = max(best, i - st[-1])
    return best
"#,
            "is-wildcard-match" => r#"
def solve(a: str, b: str) -> bool:
    i = j = 0
    star = -1
    match = 0
    while i < len(a):
        if j < len(b) and (b[j] == '?' or b[j] == a[i]):
            i += 1
            j += 1
        elif j < len(b) and b[j] == '*':
            star = j
            match = i
            j += 1
        elif star != -1:
            j = star + 1
            match += 1
            i = match
        else:
            return False
    while j < len(b) and b[j] == '*':
        j += 1
    return j == len(b)
"#,
            "count-ways-stairs-k" => r#"
def solve(n: int) -> int:
    a, b, c = 1, 1, 2
    if n == 0:
        return a
    if n == 1:
        return b
    if n == 2:
        return c
    for _ in range(3, n + 1):
        a, b, c = b, c, a + b + c
    return c
"#,
            other => panic!("missing Python reference solution for {other}"),
        }
    }

    #[test]
    fn all_18_exercises_pass_with_python_reference_solutions() {
        let reg = ToolchainRegistry::detect_full();
        let py = reg.get(PracticeLanguage::Python);
        assert!(py.available, "python3 expected on test machine");
        let cancel = Arc::new(AtomicBool::new(false));
        for ex in all_exercises() {
            let sol = python_reference_solution(ex.id);
            let rep = evaluate_submission(
                &ex,
                PracticeLanguage::Python,
                sol,
                &py,
                RunLimits::default(),
                &cancel,
            );
            assert_eq!(
                rep.verdict,
                SubmissionVerdict::Accepted,
                "exercise {} failed: {} | stderr: {} | cases: {:?}",
                ex.id,
                rep.summary,
                rep.stderr,
                rep.cases
            );
            assert_eq!(rep.passed_count, ex.tests.len());
        }
    }

    #[test]
    fn multi_language_real_execution_passes_for_installed_toolchains() {
        let reg = ToolchainRegistry::detect_full();
        let ex = all_exercises()
            .into_iter()
            .find(|e| e.id == "digital-root")
            .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        let solutions: &[(PracticeLanguage, &str)] = &[
            (
                PracticeLanguage::Python,
                "def solve(n: int) -> int:\n    return 0 if n == 0 else 1 + (n - 1) % 9\n",
            ),
            (
                PracticeLanguage::JavaScript,
                "function solve(n) { return n === 0 ? 0 : 1 + ((n - 1) % 9); }\n",
            ),
            (
                PracticeLanguage::TypeScript,
                "function solve(n: number): number { return n === 0 ? 0 : 1 + ((n - 1) % 9); }\n",
            ),
            (
                PracticeLanguage::Rust,
                "pub fn solve(n: i64) -> i64 { if n == 0 { 0 } else { 1 + (n - 1) % 9 } }\n",
            ),
            (
                PracticeLanguage::C,
                "long long solve(long long n) { return n == 0 ? 0 : 1 + (n - 1) % 9; }\n",
            ),
            (
                PracticeLanguage::Cpp,
                "long long solve(long long n) { return n == 0 ? 0 : 1 + (n - 1) % 9; }\n",
            ),
            (
                PracticeLanguage::Ruby,
                "def solve(n)\n  n == 0 ? 0 : 1 + ((n - 1) % 9)\nend\n",
            ),
            (
                PracticeLanguage::Lua,
                "function solve(n)\n  if n == 0 then return 0 end\n  return 1 + ((n - 1) % 9)\nend\n",
            ),
        ];

        let mut tested = 0usize;
        for &(lang, code) in solutions {
            let tc = reg.get(lang);
            if !tc.available {
                continue;
            }
            let rep = evaluate_submission(&ex, lang, code, &tc, RunLimits::default(), &cancel);
            assert_eq!(
                rep.verdict,
                SubmissionVerdict::Accepted,
                "{:?} failed: {} | stderr: {} | cases: {:?}",
                lang,
                rep.summary,
                rep.stderr,
                rep.cases
            );
            tested += 1;
        }
        assert!(tested >= 6, "expected at least 6 installed languages tested, got {tested}");
    }

    #[test]
    fn wrong_answer_compile_error_runtime_error_and_user_stdout_separation() {
        let reg = ToolchainRegistry::detect_full();
        let ex = all_exercises()
            .into_iter()
            .find(|e| e.id == "digital-root")
            .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        // 1. User print statements to stdout must NOT break grading!
        let py = reg.get(PracticeLanguage::Python);
        let noisy_ok = "def solve(n):\n    print('debug:', n)\n    return 0 if n == 0 else 1 + (n - 1) % 9\n";
        let rep_ok = evaluate_submission(&ex, PracticeLanguage::Python, noisy_ok, &py, RunLimits::default(), &cancel);
        assert_eq!(rep_ok.verdict, SubmissionVerdict::Accepted);
        assert!(rep_ok.stdout.contains("debug: 38"));

        // 2. Wrong answer returns exact failing case details.
        let wrong = "def solve(n):\n    return 0\n";
        let rep_wa = evaluate_submission(&ex, PracticeLanguage::Python, wrong, &py, RunLimits::default(), &cancel);
        assert_eq!(rep_wa.verdict, SubmissionVerdict::WrongAnswer);
        assert_eq!(rep_wa.passed_count, 1); // only n=0 passes

        // 3. Runtime exception is captured cleanly per case.
        let boom = "def solve(n):\n    raise ValueError('boom')\n";
        let rep_rt = evaluate_submission(&ex, PracticeLanguage::Python, boom, &py, RunLimits::default(), &cancel);
        assert_eq!(rep_rt.verdict, SubmissionVerdict::RuntimeError);
        assert!(rep_rt.cases[0].error.as_deref().unwrap_or("").contains("boom"));

        // 4. Rust compile error surfaces compiler diagnostics.
        let rust_tc = reg.get(PracticeLanguage::Rust);
        if rust_tc.available {
            let bad_rs = "pub fn solve(n: i64) -> i64 { broken_identifier }";
            let rep_ce = evaluate_submission(&ex, PracticeLanguage::Rust, bad_rs, &rust_tc, RunLimits::default(), &cancel);
            assert_eq!(rep_ce.verdict, SubmissionVerdict::CompileError);
            assert!(rep_ce.stderr.contains("broken_identifier"));
        }
    }

    #[test]
    fn enforces_timeout_stdout_cap_env_sanitization_and_missing_toolchain() {
        let reg = ToolchainRegistry::detect_full();
        let py = reg.get(PracticeLanguage::Python);
        let ex = all_exercises()
            .into_iter()
            .find(|e| e.id == "digital-root")
            .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));

        // 1. Infinite loop is killed promptly by wall-clock timeout.
        let fast_limits = RunLimits {
            exec_timeout: Duration::from_millis(350),
            exec_cpu_secs: 2,
            ..RunLimits::default()
        };
        let inf_loop = "def solve(n):\n    while True:\n        pass\n";
        let t0 = Instant::now();
        let rep_tle = evaluate_submission(&ex, PracticeLanguage::Python, inf_loop, &py, fast_limits, &cancel);
        assert_eq!(rep_tle.verdict, SubmissionVerdict::TimeLimitExceeded);
        assert!(t0.elapsed() < Duration::from_secs(3), "timeout took too long: {:?}", t0.elapsed());

        // 2. Stdout flood is safely bounded without deadlock or OOM.
        let flood_limits = RunLimits {
            max_output_bytes: 2048,
            ..RunLimits::default()
        };
        let flood = "def solve(n):\n    print('X' * 50000)\n    return 0 if n == 0 else 1 + (n - 1) % 9\n";
        let rep_flood = evaluate_submission(&ex, PracticeLanguage::Python, flood, &py, flood_limits, &cancel);
        assert_eq!(rep_flood.verdict, SubmissionVerdict::Accepted);
        assert!(rep_flood.output_truncated);
        assert!(rep_flood.stdout.len() <= 2048);

        // 3. Sanitized environment strips arbitrary parent secrets.
        std::env::set_var("GTUIM_SECRET_TEST_TOKEN", "should_not_leak");
        let check_env = "import os\ndef solve(n):\n    if 'GTUIM_SECRET_TEST_TOKEN' in os.environ:\n        raise RuntimeError('secret leaked')\n    return 0 if n == 0 else 1 + (n - 1) % 9\n";
        let rep_env = evaluate_submission(&ex, PracticeLanguage::Python, check_env, &py, RunLimits::default(), &cancel);
        assert_eq!(rep_env.verdict, SubmissionVerdict::Accepted);

        // 4. Missing toolchain yields ToolchainUnavailable with actionable hint.
        let fake_missing = unavailable_status(PracticeLanguage::Go, "go", PracticeLanguage::Go.install_hint());
        let rep_missing = evaluate_submission(&ex, PracticeLanguage::Go, "package main", &fake_missing, RunLimits::default(), &cancel);
        assert_eq!(rep_missing.verdict, SubmissionVerdict::ToolchainUnavailable);
        assert!(rep_missing.summary.contains("go"));
    }

    #[test]
    fn multi_signature_harnesses_pass_across_installed_compiled_and_scripted_languages() {
        let reg = ToolchainRegistry::detect_full();
        let cancel = Arc::new(AtomicBool::new(false));
        let all = all_exercises();

        // Test StrToStr (`run-length-encode`) in C, C++, Rust, JS, TS, Ruby, Lua
        let rle = all.iter().find(|e| e.id == "run-length-encode").unwrap();
        let rle_cases: &[(PracticeLanguage, &str)] = &[
            (
                PracticeLanguage::C,
                r#"
#include <stddef.h>
#include <stdio.h>
#include <string.h>
void solve(const char *s, char *out, size_t out_cap) {
    size_t pos = 0;
    size_t n = strlen(s);
    for (size_t i = 0; i < n; ) {
        char c = s[i];
        size_t j = i;
        while (j < n && s[j] == c) j++;
        pos += (size_t)snprintf(out + pos, out_cap - pos, "%c%zu", c, j - i);
        i = j;
    }
    if (pos < out_cap) out[pos] = '\0';
}
"#,
            ),
            (
                PracticeLanguage::Cpp,
                r#"
#include <string>
std::string solve(const std::string& s) {
    std::string out;
    for (size_t i = 0; i < s.size(); ) {
        size_t j = i;
        while (j < s.size() && s[j] == s[i]) j++;
        out.push_back(s[i]);
        out += std::to_string(j - i);
        i = j;
    }
    return out;
}
"#,
            ),
            (
                PracticeLanguage::Rust,
                r#"
pub fn solve(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let mut j = i;
        while j < bytes.len() && bytes[j] == bytes[i] { j += 1; }
        out.push(bytes[i] as char);
        out.push_str(&(j - i).to_string());
        i = j;
    }
    out
}
"#,
            ),
            (
                PracticeLanguage::JavaScript,
                r#"
function solve(s) {
  let out = "";
  for (let i = 0; i < s.length; ) {
    let j = i;
    while (j < s.length && s[j] === s[i]) j++;
    out += s[i] + String(j - i);
    i = j;
  }
  return out;
}
"#,
            ),
            (
                PracticeLanguage::TypeScript,
                r#"
function solve(s: string): string {
  let out = "";
  for (let i = 0; i < s.length; ) {
    let j = i;
    while (j < s.length && s[j] === s[i]) j++;
    out += s[i] + String(j - i);
    i = j;
  }
  return out;
}
"#,
            ),
            (
                PracticeLanguage::Ruby,
                r#"
def solve(s)
  out = +""
  i = 0
  while i < s.length
    j = i
    j += 1 while j < s.length && s[j] == s[i]
    out << s[i] << (j - i).to_s
    i = j
  end
  out
end
"#,
            ),
            (
                PracticeLanguage::Lua,
                r#"
function solve(s)
  local parts = {}
  local i = 1
  local n = #s
  while i <= n do
    local ch = s:sub(i, i)
    local j = i
    while j <= n and s:sub(j, j) == ch do j = j + 1 end
    parts[#parts + 1] = ch .. tostring(j - i)
    i = j
  end
  return table.concat(parts)
end
"#,
            ),
        ];
        for &(lang, code) in rle_cases {
            let tc = reg.get(lang);
            if !tc.available {
                continue;
            }
            let rep = evaluate_submission(rle, lang, code, &tc, RunLimits::default(), &cancel);
            assert_eq!(
                rep.verdict,
                SubmissionVerdict::Accepted,
                "RLE {:?} failed: {} | stderr: {} | cases: {:?}",
                lang,
                rep.summary,
                rep.stderr,
                rep.cases
            );
        }

        // Test IntVecToIntVec (`move-zeroes`) in C, Rust, Lua
        let mz = all.iter().find(|e| e.id == "move-zeroes").unwrap();
        let mz_cases: &[(PracticeLanguage, &str)] = &[
            (
                PracticeLanguage::C,
                r#"
#include <stddef.h>
size_t solve(const long long *nums, size_t len, long long *out) {
    size_t pos = 0;
    for (size_t i = 0; i < len; ++i) if (nums[i] != 0) out[pos++] = nums[i];
    while (pos < len) out[pos++] = 0;
    return len;
}
"#,
            ),
            (
                PracticeLanguage::Rust,
                r#"
pub fn solve(nums: &[i64]) -> Vec<i64> {
    let mut out: Vec<i64> = nums.iter().copied().filter(|&x| x != 0).collect();
    out.resize(nums.len(), 0);
    out
}
"#,
            ),
            (
                PracticeLanguage::Lua,
                r#"
function solve(nums)
  local out = {}
  local zeros = 0
  for i = 1, #nums do
    if nums[i] == 0 then zeros = zeros + 1 else out[#out + 1] = nums[i] end
  end
  for _ = 1, zeros do out[#out + 1] = 0 end
  return out
end
"#,
            ),
        ];
        for &(lang, code) in mz_cases {
            let tc = reg.get(lang);
            if !tc.available {
                continue;
            }
            let rep = evaluate_submission(mz, lang, code, &tc, RunLimits::default(), &cancel);
            assert_eq!(
                rep.verdict,
                SubmissionVerdict::Accepted,
                "move-zeroes {:?} failed: {} | stderr: {} | cases: {:?}",
                lang,
                rep.summary,
                rep.stderr,
                rep.cases
            );
        }
    }

    #[test]
    fn c_and_zig_verify_every_exercise_signature_accepted_rejected_templates_and_unavailable() {
        use crate::gui::practice::catalog::starter_code;

        let reg = ToolchainRegistry::detect_full();
        let c_tc = reg.get(PracticeLanguage::C);
        let zig_tc = reg.get(PracticeLanguage::Zig);
        assert!(c_tc.available, "C compiler (cc/gcc/clang) expected on PATH");
        assert!(zig_tc.available, "Zig compiler expected on PATH");
        assert!(
            zig_tc.version.as_deref().unwrap_or("").contains("Zig"),
            "expected detected Zig version string, got {:?}",
            zig_tc.version
        );

        let all = all_exercises();
        let cancel = Arc::new(AtomicBool::new(false));
        let find_ex = |id: &str| all.iter().find(|e| e.id == id).unwrap().clone();

        // One representative exercise for EACH of the 9 ExerciseSignature variants.
        let c_accepted: &[(&str, &str)] = &[
            // 1. IntToInt
            (
                "digital-root",
                "long long solve(long long n) { return n == 0 ? 0 : 1 + (n - 1) % 9; }\n",
            ),
            // 2. StrToBool
            (
                "valid-parentheses",
                r#"
#include <stdbool.h>
#include <stddef.h>
#include <string.h>
bool solve(const char *s) {
    char st[10005];
    size_t top = 0;
    for (size_t i = 0; s[i]; ++i) {
        char c = s[i];
        if (c == '(' || c == '[' || c == '{') st[top++] = c;
        else {
            if (top == 0) return false;
            char o = st[--top];
            if ((c == ')' && o != '(') || (c == ']' && o != '[') || (c == '}' && o != '{')) return false;
        }
    }
    return top == 0;
}
"#,
            ),
            // 3. StrToInt
            (
                "longest-unique-substr",
                r#"
#include <stddef.h>
long long solve(const char *s) {
    long long last[256];
    for (int i = 0; i < 256; ++i) last[i] = -1;
    long long left = 0, best = 0;
    for (long long r = 0; s[r]; ++r) {
        unsigned char c = (unsigned char)s[r];
        if (last[c] >= left) left = last[c] + 1;
        last[c] = r;
        if (r - left + 1 > best) best = r - left + 1;
    }
    return best;
}
"#,
            ),
            // 4. StrToStr
            (
                "run-length-encode",
                r#"
#include <stddef.h>
#include <stdio.h>
#include <string.h>
void solve(const char *s, char *out, size_t out_cap) {
    size_t pos = 0, n = strlen(s);
    for (size_t i = 0; i < n; ) {
        char c = s[i];
        size_t j = i;
        while (j < n && s[j] == c) j++;
        pos += (size_t)snprintf(out + pos, out_cap - pos, "%c%zu", c, j - i);
        i = j;
    }
    if (pos < out_cap) out[pos] = '\0';
}
"#,
            ),
            // 5. TwoStrsToBool
            (
                "valid-anagram",
                r#"
#include <stdbool.h>
#include <stddef.h>
bool solve(const char *a, const char *b) {
    int cnt[256] = {0};
    for (size_t i = 0; a[i]; ++i) cnt[(unsigned char)a[i]]++;
    for (size_t i = 0; b[i]; ++i) cnt[(unsigned char)b[i]]--;
    for (int i = 0; i < 256; ++i) if (cnt[i] != 0) return false;
    return true;
}
"#,
            ),
            // 6. IntVecToInt
            (
                "best-stock-profit",
                r#"
#include <stddef.h>
long long solve(const long long *nums, size_t len) {
    if (len == 0) return 0;
    long long low = nums[0], best = 0;
    for (size_t i = 1; i < len; ++i) {
        if (nums[i] < low) low = nums[i];
        else if (nums[i] - low > best) best = nums[i] - low;
    }
    return best;
}
"#,
            ),
            // 7. IntVecIntToInt
            (
                "subarray-sum-k",
                r#"
#include <stddef.h>
long long solve(const long long *nums, size_t len, long long k) {
    long long ans = 0;
    for (size_t i = 0; i < len; ++i) {
        long long sum = 0;
        for (size_t j = i; j < len; ++j) {
            sum += nums[j];
            if (sum == k) ans++;
        }
    }
    return ans;
}
"#,
            ),
            // 8. IntVecToIntVec
            (
                "move-zeroes",
                r#"
#include <stddef.h>
size_t solve(const long long *nums, size_t len, long long *out) {
    size_t pos = 0;
    for (size_t i = 0; i < len; ++i) if (nums[i] != 0) out[pos++] = nums[i];
    while (pos < len) out[pos++] = 0;
    return len;
}
"#,
            ),
            // 9. IntVecIntToPair
            (
                "two-sum",
                r#"
#include <stddef.h>
void solve(const long long *nums, size_t len, long long k, long long *out_a, long long *out_b) {
    for (size_t i = 0; i < len; ++i) {
        for (size_t j = i + 1; j < len; ++j) {
            if (nums[i] + nums[j] == k) {
                *out_a = (long long)i;
                *out_b = (long long)j;
                return;
            }
        }
    }
}
"#,
            ),
        ];

        let zig_accepted: &[(&str, &str)] = &[
            // 1. IntToInt
            (
                "digital-root",
                "pub fn solve(n: i64) i64 { return if (n == 0) 0 else 1 + @rem(n - 1, 9); }\n",
            ),
            // 2. StrToBool
            (
                "valid-parentheses",
                r#"
pub fn solve(s: []const u8) bool {
    var st: [10005]u8 = undefined;
    var top: usize = 0;
    for (s) |c| {
        if (c == '(' or c == '[' or c == '{') {
            st[top] = c;
            top += 1;
        } else {
            if (top == 0) return false;
            top -= 1;
            const o = st[top];
            if ((c == ')' and o != '(') or (c == ']' and o != '[') or (c == '}' and o != '{')) return false;
        }
    }
    return top == 0;
}
"#,
            ),
            // 3. StrToInt
            (
                "longest-unique-substr",
                r#"
pub fn solve(s: []const u8) i64 {
    var last: [256]i64 = @splat(-1);
    var left: i64 = 0;
    var best: i64 = 0;
    for (s, 0..) |c, idx| {
        const r: i64 = @intCast(idx);
        if (last[c] >= left) left = last[c] + 1;
        last[c] = r;
        if (r - left + 1 > best) best = r - left + 1;
    }
    return best;
}
"#,
            ),
            // 4. StrToStr
            (
                "run-length-encode",
                r#"
const std = @import("std");
pub fn solve(alloc: std.mem.Allocator, s: []const u8) ![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    while (i < s.len) {
        var j = i;
        while (j < s.len and s[j] == s[i]) : (j += 1) {}
        try out.append(alloc, s[i]);
        var tmp: [32]u8 = undefined;
        const cnt = std.fmt.bufPrint(&tmp, "{d}", .{j - i}) catch unreachable;
        try out.appendSlice(alloc, cnt);
        i = j;
    }
    return out.toOwnedSlice(alloc);
}
"#,
            ),
            // 5. TwoStrsToBool
            (
                "valid-anagram",
                r#"
pub fn solve(a: []const u8, b: []const u8) bool {
    var cnt: [256]i32 = @splat(0);
    for (a) |c| cnt[c] += 1;
    for (b) |c| cnt[c] -= 1;
    for (cnt) |v| if (v != 0) return false;
    return true;
}
"#,
            ),
            // 6. IntVecToInt
            (
                "best-stock-profit",
                r#"
pub fn solve(nums: []const i64) i64 {
    if (nums.len == 0) return 0;
    var low = nums[0];
    var best: i64 = 0;
    for (nums[1..]) |p| {
        if (p < low) low = p else if (p - low > best) best = p - low;
    }
    return best;
}
"#,
            ),
            // 7. IntVecIntToInt
            (
                "subarray-sum-k",
                r#"
pub fn solve(nums: []const i64, k: i64) i64 {
    var ans: i64 = 0;
    for (0..nums.len) |i| {
        var sum: i64 = 0;
        for (i..nums.len) |j| {
            sum += nums[j];
            if (sum == k) ans += 1;
        }
    }
    return ans;
}
"#,
            ),
            // 8. IntVecToIntVec
            (
                "move-zeroes",
                r#"
const std = @import("std");
pub fn solve(alloc: std.mem.Allocator, nums: []const i64) ![]const i64 {
    const out = try alloc.alloc(i64, nums.len);
    var pos: usize = 0;
    for (nums) |x| {
        if (x != 0) {
            out[pos] = x;
            pos += 1;
        }
    }
    while (pos < nums.len) : (pos += 1) out[pos] = 0;
    return out;
}
"#,
            ),
            // 9. IntVecIntToPair
            (
                "two-sum",
                r#"
pub fn solve(nums: []const i64, k: i64) [2]i64 {
    for (0..nums.len) |i| {
        for (i + 1..nums.len) |j| {
            if (nums[i] + nums[j] == k) return .{ @intCast(i), @intCast(j) };
        }
    }
    return .{ 0, 0 };
}
"#,
            ),
        ];

        assert_eq!(c_accepted.len(), 9);
        assert_eq!(zig_accepted.len(), 9);

        // Verify ALL 9 signatures pass in C AND default C starter templates compile cleanly (rejected as WrongAnswer).
        for &(id, code) in c_accepted {
            let ex = find_ex(id);
            let rep_ok = evaluate_submission(&ex, PracticeLanguage::C, code, &c_tc, RunLimits::default(), &cancel);
            assert_eq!(
                rep_ok.verdict,
                SubmissionVerdict::Accepted,
                "C accepted failed for {id}: {} | stderr: {} | cases: {:?}",
                rep_ok.summary,
                rep_ok.stderr,
                rep_ok.cases
            );

            let starter = starter_code(&ex, PracticeLanguage::C);
            let rep_starter = evaluate_submission(&ex, PracticeLanguage::C, &starter, &c_tc, RunLimits::default(), &cancel);
            assert_eq!(
                rep_starter.verdict,
                SubmissionVerdict::WrongAnswer,
                "C starter should compile and fail correctness for {id}: {:?} | stderr: {}",
                rep_starter.verdict,
                rep_starter.stderr
            );
        }

        // Verify ALL 9 signatures pass in Zig AND default Zig starter templates compile cleanly (rejected as WrongAnswer).
        for &(id, code) in zig_accepted {
            let ex = find_ex(id);
            let rep_ok = evaluate_submission(&ex, PracticeLanguage::Zig, code, &zig_tc, RunLimits::default(), &cancel);
            assert_eq!(
                rep_ok.verdict,
                SubmissionVerdict::Accepted,
                "Zig accepted failed for {id}: {} | stderr: {} | cases: {:?}",
                rep_ok.summary,
                rep_ok.stderr,
                rep_ok.cases
            );

            let starter = starter_code(&ex, PracticeLanguage::Zig);
            let rep_starter = evaluate_submission(&ex, PracticeLanguage::Zig, &starter, &zig_tc, RunLimits::default(), &cancel);
            assert_eq!(
                rep_starter.verdict,
                SubmissionVerdict::WrongAnswer,
                "Zig starter should compile and fail correctness for {id}: {:?} | stderr: {}",
                rep_starter.verdict,
                rep_starter.stderr
            );
        }

        // Verify CompileError rejection for both C and Zig.
        let dr = find_ex("digital-root");
        let c_bad = evaluate_submission(&dr, PracticeLanguage::C, "long long solve(long long n) { return unknown_var; }", &c_tc, RunLimits::default(), &cancel);
        assert_eq!(c_bad.verdict, SubmissionVerdict::CompileError);
        assert!(c_bad.stderr.contains("unknown_var"));

        let zig_bad = evaluate_submission(&dr, PracticeLanguage::Zig, "pub fn solve(n: i64) i64 { _ = n; return unknown_ident; }", &zig_tc, RunLimits::default(), &cancel);
        assert_eq!(zig_bad.verdict, SubmissionVerdict::CompileError);
        assert!(zig_bad.stderr.contains("unknown_ident"));

        // Verify actionable compiler-unavailable messages for both C and Zig.
        let c_missing = unavailable_status(PracticeLanguage::C, "cc / gcc / clang", PracticeLanguage::C.install_hint());
        let rep_c_unavail = evaluate_submission(&dr, PracticeLanguage::C, "", &c_missing, RunLimits::default(), &cancel);
        assert_eq!(rep_c_unavail.verdict, SubmissionVerdict::ToolchainUnavailable);
        assert!(rep_c_unavail.summary.contains("cc") && rep_c_unavail.summary.contains("PATH"));

        let zig_missing = unavailable_status(PracticeLanguage::Zig, "zig", PracticeLanguage::Zig.install_hint());
        let rep_zig_unavail = evaluate_submission(&dr, PracticeLanguage::Zig, "", &zig_missing, RunLimits::default(), &cancel);
        assert_eq!(rep_zig_unavail.verdict, SubmissionVerdict::ToolchainUnavailable);
        assert!(rep_zig_unavail.summary.contains("zig") && rep_zig_unavail.summary.contains("PATH"));
    }

    #[cfg(unix)]
    fn pid_is_alive(pid: i32) -> bool {
        if pid <= 0 {
            return false;
        }
        if unsafe { libc::kill(pid, 0) } != 0 {
            return false;
        }
        // In container/sandbox PID namespaces, a killed grandchild reparented to the subreaper
        // can briefly remain a zombie (`State: Z`) before reaping while `kill(pid, 0)` still returns 0.
        #[cfg(target_os = "linux")]
        if let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) {
            if status
                .lines()
                .any(|l| l.starts_with("State:") && (l.contains("\tZ") || l.contains("\tX")))
            {
                return false;
            }
        }
        true
    }

    #[cfg(unix)]
    #[test]
    fn wall_timeout_covers_descendants_retaining_pipes_and_cleans_up_on_exit_and_cancel() {
        let reg = ToolchainRegistry::detect_full();
        let py = reg.get(PracticeLanguage::Python);
        assert!(py.available);
        let ex = all_exercises()
            .into_iter()
            .find(|e| e.id == "digital-root")
            .unwrap();

        // 1. Direct child spawns a 3-second sleeping descendant inheriting stdout/stderr pipes
        //    and exits immediately. A 150 ms wall limit MUST return TimeLimitExceeded promptly
        //    and kill the descendant process.
        let pid_file_1 = std::env::temp_dir().join(format!(
            "gtuim-desc-pipe-pid-{}.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&pid_file_1);
        let src_pipe_desc = format!(
            r#"import subprocess, sys
_spawned = False
def solve(n):
    global _spawned
    if not _spawned:
        _spawned = True
        p = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(5)"])
        with open({:?}, "w") as f:
            f.write(str(p.pid))
    return 0 if n == 0 else 1 + (n - 1) % 9
"#,
            pid_file_1.to_string_lossy()
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let limits = RunLimits {
            exec_timeout: Duration::from_millis(150),
            exec_cpu_secs: 2,
            ..RunLimits::default()
        };
        let t0 = Instant::now();
        let rep = evaluate_submission(&ex, PracticeLanguage::Python, &src_pipe_desc, &py, limits, &cancel);
        let elapsed = t0.elapsed();
        assert_eq!(rep.verdict, SubmissionVerdict::TimeLimitExceeded);
        assert!(
            elapsed < Duration::from_millis(1200),
            "descendant retaining pipes hung past wall limit: {elapsed:?}"
        );
        if let Ok(raw) = fs::read_to_string(&pid_file_1) {
            if let Ok(desc_pid) = raw.trim().parse::<i32>() {
                thread::sleep(Duration::from_millis(50));
                assert!(
                    !pid_is_alive(desc_pid),
                    "descendant {desc_pid} survived wall timeout"
                );
            }
        }
        let _ = fs::remove_file(&pid_file_1);

        // 2. Direct child spawns a detached descendant with DEVNULL pipes and exits cleanly.
        //    Surviving group member MUST be killed on normal exit too!
        let pid_file_2 = std::env::temp_dir().join(format!(
            "gtuim-desc-detached-pid-{}.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&pid_file_2);
        let src_detached = format!(
            r#"import subprocess, sys
_spawned = False
def solve(n):
    global _spawned
    if not _spawned:
        _spawned = True
        p = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(5)"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        with open({:?}, "w") as f:
            f.write(str(p.pid))
    return 0 if n == 0 else 1 + (n - 1) % 9
"#,
            pid_file_2.to_string_lossy()
        );
        let rep2 = evaluate_submission(
            &ex,
            PracticeLanguage::Python,
            &src_detached,
            &py,
            RunLimits::default(),
            &cancel,
        );
        assert_eq!(rep2.verdict, SubmissionVerdict::Accepted);
        if let Ok(raw) = fs::read_to_string(&pid_file_2) {
            if let Ok(desc_pid) = raw.trim().parse::<i32>() {
                thread::sleep(Duration::from_millis(50));
                assert!(
                    !pid_is_alive(desc_pid),
                    "detached descendant {desc_pid} survived normal parent exit"
                );
            }
        }
        let _ = fs::remove_file(&pid_file_2);

        // 3. External cancellation terminates actual live OS child promptly.
        let pid_file_3 = std::env::temp_dir().join(format!(
            "gtuim-cancel-pid-{}.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&pid_file_3);
        let src_cancel = format!(
            r#"import os, time
def solve(n):
    with open({:?}, "w") as f:
        f.write(str(os.getpid()))
    time.sleep(10)
    return 0
"#,
            pid_file_3.to_string_lossy()
        );
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_trigger = cancel_flag.clone();
        let pid_file_wait = pid_file_3.clone();
        let trigger_thread = thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(2) {
                if pid_file_wait.exists() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            cancel_trigger.store(true, Ordering::Relaxed);
        });
        let rep3 = evaluate_submission(
            &ex,
            PracticeLanguage::Python,
            &src_cancel,
            &py,
            RunLimits::default(),
            &cancel_flag,
        );
        trigger_thread.join().unwrap();
        assert_eq!(rep3.verdict, SubmissionVerdict::Cancelled);
        let raw_pid = fs::read_to_string(&pid_file_3).expect("child wrote its pid");
        let child_pid = raw_pid.trim().parse::<i32>().expect("numeric pid");
        thread::sleep(Duration::from_millis(50));
        assert!(
            !pid_is_alive(child_pid),
            "cancelled child {child_pid} is still alive"
        );
        let _ = fs::remove_file(&pid_file_3);

        // 4. On Linux, verify PR_SET_PDEATHSIG(SIGKILL) is actually armed on the child process
        //    so host process death immediately terminates the running submission.
        #[cfg(target_os = "linux")]
        {
            let check_pdeath = r#"import ctypes
def solve(n):
    libc = ctypes.CDLL("libc.so.6")
    sig = ctypes.c_int(0)
    rc = libc.prctl(2, ctypes.byref(sig))
    if rc != 0 or sig.value != 9:
        raise RuntimeError(f"expected PR_GET_PDEATHSIG==SIGKILL(9), got rc={rc} sig={sig.value}")
    return 0 if n == 0 else 1 + (n - 1) % 9
"#;
            let no_cancel = Arc::new(AtomicBool::new(false));
            let rep4 = evaluate_submission(
                &ex,
                PracticeLanguage::Python,
                check_pdeath,
                &py,
                RunLimits::default(),
                &no_cancel,
            );
            assert_eq!(
                rep4.verdict,
                SubmissionVerdict::Accepted,
                "PR_SET_PDEATHSIG verification failed: {} | cases: {:?}",
                rep4.summary,
                rep4.cases
            );
        }
    }
}
