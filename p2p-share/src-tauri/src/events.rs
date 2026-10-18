use serde::Serialize;

/// Progress and status events streamed from Rust to the UI over a
/// [`tauri::ipc::Channel`]. Channels stay ordered under rapid async emission;
/// never use plain events for progress.
#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum TransferEvent {
    Progress { done: u64, total: u64 },
    Finished,
    Failed { message: String },
    // Phase 4 adds: WaitingForReceiver, Sending { receivers: u32 },
    // fingerprint + diagnostics payloads.
}
