use eyre::Result;
use indicatif::ProgressBar;
use std::sync::{Arc, Mutex};

static ACTIVE_BAR: Mutex<Option<ProgressBar>> = Mutex::new(None);

pub fn register_progress_bar(pb: &ProgressBar) {
    if let Ok(mut guard) = ACTIVE_BAR.lock() {
        *guard = Some(pb.clone());
    }
}

#[allow(dead_code)]
pub fn unregister_progress_bar() {
    if let Ok(mut guard) = ACTIVE_BAR.lock() {
        *guard = None;
    }
}

fn cleanup_progress_bar() {
    if let Ok(guard) = ACTIVE_BAR.lock() {
        if let Some(pb) = guard.as_ref() {
            pb.finish_and_clear();
        }
    }
}

pub fn with_ctrlc<F: FnOnce() -> Result<()>>(f: F) -> Result<()> {
    let interrupted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let interrupted_clone = Arc::clone(&interrupted);

    let handler_result = ctrlc::set_handler(move || {
        interrupted_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        cleanup_progress_bar();
        std::process::exit(130);
    });

    match handler_result {
        Ok(_) => {}
        Err(ctrlc::Error::MultipleHandlers) => {}
        Err(e) => return Err(eyre::eyre!("Failed to set Ctrl-C handler: {}", e)),
    }

    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indicatif::{ProgressBar, ProgressDrawTarget};

    #[test]
    fn test_register_and_cleanup_progress_bar() {
        let pb = ProgressBar::with_draw_target(Some(10), ProgressDrawTarget::hidden());
        assert!(!pb.is_finished());

        register_progress_bar(&pb);
        cleanup_progress_bar();

        assert!(pb.is_finished());
    }

    #[test]
    fn test_unregister_progress_bar() {
        let pb = ProgressBar::with_draw_target(Some(10), ProgressDrawTarget::hidden());
        register_progress_bar(&pb);
        unregister_progress_bar();

        let guard = ACTIVE_BAR.lock().unwrap();
        assert!(guard.is_none());
    }

    #[test]
    fn test_with_ctrlc_runs_closure() {
        let result = with_ctrlc(|| {
            let x: i32 = 2 + 2;
            assert_eq!(x, 4);
            Ok(())
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_with_ctrlc_propagates_error() {
        let result = with_ctrlc(|| eyre::bail!("test error"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test error"));
    }

    #[test]
    fn test_cleanup_without_registered_bar() {
        unregister_progress_bar();
        // Should not panic when no bar is registered
        cleanup_progress_bar();
    }
}
