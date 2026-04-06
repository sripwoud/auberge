use eyre::{Context, Result};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde::Deserialize;
use std::env;
use std::io::{BufRead, BufReader, IsTerminal};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

const RED: &str = "\x1b[31m";
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

pub fn error(msg: &str) {
    if should_use_colors() {
        eprintln!("{}✗{} {}", RED, RESET, msg);
    } else {
        eprintln!("✗ {}", msg);
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
    #[allow(dead_code)]
    pub stdout: String,
    pub last_stderr: String,
}

pub fn run_with_progress(
    label: &str,
    cmd: &mut Command,
    pb: &ProgressBar,
    line_handler: impl Fn(&str, &ProgressBar) + Send,
) -> Result<ProgressResult> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().wrap_err("failed to spawn subprocess")?;

    let stdout_handle = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let verbose = is_verbose();
    let label_owned = label.to_owned();

    let stdout_content = std::sync::Mutex::new(String::new());
    let stderr_tail = std::sync::Mutex::new(Vec::<String>::new());
    const MAX_STDERR_LINES: usize = 20;

    std::thread::scope(|s| {
        s.spawn(|| {
            let reader = BufReader::new(stdout_handle);
            let mut buf = stdout_content.lock().unwrap();
            for line in reader.lines().map_while(Result::ok) {
                if verbose {
                    emit_subprocess_line(&label_owned, &line);
                }
                buf.push_str(&line);
                buf.push('\n');
            }
        });

        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if verbose {
                emit_subprocess_line(label, &line);
            }
            {
                let mut tail = stderr_tail.lock().unwrap();
                tail.push(line.clone());
                if tail.len() > MAX_STDERR_LINES {
                    tail.remove(0);
                }
            }
            line_handler(&line, pb);
        }
    });

    let status = child.wait().wrap_err("failed to wait on subprocess")?;
    let stdout = stdout_content.into_inner().unwrap();
    let last_stderr = stderr_tail.into_inner().unwrap().join("\n");

    Ok(ProgressResult {
        status,
        stdout,
        last_stderr,
    })
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

pub fn spinner(msg: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();

    if should_use_colors() {
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner} {msg}")
                .unwrap(),
        );
    } else {
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("/-\\|")
                .template("{spinner} {msg}")
                .unwrap(),
        );
    }

    spinner.set_message(msg.to_string());
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner
}

pub fn progress_bar(msg: &str, total_bytes: Option<u64>) -> ProgressBar {
    match total_bytes {
        Some(total) => {
            let pb = ProgressBar::with_draw_target(Some(total), ProgressDrawTarget::stderr());
            if should_use_colors() {
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("{spinner} {msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")
                        .unwrap()
                        .progress_chars("█▉▊▋▌▍▎▏ "),
                );
            } else {
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("{msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")
                        .unwrap()
                        .progress_chars("#>-"),
                );
            }
            pb.set_message(msg.to_string());
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            pb
        }
        None => {
            let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
            if should_use_colors() {
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                        .template("{spinner} {msg}")
                        .unwrap(),
                );
            } else {
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .tick_chars("/-\\|")
                        .template("{spinner} {msg}")
                        .unwrap(),
                );
            }
            pb.set_message(msg.to_string());
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            pb
        }
    }
}

pub fn set_bytes_style(pb: &ProgressBar) {
    if should_use_colors() {
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner} {msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
    } else {
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
    }
}

pub fn set_percent_style(pb: &ProgressBar) {
    if should_use_colors() {
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner} {msg} [{bar:40}] {pos:>3}%")
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
    } else {
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40}] {pos:>3}%")
                .unwrap()
                .progress_chars("#>-"),
        );
    }
}

pub fn reset_to_spinner(pb: &ProgressBar) {
    if should_use_colors() {
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner} {msg}")
                .unwrap(),
        );
    } else {
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("/-\\|")
                .template("{spinner} {msg}")
                .unwrap(),
        );
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

#[derive(Debug, Deserialize)]
pub struct ResticStatus {
    pub percent_done: f64,
    pub total_bytes: Option<u64>,
    pub bytes_done: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ResticSummary {
    pub snapshot_id: String,
    #[allow(dead_code)]
    pub files_new: u64,
    #[allow(dead_code)]
    pub files_changed: u64,
    #[allow(dead_code)]
    pub data_added: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "message_type", rename_all = "lowercase")]
pub enum ResticMessage {
    Status(ResticStatus),
    Summary(ResticSummary),
}

#[derive(Debug, PartialEq)]
pub struct RsyncProgress {
    pub bytes_transferred: u64,
    pub percent: u8,
    pub speed: String,
    pub eta: String,
}

pub fn parse_restic_message(line: &str) -> Option<ResticMessage> {
    serde_json::from_str(line).ok()
}

pub fn parse_rsync_progress(line: &str) -> Option<RsyncProgress> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 4 {
        return None;
    }
    let percent_str = fields[1].strip_suffix('%')?;
    let percent: u8 = percent_str.parse().ok()?;
    let bytes_str = fields[0].replace(',', "");
    let bytes_transferred: u64 = bytes_str.parse().ok()?;
    Some(RsyncProgress {
        bytes_transferred,
        percent,
        speed: fields[2].to_string(),
        eta: fields[3].to_string(),
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
    fn parse_restic_status_line() {
        let line = r#"{"message_type":"status","percent_done":0.5,"total_bytes":1048576,"bytes_done":524288}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => {
                assert!((s.percent_done - 0.5).abs() < f64::EPSILON);
                assert_eq!(s.total_bytes, Some(1048576));
                assert_eq!(s.bytes_done, Some(524288));
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn parse_restic_summary_line() {
        let line = r#"{"message_type":"summary","snapshot_id":"abc123","files_new":10,"files_changed":2,"data_added":1048576}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Summary(s) => {
                assert_eq!(s.snapshot_id, "abc123");
                assert_eq!(s.files_new, 10);
                assert_eq!(s.files_changed, 2);
                assert_eq!(s.data_added, 1048576);
            }
            _ => panic!("expected Summary"),
        }
    }

    #[test]
    fn parse_restic_plain_text_returns_none() {
        assert!(parse_restic_message("using parent snapshot abc123").is_none());
    }

    #[test]
    fn parse_restic_malformed_json_returns_none() {
        assert!(parse_restic_message("{bad json}").is_none());
    }

    #[test]
    fn parse_restic_zero_percent() {
        let line = r#"{"message_type":"status","percent_done":0.0}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => {
                assert!((s.percent_done).abs() < f64::EPSILON);
                assert_eq!(s.total_bytes, None);
                assert_eq!(s.bytes_done, None);
            }
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn parse_restic_full_percent() {
        let line =
            r#"{"message_type":"status","percent_done":1.0,"total_bytes":100,"bytes_done":100}"#;
        let msg = parse_restic_message(line).unwrap();
        match msg {
            ResticMessage::Status(s) => assert!((s.percent_done - 1.0).abs() < f64::EPSILON),
            _ => panic!("expected Status"),
        }
    }

    #[test]
    fn parse_rsync_canonical_line() {
        let line = "    1,234,567  42%   12.34MB/s    0:01:23";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(
            p,
            RsyncProgress {
                bytes_transferred: 1234567,
                percent: 42,
                speed: "12.34MB/s".to_string(),
                eta: "0:01:23".to_string(),
            }
        );
    }

    #[test]
    fn parse_rsync_single_digit_percent() {
        let line = "  500  5%   1.00MB/s    0:00:01";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.percent, 5);
        assert_eq!(p.bytes_transferred, 500);
    }

    #[test]
    fn parse_rsync_100_percent() {
        let line = "  10,000,000 100%   50.00MB/s    0:00:00";
        let p = parse_rsync_progress(line).unwrap();
        assert_eq!(p.percent, 100);
    }

    #[test]
    fn parse_rsync_plain_text_returns_none() {
        assert!(parse_rsync_progress("sending incremental file list").is_none());
    }

    #[test]
    fn parse_rsync_too_few_fields_returns_none() {
        assert!(parse_rsync_progress("1234 42%").is_none());
    }

    #[test]
    fn run_with_progress_invokes_handler_for_each_stderr_line() {
        let pb = ProgressBar::hidden();
        let lines_seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let lines_clone = std::sync::Arc::clone(&lines_seen);

        let result = run_with_progress(
            "test",
            Command::new("sh")
                .arg("-c")
                .arg("echo line1 >&2; echo line2 >&2; echo line3 >&2"),
            &pb,
            move |line, _pb| {
                lines_clone.lock().unwrap().push(line.to_string());
            },
        )
        .unwrap();

        assert!(result.status.success());
        let seen = lines_seen.lock().unwrap();
        assert_eq!(*seen, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn run_with_progress_captures_last_stderr() {
        let pb = ProgressBar::hidden();

        let result = run_with_progress(
            "test",
            Command::new("sh")
                .arg("-c")
                .arg("echo error details >&2; exit 1"),
            &pb,
            |_line, _pb| {},
        )
        .unwrap();

        assert!(!result.status.success());
        assert!(result.last_stderr.contains("error details"));
    }
}
