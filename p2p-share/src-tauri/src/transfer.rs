use crate::events::TransferEvent;
use serde::Serialize;
use tauri::Manager;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::ipc::Channel;

/// Phase 2 fake of the share state. Phase 3 replaces the flag with the real
/// serving session (endpoint/router + blob `Tag`) behind the same handle.
#[derive(Default)]
pub struct ShareState {
    cancel_requested: AtomicBool,
}

impl ShareState {
    fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
    }

    fn reset(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }
}

/// What the UI needs after a share starts. Phase 3 fills both from real
/// iroh state; the shape already matches the plan's flow.
#[derive(Serialize)]
pub struct ShareInfo {
    pub ticket: String,
    pub fingerprint: String,
}

/// Fake values until Phase 3 wires real iroh state.
const FAKE_TICKET: &str = "FAKE-TICKET-p2p-share-phase2";
const FAKE_FINGERPRINT: &str = "FAKE-FINGERPRINT-0123";

/// Fake progress shape: 20 steps x 400 ms, about 8 seconds per transfer.
const FAKE_STEPS: u64 = 20;
const FAKE_STEP_DELAY: Duration = Duration::from_millis(400);

#[tauri::command]
pub async fn send_file(
    app: tauri::AppHandle,
    path: String,
    on_event: Channel<TransferEvent>,
) -> Result<ShareInfo, String> {
    if path.trim().is_empty() {
        return Err("No file selected.".to_string());
    }
    // Phase 3: hash the file, start serving it, return ticket + fingerprint.
    // For now: return the fake share immediately and stream fake progress
    // from a background task, like the real serving loop will.
    app.state::<ShareState>().reset();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<ShareState>();
        let total = 100u64;
        for step in 1..=FAKE_STEPS {
            tokio::time::sleep(FAKE_STEP_DELAY).await;
            if state.is_cancelled() {
                let _ = on_event.send(TransferEvent::Failed {
                    message: "Sharing stopped.".to_string(),
                });
                return;
            }
            let _ = on_event.send(TransferEvent::Progress {
                done: step * total / FAKE_STEPS,
                total,
            });
        }
        let _ = on_event.send(TransferEvent::Finished);
    });
    Ok(ShareInfo {
        ticket: FAKE_TICKET.to_string(),
        fingerprint: FAKE_FINGERPRINT.to_string(),
    })
}

#[tauri::command]
pub async fn receive_file(
    ticket: String,
    save_dir: String,
    on_event: Channel<TransferEvent>,
) -> Result<String, String> {
    if ticket.trim().is_empty() {
        return Err("Paste a ticket first.".to_string());
    }
    if save_dir.trim().is_empty() {
        return Err("Pick a save folder first.".to_string());
    }
    // Phase 3: parse the ticket, connect, download, export to save_dir.
    // For now: fixed fake progress on the command task, then a fake name.
    let total = 100u64;
    for step in 1..=FAKE_STEPS {
        tokio::time::sleep(FAKE_STEP_DELAY).await;
        let _ = on_event.send(TransferEvent::Progress {
            done: step * total / FAKE_STEPS,
            total,
        });
    }
    let _ = on_event.send(TransferEvent::Finished);
    Ok("fake-download.bin".to_string())
}

#[tauri::command]
pub async fn stop_sharing(app: tauri::AppHandle) -> Result<String, String> {
    // Phase 3: close the router (invalidates the ticket) and stop serving.
    app.state::<ShareState>().request_cancel();
    Ok("Sharing stopped.".to_string())
}
