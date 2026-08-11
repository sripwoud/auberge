use clap::ValueEnum;
use eyre::{Context, Result};
use std::env;
use std::io::{BufRead, BufReader, IsTerminal};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tabled::{Table, Tabled, settings::Style as TableStyle};

/// Shared output format for commands that produce structured output.
///
/// See ADR-0004: only commands with at least one load-bearing JSON field
/// expose `--output`. Drop-in serialisation surface; nothing more.
#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

static VERBOSE: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);
static NO_COLOR_FLAG: AtomicBool = AtomicBool::new(false);

// Shared across this binary's test suite: any test that mutates global state
// (env vars, NO_COLOR_FLAG, VERBOSE) must hold this lock so concurrent tests
// in other modules don't race on those reads.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

// RAII guard that snapshots an env var on construction and restores it on Drop.
// Callers MUST hold TEST_LOCK so env mutations are serialized across this binary's tests.
#[cfg(test)]
pub(crate) struct EnvVarGuard {
    key: &'static str,
    prev: Option<String>,
}

#[cfg(test)]
impl EnvVarGuard {
    pub(crate) fn unset(key: &'static str) -> Self {
        let prev = env::var(key).ok();
        // SAFETY: caller holds TEST_LOCK; no concurrent env access in this binary's tests.
        unsafe { env::remove_var(key) };
        Self { key, prev }
    }

    pub(crate) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let prev = env::var(key).ok();
        // SAFETY: caller holds TEST_LOCK; no concurrent env access in this binary's tests.
        unsafe { env::set_var(key, value) };
        Self { key, prev }
    }
}

#[cfg(test)]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: caller holds TEST_LOCK; no concurrent env access in this binary's tests.
        match &self.prev {
            Some(v) => unsafe { env::set_var(self.key, v) },
            None => unsafe { env::remove_var(self.key) },
        }
    }
}

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

pub fn set_quiet(v: bool) {
    QUIET.store(v, Ordering::Relaxed);
}

pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub(crate) fn set_no_color(v: bool) {
    NO_COLOR_FLAG.store(v, Ordering::Relaxed);
}

pub(crate) fn should_use_colors() -> bool {
    if NO_COLOR_FLAG.load(Ordering::Relaxed) {
        return false;
    }
    if env::var("NO_COLOR").is_ok() {
        return false;
    }
    if let Ok(term) = env::var("TERM")
        && term == "dumb"
    {
        return false;
    }
    std::io::stderr().is_terminal()
}

pub(crate) const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
pub(crate) const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
pub(crate) const RESET: &str = "\x1b[0m";

pub fn success(msg: &str) {
    if is_quiet() {
        return;
    }
    if should_use_colors() {
        eprintln!("{}✓{} {}", GREEN, RESET, msg);
    } else {
        eprintln!("✓ {}", msg);
    }
}

pub fn warn(msg: &str) {
    if is_quiet() {
        return;
    }
    if should_use_colors() {
        eprintln!("{}⚠{} {}", YELLOW, RESET, msg);
    } else {
        eprintln!("⚠ {}", msg);
    }
}

pub fn info(msg: &str) {
    if is_quiet() {
        return;
    }
    if should_use_colors() {
        eprintln!("{}→{} {}", CYAN, RESET, msg);
    } else {
        eprintln!("→ {}", msg);
    }
}

pub struct SubprocessResult {
    pub status: ExitStatus,
    pub lines_written: usize,
    pub last_stderr: String,
}

impl SubprocessResult {
    pub fn error(&self, context: impl std::fmt::Display) -> eyre::Report {
        let tail = self.last_stderr.trim();
        if tail.is_empty() {
            eyre::eyre!("{context}")
        } else {
            eyre::eyre!("{context}: {tail}")
        }
    }
}

const STDERR_TAIL_LINES: usize = 20;

fn push_stderr_tail(tail: &mut Vec<String>, line: String) {
    tail.push(line);
    if tail.len() > STDERR_TAIL_LINES {
        tail.remove(0);
    }
}

const CURSOR_UP: &str = "\x1b[A";
const ERASE_LINE: &str = "\x1b[2K";

// Cursor movement and line erasure are intentionally not gated by --no-color:
// per https://no-color.org/ the contract is "suppress color output", not all
// terminal control. They are still skipped on non-TTY stderr to avoid leaking
// escape codes into pipes and log files.
pub fn clear_subprocess_lines(count: usize) {
    if count == 0 || !std::io::stderr().is_terminal() {
        return;
    }
    for _ in 0..count {
        eprint!("{CURSOR_UP}{ERASE_LINE}");
    }
}

fn emit_subprocess_line(label: &str, line: &str) -> bool {
    if line.trim().is_empty() {
        return false;
    }
    if should_use_colors() {
        eprintln!("{DIM}  {label} | {line}{RESET}");
    } else {
        eprintln!("  {label} | {line}");
    }
    true
}

pub fn subprocess_output(label: &str, text: &str) -> usize {
    if !is_verbose() {
        return 0;
    }
    let mut count = 0;
    for line in text.lines() {
        if emit_subprocess_line(label, line) {
            count += 1;
        }
    }
    count
}

pub fn run_piped(label: &str, cmd: &mut Command) -> Result<SubprocessResult> {
    let verbose = is_verbose();

    // stderr is piped even when not verbose: it is the only account of why a
    // subprocess failed, and nulling it leaves every caller bailing causelessly.
    // It is drained on this thread, so an unread pipe can never stall the child.
    cmd.stderr(Stdio::piped());
    cmd.stdout(if verbose {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = cmd.spawn().wrap_err("failed to spawn subprocess")?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take().unwrap();

    let line_count = AtomicUsize::new(0);
    let mut stderr_tail: Vec<String> = Vec::new();

    std::thread::scope(|s| {
        if let Some(stdout) = stdout {
            s.spawn(|| {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if emit_subprocess_line(label, &line) {
                        line_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }

        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if verbose && emit_subprocess_line(label, &line) {
                line_count.fetch_add(1, Ordering::Relaxed);
            }
            push_stderr_tail(&mut stderr_tail, line);
        }
    });

    let status = child.wait().wrap_err("failed to wait on subprocess")?;
    Ok(SubprocessResult {
        status,
        lines_written: line_count.load(Ordering::Relaxed),
        last_stderr: stderr_tail.join("\n"),
    })
}

pub struct ProgressResult {
    pub status: ExitStatus,
    pub last_stderr: String,
}

pub fn print_table<T: Tabled>(data: &[T]) {
    if data.is_empty() {
        return;
    }

    let mut table = Table::new(data);
    table.with(TableStyle::modern());

    println!("{}", table);
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else {
        let mins = seconds / 60;
        let secs = seconds % 60;
        format!("{}m {}s", mins, secs)
    }
}

pub fn stream_command_stdout(
    label: &str,
    cmd: &mut Command,
    mut line_handler: impl FnMut(&str),
) -> Result<ProgressResult> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().wrap_err("failed to spawn subprocess")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let verbose = is_verbose();

    let stderr_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_tail_clone = Arc::clone(&stderr_tail);
    let label_owned = label.to_owned();

    std::thread::scope(|s| {
        s.spawn(move || {
            for line_result in BufReader::new(stderr).lines() {
                let Ok(line) = line_result else { continue };
                if verbose {
                    emit_subprocess_line(&label_owned, &line);
                }
                push_stderr_tail(&mut stderr_tail_clone.lock().unwrap(), line);
            }
        });

        for line_result in BufReader::new(stdout).lines() {
            let Ok(line) = line_result else { continue };
            if verbose {
                emit_subprocess_line(label, &line);
            }
            line_handler(&line);
        }
    });

    let status = child.wait().wrap_err("failed to wait on subprocess")?;
    let last_stderr = stderr_tail.lock().unwrap().join("\n");
    Ok(ProgressResult {
        status,
        last_stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_flag_overrides_tty_check() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _env_guard = EnvVarGuard::unset("NO_COLOR");
        set_no_color(true);
        assert!(!should_use_colors());
        set_no_color(false);
    }

    #[test]
    fn no_color_env_disables_colors_when_flag_cleared() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _env_guard = EnvVarGuard::unset("NO_COLOR");
        // SAFETY: caller holds TEST_LOCK; no concurrent env access in this binary's tests.
        unsafe { env::set_var("NO_COLOR", "1") };
        set_no_color(false);
        assert!(!should_use_colors());
    }

    #[test]
    fn term_dumb_disables_colors_when_flag_and_env_absent() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _no_color_guard = EnvVarGuard::unset("NO_COLOR");
        let _term_guard = EnvVarGuard::unset("TERM");
        // SAFETY: caller holds TEST_LOCK; no concurrent env access in this binary's tests.
        unsafe { env::set_var("TERM", "dumb") };
        set_no_color(false);
        assert!(!should_use_colors());
    }

    #[test]
    fn verbose_defaults_to_false() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        assert!(!is_verbose());
    }

    #[test]
    fn set_verbose_true() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(true);
        assert!(is_verbose());
        set_verbose(false);
    }

    #[test]
    fn set_verbose_false_after_true() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(true);
        assert!(is_verbose());
        set_verbose(false);
        assert!(!is_verbose());
    }

    #[test]
    fn quiet_defaults_to_false() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_quiet(false);
        assert!(!is_quiet());
    }

    #[test]
    fn set_quiet_true() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_quiet(true);
        assert!(is_quiet());
        set_quiet(false);
    }

    #[test]
    fn set_quiet_false_after_true() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_quiet(true);
        assert!(is_quiet());
        set_quiet(false);
        assert!(!is_quiet());
    }

    #[test]
    fn subprocess_output_noop_when_not_verbose() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        subprocess_output("test", "should not appear");
    }

    #[test]
    fn subprocess_output_skips_blank_lines() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(true);
        let count = subprocess_output("test", "\n\n   \n");
        assert_eq!(count, 0);
        set_verbose(false);
    }

    #[test]
    fn subprocess_output_handles_empty_string() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(true);
        let count = subprocess_output("test", "");
        assert_eq!(count, 0);
        set_verbose(false);
    }

    #[test]
    fn subprocess_output_counts_lines() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(true);
        let count = subprocess_output("test", "line1\nline2\n\nline3");
        assert_eq!(count, 3);
        set_verbose(false);
    }

    #[test]
    fn subprocess_output_returns_zero_when_not_verbose() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        let count = subprocess_output("test", "line1\nline2");
        assert_eq!(count, 0);
    }

    #[test]
    fn run_piped_suppresses_when_not_verbose() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        let result = run_piped("true", Command::new("true").arg("")).unwrap();
        assert!(result.status.success());
        assert_eq!(result.lines_written, 0);
    }

    #[test]
    fn run_piped_streams_when_verbose() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(true);
        let result = run_piped("echo", Command::new("echo").arg("hello")).unwrap();
        assert!(result.status.success());
        assert!(result.lines_written > 0);
        set_verbose(false);
    }

    #[test]
    fn run_piped_captures_stderr_when_not_verbose() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        let result = run_piped(
            "sh",
            Command::new("sh")
                .arg("-c")
                .arg("echo 'Operation not permitted' >&2; exit 1"),
        )
        .unwrap();
        assert!(!result.status.success());
        assert_eq!(result.lines_written, 0);
        assert!(result.last_stderr.contains("Operation not permitted"));
    }

    #[test]
    fn run_piped_captures_stderr_when_verbose() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(true);
        let result = run_piped(
            "sh",
            Command::new("sh")
                .arg("-c")
                .arg("echo progress; echo 'Operation not permitted' >&2; exit 1"),
        );
        set_verbose(false);
        let result = result.unwrap();
        assert!(!result.status.success());
        assert_eq!(result.lines_written, 2);
        assert!(result.last_stderr.contains("Operation not permitted"));
    }

    #[test]
    fn run_piped_bounds_stderr_tail() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        let result = run_piped(
            "sh",
            Command::new("sh").arg("-c").arg("seq 1 100 >&2; exit 1"),
        )
        .unwrap();
        let lines: Vec<&str> = result.last_stderr.lines().collect();
        assert_eq!(lines.len(), STDERR_TAIL_LINES);
        assert_eq!(lines.first(), Some(&"81"));
        assert_eq!(lines.last(), Some(&"100"));
    }

    #[test]
    fn subprocess_error_names_the_stderr_tail() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        let result = run_piped(
            "sh",
            Command::new("sh").arg("-c").arg("echo boom >&2; exit 1"),
        )
        .unwrap();
        assert_eq!(
            result.error("rsync failed").to_string(),
            "rsync failed: boom"
        );
    }

    #[test]
    fn subprocess_error_falls_back_to_bare_context() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_verbose(false);
        let result = run_piped("false", &mut Command::new("false")).unwrap();
        assert!(!result.status.success());
        assert_eq!(result.error("rsync failed").to_string(), "rsync failed");
    }

    #[test]
    fn clear_subprocess_lines_zero_is_noop() {
        clear_subprocess_lines(0);
    }

    #[test]
    fn stream_command_stdout_invokes_handler_for_each_line() {
        let lines_seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let lines_clone = std::sync::Arc::clone(&lines_seen);

        let result = stream_command_stdout(
            "test",
            Command::new("sh")
                .arg("-c")
                .arg("echo line1; echo line2; echo line3"),
            move |line| {
                lines_clone.lock().unwrap().push(line.to_string());
            },
        )
        .unwrap();

        assert!(result.status.success());
        let seen = lines_seen.lock().unwrap();
        assert_eq!(*seen, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn stream_command_stdout_captures_last_lines_on_failure() {
        let result = stream_command_stdout(
            "test",
            Command::new("sh")
                .arg("-c")
                .arg("echo output details >&2; exit 1"),
            |_line| {},
        )
        .unwrap();

        assert!(!result.status.success());
        assert!(result.last_stderr.contains("output details"));
    }
}
