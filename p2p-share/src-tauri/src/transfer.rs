//! Thin Tauri adapters over [`crate::share`] and [`crate::download`].
//!
//! These translate between IPC types (`Channel`, command args/results) and
//! the core logic, which speaks only mpsc + anyhow and stays testable
//! without a Tauri context.

use crate::{
    download,
    events::TransferEvent,
    share::{self, ShareSession, TicketType},
};
use serde::Serialize;
use std::{path::Path, sync::Mutex};
use tauri::{AppHandle, Manager, State, ipc::Channel};
use tokio::sync::mpsc;

/// What the UI needs once a share starts.
#[derive(Serialize)]
pub struct ShareInfo {
    pub ticket: String,
    pub fingerprint: String,
}

enum ShareSlot {
    Empty,
    Starting { cancelled: bool },
    Active(ShareSession),
}

impl Default for ShareSlot {
    fn default() -> Self {
        Self::Empty
    }
}

#[derive(Default)]
pub struct AppState {
    /// Sync mutex, deliberately never held across `.await`
    /// (short critical sections only).
    share: Mutex<ShareSlot>,
    /// Async mutex: held across the whole download on purpose, so a
    /// second Start click fails fast instead of corrupting the cache.
    receive_lock: tokio::sync::Mutex<()>,
    /// Abort handle for the running download task, if any. Sync mutex,
    /// short critical sections only — aborting is synchronous.
    receive_cancel: Mutex<Option<tokio::task::AbortHandle>>,
}

fn spawn_forwarder(
    mut events: mpsc::UnboundedReceiver<TransferEvent>,
    channel: Channel<TransferEvent>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            if channel.send(event).is_err() {
                break;
            }
        }
    });
}

fn lock_share<'a>(state: &'a State<'a, AppState>) -> Result<std::sync::MutexGuard<'a, ShareSlot>, String> {
    state.share.lock().map_err(|_| "internal state error".to_string())
}

#[tauri::command]
pub async fn send_file(
    app: AppHandle,
    path: String,
    ticket_type: String,
    on_event: Channel<TransferEvent>,
) -> Result<ShareInfo, String> {
    let ticket_type = TicketType::parse(&ticket_type).map_err(|error| error.to_string())?;
    let state = app.state::<AppState>();
    {
        let mut slot = lock_share(&state)?;
        match *slot {
            ShareSlot::Empty => *slot = ShareSlot::Starting { cancelled: false },
            ShareSlot::Starting { .. } => {
                return Err("A share is already starting. Stop it first.".to_string());
            }
            ShareSlot::Active(_) => {
                return Err("Already sharing a file. Stop sharing first.".to_string());
            }
        }
    }
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    spawn_forwarder(events_rx, on_event);
    let started = share::start_share(Path::new(&path), ticket_type, events_tx).await;
    // Decide under the lock; act after it. The sync guard must never be
    // held across an `.await` (it is not `Send`).
    enum PostStart {
        Keep(ShareInfo),
        Cancelled(ShareSession),
        Failed(String),
    }
    let action = {
        let mut slot = lock_share(&state)?;
        match started {
            Ok(started) => {
                if matches!(*slot, ShareSlot::Starting { cancelled: true }) {
                    *slot = ShareSlot::Empty;
                    PostStart::Cancelled(started.session)
                } else {
                    let info = ShareInfo {
                        ticket: started.ticket,
                        fingerprint: started.fingerprint,
                    };
                    *slot = ShareSlot::Active(started.session);
                    PostStart::Keep(info)
                }
            }
            Err(error) => {
                *slot = ShareSlot::Empty;
                PostStart::Failed(format!("{error:#}"))
            }
        }
    };
    match action {
        PostStart::Keep(info) => Ok(info),
        PostStart::Cancelled(session) => {
            session.shutdown().await;
            Err("Sharing stopped.".to_string())
        }
        PostStart::Failed(message) => Err(message),
    }
}

#[tauri::command]
pub async fn receive_file(
    app: AppHandle,
    ticket: String,
    save_dir: String,
    on_event: Channel<TransferEvent>,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    let _guard = state
        .receive_lock
        .try_lock()
        .map_err(|_| "A download is already running.".to_string())?;
    let store_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot locate app data: {error}"))?
        .join("blobs");
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    spawn_forwarder(events_rx, on_event);
    // The download runs as a task so `cancel_receive` can abort it. The
    // lock stays in the command (not the task), so an abort can never
    // leak it. Partial cache survives an abort and resumes next run.
    let task = tokio::spawn(async move {
        download::receive(&ticket, Path::new(&save_dir), &store_dir, events_tx).await
    });
    *state
        .receive_cancel
        .lock()
        .map_err(|_| "internal state error".to_string())? = Some(task.abort_handle());
    let result = task.await;
    *state
        .receive_cancel
        .lock()
        .map_err(|_| "internal state error".to_string())? = None;
    match result {
        Ok(Ok(name)) => Ok(name),
        Ok(Err(error)) => Err(format!("{error:#}")),
        Err(join) if join.is_cancelled() => Err("Download cancelled.".to_string()),
        Err(join) => Err(format!("download task failed: {join}")),
    }
}

#[tauri::command]
pub async fn cancel_receive(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let handle = state
        .receive_cancel
        .lock()
        .map_err(|_| "internal state error".to_string())?
        .take();
    match handle {
        Some(handle) => {
            handle.abort();
            Ok("Download cancelled.".to_string())
        }
        None => Ok("No download running.".to_string()),
    }
}

#[tauri::command]
pub async fn preview_ticket(app: AppHandle, ticket: String) -> Result<download::FilePreview, String> {
    let store_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot locate app data: {error}"))?
        .join("blobs");
    download::preview(&ticket, &store_dir)
        .await
        .map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn stop_sharing(app: AppHandle) -> Result<String, String> {
    // Take the session out under the lock; shut it down after. The sync
    // guard must never be held across an `.await` (it is not `Send`).
    let state = app.state::<AppState>();
    let action = {
        let mut slot = lock_share(&state)?;
        match std::mem::replace(&mut *slot, ShareSlot::Empty) {
            ShareSlot::Active(session) => Some(session),
            ShareSlot::Starting { .. } => {
                *slot = ShareSlot::Starting { cancelled: true };
                return Ok("Stopping…".to_string());
            }
            ShareSlot::Empty => return Ok("Not sharing.".to_string()),
        }
    };
    if let Some(session) = action {
        session.shutdown().await;
        Ok("Sharing stopped.".to_string())
    } else {
        Ok("Not sharing.".to_string())
    }
}
