use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::output::{CYAN, RESET, YELLOW, should_use_colors};

pub trait Progress {
    fn task_started(&mut self, name: &str);
    fn task_done(&mut self);
    fn bytes_transferred(&mut self, n: u64);
    fn set_total(&mut self, n: Option<u64>);
    #[allow(dead_code)]
    fn info(&mut self, msg: &str);
    #[allow(dead_code)]
    fn warn(&mut self, msg: &str);
    #[allow(dead_code)]
    fn cancel(&mut self);
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
        let line = if should_use_colors() {
            format!("{CYAN}\u{2192}{RESET} {msg}")
        } else {
            format!("\u{2192} {msg}")
        };
        self.pb.println(line);
    }

    fn warn(&mut self, msg: &str) {
        let line = if should_use_colors() {
            format!("{YELLOW}\u{26A0}{RESET} {msg}")
        } else {
            format!("\u{26A0} {msg}")
        };
        self.pb.println(line);
    }

    fn cancel(&mut self) {
        self.pb.finish_and_clear();
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
    Cancel,
}

#[cfg(test)]
pub struct MockProgress {
    events: Vec<ProgressEvent>,
}

#[cfg(test)]
impl MockProgress {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn events(&self) -> &[ProgressEvent] {
        &self.events
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
        self.events
            .push(ProgressEvent::TaskStarted(name.to_string()));
    }

    fn task_done(&mut self) {
        self.events.push(ProgressEvent::TaskDone);
    }

    fn bytes_transferred(&mut self, n: u64) {
        self.events.push(ProgressEvent::BytesTransferred(n));
    }

    fn set_total(&mut self, n: Option<u64>) {
        self.events.push(ProgressEvent::SetTotal(n));
    }

    fn info(&mut self, msg: &str) {
        self.events.push(ProgressEvent::Info(msg.to_string()));
    }

    fn warn(&mut self, msg: &str) {
        self.events.push(ProgressEvent::Warn(msg.to_string()));
    }

    fn cancel(&mut self) {
        self.events.push(ProgressEvent::Cancel);
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
    fn mock_records_cancel() {
        let mut p = MockProgress::new();
        p.task_started("rsync");
        p.cancel();
        assert_eq!(
            p.events(),
            &[
                ProgressEvent::TaskStarted("rsync".to_string()),
                ProgressEvent::Cancel,
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
        p.task_done();
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
}
