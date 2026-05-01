use eyre::{Context, Result};
use std::env;
use std::io::{BufRead, BufReader, IsTerminal};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tabled::{Table, Tabled, settings::Style as TableStyle};

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

fn should_use_colors() -> bool {
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

const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

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
            for line in reader.lines().flatten() {
                if emit_subprocess_line(&label_owned, &line) {
                    count_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        let reader = BufReader::new(stderr);
        for line in reader.lines().flatten() {
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
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

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
