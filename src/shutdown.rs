use std::sync::Arc;
use tokio::sync::Notify;

pub async fn shutdown_signal(
    timeout_minutes: u64,
    shutdown_notify: Arc<Notify>,
    handle_ctrlc: bool,
) {
    let notify = shutdown_notify.notified();
    tokio::pin!(notify);

    let msg = if timeout_minutes == 0 {
        // No timeout — wait for shutdown notify or Ctrl+C only
        if handle_ctrlc {
            let ctrl_c = tokio::signal::ctrl_c();
            tokio::pin!(ctrl_c);
            tokio::select! {
                _ = &mut notify => "Download limit reached",
                _ = &mut ctrl_c => "Ctrl+C received",
            }
        } else {
            notify.await;
            "Download limit reached"
        }
    } else {
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(timeout_minutes * 60));
        tokio::pin!(timeout);

        if handle_ctrlc {
            let ctrl_c = tokio::signal::ctrl_c();
            tokio::pin!(ctrl_c);
            tokio::select! {
                _ = &mut timeout => "Timeout reached",
                _ = &mut notify => "Download limit reached",
                _ = &mut ctrl_c => "Ctrl+C received",
            }
        } else {
            tokio::select! {
                _ = &mut timeout => "Timeout reached",
                _ = &mut notify => "Download limit reached",
            }
        }
    };

    eprintln!("\n{msg}. Shutting down...");
}
