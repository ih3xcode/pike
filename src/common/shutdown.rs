use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

/// Resolves on the first termination signal from the OS.
///
/// SIGTERM matters as much as SIGINT here: `systemctl stop` and
/// `systemctl restart` send SIGTERM, and Rust's default disposition for it
/// kills the process outright — the graceful shutdown would never run and
/// any host mid-download would get a truncated sensor.
///
/// Never resolves when `enabled` is false: the GUI runs the server in-process
/// and stops it through the notifier, so it must not claim the process signals.
async fn termination_signal(enabled: bool) -> &'static str {
    if !enabled {
        std::future::pending::<()>().await;
    }

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => "Ctrl+C received",
                    _ = term.recv() => "SIGTERM received",
                }
            }
            Err(e) => {
                eprintln!("[server] WARNING: cannot listen for SIGTERM: {e}");
                let _ = tokio::signal::ctrl_c().await;
                "Ctrl+C received"
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "Ctrl+C received"
    }
}

pub async fn shutdown_signal(
    timeout_minutes: u64,
    shutdown_notify: Arc<Notify>,
    handle_signals: bool,
) {
    let notify = shutdown_notify.notified();
    tokio::pin!(notify);
    let signal = termination_signal(handle_signals);
    tokio::pin!(signal);

    let msg = if timeout_minutes == 0 {
        tokio::select! {
            _ = &mut notify => "Download limit reached",
            reason = &mut signal => reason,
        }
    } else {
        let timeout = tokio::time::sleep(Duration::from_secs(timeout_minutes * 60));
        tokio::pin!(timeout);

        tokio::select! {
            _ = &mut timeout => "Timeout reached",
            _ = &mut notify => "Download limit reached",
            reason = &mut signal => reason,
        }
    };

    eprintln!("\n{msg}. Shutting down...");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn timeout_ends_the_wait() {
        shutdown_signal(1, Arc::new(Notify::new()), false).await;
    }

    #[tokio::test]
    async fn notify_ends_the_wait() {
        let notify = Arc::new(Notify::new());
        let n = notify.clone();
        tokio::spawn(async move { n.notify_one() });
        shutdown_signal(0, notify, false).await;
    }

    #[tokio::test(start_paused = true)]
    async fn signals_are_ignored_when_not_owned() {
        // With handle_signals = false only the timeout may end the wait;
        // if the signal branch resolved eagerly this would finish early
        let notify = Arc::new(Notify::new());
        let start = tokio::time::Instant::now();
        shutdown_signal(5, notify, false).await;
        assert_eq!(start.elapsed(), Duration::from_secs(300));
    }
}
