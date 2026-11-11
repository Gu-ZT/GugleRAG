#![cfg(windows)]

use gugle_rag::desktop::DesktopTray;
use tokio::sync::watch;

#[test]
fn tray_can_start_update_and_stop_on_its_dedicated_thread() {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let tray = DesktopTray::start("http://127.0.0.1:18080", shutdown_tx)
        .expect("the Windows tray must initialize on its dedicated thread");
    tray.update_listener_url("http://127.0.0.1:18081");
    assert!(!*shutdown_rx.borrow());
    // Drop waits for the event loop to terminate instead of abandoning its thread.
    drop(tray);
}
