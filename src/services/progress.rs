use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::output::{CYAN, GREEN, RESET, YELLOW, is_quiet, should_use_colors};

/// The event stream a runner reports through.
///
/// Every message a runner emits is one of these, so its whole output is
/// observable from a test. A runner that reaches `eprintln!` or `output::*`
/// directly has an output path no `MockProgress` can see (ADR-0047).
pub trait Progress {
    fn task_started(&mut self, name: &str);
    fn task_done(&mut self);
    fn bytes_transferred(&mut self, n: u64);
    fn set_total(&mut self, n: Option<u64>);
    fn info(&mut self, msg: &str);
    fn warn(&mut self, msg: &str);
    /// An item finished.
    fn success(&mut self, msg: &str);
    /// An item failed and the run carried on without it. Reaches the terminal
    /// under `--quiet`: a failure is the only account of why a run stopped
    /// short, which is why the `eprintln!` it replaced was ungated too.
    fn error(&mut self, msg: &str);
    /// A subprocess's own report, verbatim, because decorating it would corrupt
    /// the thing the command was run to produce. Ungated for the same reason
    /// stdout is: it is data, not chrome.
    fn line(&mut self, text: &str);
}

pub struct TerminalProgress {
    pb: ProgressBar,
    has_total: bool,
}

impl TerminalProgress {
    pub fn new(initial_message: &str) -> Self {
        let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        apply_spinner_style(&pb);
        pb.set_message(initial_message.to_string());
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        crate::signal::register_progress_bar(&pb);
        Self {
            pb,
            has_total: false,
        }
    }

    #[cfg(test)]
    pub fn hidden(initial_message: &str) -> Self {
        let pb = ProgressBar::hidden();
        pb.set_message(initial_message.to_string());
        Self {
            pb,
            has_total: false,
        }
    }

    /// A decoration line above the bar, unless `--quiet`.
    ///
    /// One gate for the three events that carry chrome, because
    /// `output::success`, `output::info` and `output::warn` each open with the
    /// same check and this renderer stands in for all three. Gating two of the
    /// three is how `backup push`'s opening line gets to escape `--quiet`.
    fn chrome(&self, line: String) {
        if is_quiet() {
            return;
        }
        self.pb.println(line);
    }
}

impl Progress for TerminalProgress {
    fn task_started(&mut self, name: &str) {
        self.pb.set_message(name.to_string());
    }

    fn task_done(&mut self) {
        self.pb.finish_and_clear();
    }

    fn bytes_transferred(&mut self, n: u64) {
        self.pb.set_position(n);
    }

    fn set_total(&mut self, n: Option<u64>) {
        match n {
            Some(total) => {
                if !self.has_total {
                    apply_bytes_style(&self.pb);
                    self.has_total = true;
                }
                self.pb.set_length(total);
            }
            None => {
                if self.has_total {
                    apply_spinner_style(&self.pb);
                    self.has_total = false;
                }
            }
        }
    }

    fn info(&mut self, msg: &str) {
        self.chrome(format_info_line(msg, should_use_colors()));
    }

    fn warn(&mut self, msg: &str) {
        self.chrome(format_warn_line(msg, should_use_colors()));
    }

    fn success(&mut self, msg: &str) {
        self.chrome(format_success_line(msg, should_use_colors()));
    }

    fn error(&mut self, msg: &str) {
        self.pb.println(format_error_line(msg));
    }

    fn line(&mut self, text: &str) {
        self.pb.println(text);
    }
}

fn format_info_line(msg: &str, use_colors: bool) -> String {
    if use_colors {
        format!("{CYAN}\u{2192}{RESET} {msg}")
    } else {
        format!("\u{2192} {msg}")
    }
}

fn format_success_line(msg: &str, use_colors: bool) -> String {
    if use_colors {
        format!("{GREEN}\u{2713}{RESET} {msg}")
    } else {
        format!("\u{2713} {msg}")
    }
}

// Uncoloured where `success` is green, because that is what the raw
// `eprintln!` this replaced printed, and the failure it reports is already
// repeated in the results table with the same glyph.
fn format_error_line(msg: &str) -> String {
    format!("\u{2717} {msg}")
}

fn format_warn_line(msg: &str, use_colors: bool) -> String {
    if use_colors {
        format!("{YELLOW}\u{26A0}{RESET} {msg}")
    } else {
        format!("\u{26A0} {msg}")
    }
}

// `should_use_colors()` here also gates non-color glyphs (braille spinner,
// block progress chars). Bundling is intentional: every code path that
// disables colors today (--no-color, NO_COLOR, TERM=dumb, non-TTY) is also
// the path most likely to render Unicode poorly. Split if a real use case
// needs colored ASCII or vice versa.
fn apply_spinner_style(pb: &ProgressBar) {
    if should_use_colors() {
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("\u{280B}\u{2819}\u{2839}\u{2838}\u{283C}\u{2834}\u{2826}\u{2827}\u{2807}\u{280F}")
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

fn apply_bytes_style(pb: &ProgressBar) {
    if should_use_colors() {
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner} {msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars(
                    "\u{2588}\u{2589}\u{258A}\u{258B}\u{258C}\u{258D}\u{258E}\u{258F} ",
                ),
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

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    TaskStarted(String),
    TaskDone,
    BytesTransferred(u64),
    SetTotal(Option<u64>),
    Info(String),
    Warn(String),
    Success(String),
    Error(String),
    Line(String),
}

#[cfg(test)]
pub struct MockProgress {
    events: std::rc::Rc<std::cell::RefCell<Vec<ProgressEvent>>>,
}

#[cfg(test)]
impl MockProgress {
    pub fn new() -> Self {
        Self {
            events: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        }
    }

    /// A second handle on the same event list.
    ///
    /// A Progress factory hands its `Box<dyn Progress>` to the runner, which
    /// drops it when the item is done; a list owned by the box would go with
    /// it. Sharing lets a test hold one recorder across every box a run asks
    /// for, and read the whole stream afterwards in the order it was emitted.
    pub fn share(&self) -> Self {
        Self {
            events: std::rc::Rc::clone(&self.events),
        }
    }

    pub fn events(&self) -> Vec<ProgressEvent> {
        self.events.borrow().clone()
    }

    fn record(&self, event: ProgressEvent) {
        self.events.borrow_mut().push(event);
    }
}

#[cfg(test)]
impl Default for MockProgress {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl Progress for MockProgress {
    fn task_started(&mut self, name: &str) {
        self.record(ProgressEvent::TaskStarted(name.to_string()));
    }

    fn task_done(&mut self) {
        self.record(ProgressEvent::TaskDone);
    }

    fn bytes_transferred(&mut self, n: u64) {
        self.record(ProgressEvent::BytesTransferred(n));
    }

    fn set_total(&mut self, n: Option<u64>) {
        self.record(ProgressEvent::SetTotal(n));
    }

    fn info(&mut self, msg: &str) {
        self.record(ProgressEvent::Info(msg.to_string()));
    }

    fn warn(&mut self, msg: &str) {
        self.record(ProgressEvent::Warn(msg.to_string()));
    }

    fn success(&mut self, msg: &str) {
        self.record(ProgressEvent::Success(msg.to_string()));
    }

    fn error(&mut self, msg: &str) {
        self.record(ProgressEvent::Error(msg.to_string()));
    }

    fn line(&mut self, text: &str) {
        self.record(ProgressEvent::Line(text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_records_task_lifecycle() {
        let mut p = MockProgress::new();
        p.task_started("rsync /opt/foo");
        p.bytes_transferred(1024);
        p.task_done();

        assert_eq!(
            p.events(),
            &[
                ProgressEvent::TaskStarted("rsync /opt/foo".to_string()),
                ProgressEvent::BytesTransferred(1024),
                ProgressEvent::TaskDone,
            ]
        );
    }

    #[test]
    fn mock_records_bytes_progression() {
        let mut p = MockProgress::new();
        p.set_total(Some(1_000_000));
        p.bytes_transferred(250_000);
        p.bytes_transferred(500_000);
        p.bytes_transferred(1_000_000);
        p.task_done();

        assert_eq!(
            p.events(),
            &[
                ProgressEvent::SetTotal(Some(1_000_000)),
                ProgressEvent::BytesTransferred(250_000),
                ProgressEvent::BytesTransferred(500_000),
                ProgressEvent::BytesTransferred(1_000_000),
                ProgressEvent::TaskDone,
            ]
        );
    }

    #[test]
    fn mock_records_info_and_warn() {
        let mut p = MockProgress::new();
        p.info("starting backup");
        p.warn("config missing optional key");
        assert_eq!(
            p.events(),
            &[
                ProgressEvent::Info("starting backup".to_string()),
                ProgressEvent::Warn("config missing optional key".to_string()),
            ]
        );
    }

    #[test]
    fn mock_records_results_and_verbatim_lines() {
        let mut p = MockProgress::new();
        p.success("baikal (1.20 MB)");
        p.error("paperless backup failed: connection refused");
        p.line("remove 2 snapshots");

        assert_eq!(
            p.events(),
            [
                ProgressEvent::Success("baikal (1.20 MB)".to_string()),
                ProgressEvent::Error("paperless backup failed: connection refused".to_string()),
                ProgressEvent::Line("remove 2 snapshots".to_string()),
            ]
        );
    }

    // A per-item factory hands each box away and the runner drops it, so a
    // recorder that did not share would see only the last item's events.
    #[test]
    fn mock_handles_record_into_one_ordered_list() {
        let recorder = MockProgress::new();

        for app in ["baikal", "bichon"] {
            let mut handed_away: Box<dyn Progress> = Box::new(recorder.share());
            handed_away.success(app);
        }

        assert_eq!(
            recorder.events(),
            [
                ProgressEvent::Success("baikal".to_string()),
                ProgressEvent::Success("bichon".to_string()),
            ]
        );
    }

    #[test]
    fn terminal_progress_lifecycle_does_not_panic() {
        let mut p = TerminalProgress::hidden("test");
        p.task_started("step 1");
        p.set_total(Some(100));
        p.bytes_transferred(50);
        p.set_total(None);
        p.info("informational");
        p.warn("warning");
        p.success("done");
        p.error("failed");
        p.line("verbatim");
        p.task_done();
        p.line("after the bar is cleared");
    }

    #[test]
    fn terminal_progress_set_total_swaps_styles_idempotently() {
        let mut p = TerminalProgress::hidden("test");
        p.set_total(Some(1024));
        p.set_total(Some(2048));
        p.set_total(None);
        p.set_total(None);
        p.task_done();
    }

    #[test]
    fn format_info_line_omits_color_when_disabled() {
        let line = format_info_line("hello", false);
        assert_eq!(line, "\u{2192} hello");
        assert!(!line.contains('\x1b'));
    }

    #[test]
    fn format_info_line_wraps_glyph_in_cyan_when_enabled() {
        let line = format_info_line("hello", true);
        assert_eq!(line, format!("{CYAN}\u{2192}{RESET} hello"));
    }

    #[test]
    fn format_success_line_omits_color_when_disabled() {
        let line = format_success_line("baikal (1.20 MB)", false);
        assert_eq!(line, "\u{2713} baikal (1.20 MB)");
        assert!(!line.contains('\x1b'));
    }

    #[test]
    fn format_success_line_wraps_glyph_in_green_when_enabled() {
        let line = format_success_line("baikal (1.20 MB)", true);
        assert_eq!(line, format!("{GREEN}\u{2713}{RESET} baikal (1.20 MB)"));
    }

    #[test]
    fn format_error_line_is_uncoloured() {
        let line = format_error_line("baikal backup failed: no route to host");
        assert_eq!(line, "\u{2717} baikal backup failed: no route to host");
        assert!(!line.contains('\x1b'));
    }

    #[test]
    fn format_warn_line_omits_color_when_disabled() {
        let line = format_warn_line("careful", false);
        assert_eq!(line, "\u{26A0} careful");
        assert!(!line.contains('\x1b'));
    }

    #[test]
    fn format_warn_line_wraps_glyph_in_yellow_when_enabled() {
        let line = format_warn_line("careful", true);
        assert_eq!(line, format!("{YELLOW}\u{26A0}{RESET} careful"));
    }
}
