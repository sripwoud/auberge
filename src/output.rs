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
static NO_COLOR_FLAG: AtomicBool = AtomicBool::new(false);

// Shared across this binary's test suite: any test that mutates global state
// (env vars, NO_COLOR_FLAG, VERBOSE) must hold this lock so concurrent tests
// in other modules don't race on those reads.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
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
    if should_use_colors() {
        eprintln!("{}✓{} {}", GREEN, RESET, msg);
    } else {
        eprintln!("✓ {}", msg);
    }
}

pub fn warn(msg: &str) {
    if should_use_colors() {
        eprintln!("{}⚠{} {}", YELLOW, RESET, msg);
    } else {
        eprintln!("⚠ {}", msg);
    }
}

pub fn info(msg: &str) {
    if should_use_colors() {
        eprintln!("{}→{} {}", CYAN, RESET, msg);
    } else {
        eprintln!("→ {}", msg);
    }
}

pub struct SubprocessResult {
    pub status: ExitStatus,
    pub lines_written: usize,
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
    if !is_verbose() {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let status = cmd.status().wrap_err("failed to run subprocess")?;
        return Ok(SubprocessResult {
            status,
            lines_written: 0,
        });
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().wrap_err("failed to spawn subprocess")?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let line_count = Arc::new(AtomicUsize::new(0));
    let label_owned = label.to_owned();
    let count_clone = Arc::clone(&line_count);

    std::thread::scope(|s| {
        s.spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if emit_subprocess_line(&label_owned, &line) {
                    count_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if emit_subprocess_line(label, &line) {
                line_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    let status = child.wait().wrap_err("failed to wait on subprocess")?;
    Ok(SubprocessResult {
        status,
        lines_written: line_count.load(Ordering::Relaxed),
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

    const MAX_LINES: usize = 20;

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
                let mut tail = stderr_tail_clone.lock().unwrap();
                tail.push(line);
                if tail.len() > MAX_LINES {
                    tail.remove(0);
                }
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

    // RAII guard that unsets an env var for the duration of a test and restores it on Drop.
    // Callers MUST hold TEST_LOCK so env mutations are serialized across this binary's tests.
    struct EnvVarGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvVarGuard {
        fn unset(key: &'static str) -> Self {
            let prev = env::var(key).ok();
            // SAFETY: caller holds TEST_LOCK; no concurrent env access in this binary's tests.
            unsafe { env::remove_var(key) };
            Self { key, prev }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: caller holds TEST_LOCK; no concurrent env access in this binary's tests.
            match &self.prev {
                Some(v) => unsafe { env::set_var(self.key, v) },
                None => unsafe { env::remove_var(self.key) },
            }
        }
    }

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
