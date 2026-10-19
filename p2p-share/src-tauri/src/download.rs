//! Incoming transfers: parse a ticket, download, verify, export.
//!
//! The filename always comes from the hashed collection metadata, never
//! from side-channel text. Progress streams to the UI over an mpsc channel
//! (the Tauri adapter forwards it into the IPC `Channel`).

use crate::{
    events::TransferEvent,
    export::{export_file, validate_single_segment},
    share::{METADATA_NAME, ShareMetadata, build_endpoint},
};
use anyhow::{Context, Result, anyhow, bail};
use iroh::{Endpoint, endpoint::Connection};
use iroh_blobs::{
    BlobFormat, BlobsProtocol, Hash,
    api::{Store, remote::GetProgressItem},
    format::collection::Collection,
    hashseq::HashSeq,
    store::fs::FsStore,
    ticket::BlobTicket,
};
use n0_future::StreamExt;
use serde::Serialize;
use std::{path::Path, str::FromStr, time::Duration};
use tokio::{sync::mpsc, time::timeout};

/// Timeouts: explicit everywhere so a stalled transfer always ends.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const METADATA_TIMEOUT: Duration = Duration::from_secs(60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Same-process retry reuses already-downloaded ranges automatically.
const RETRY_LIMIT: u32 = 3;
const RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// What a ticket points at, with the connection ready to pull from.
struct TicketTarget {
    endpoint: Endpoint,
    store: Store,
    _protocol: BlobsProtocol,
    connection: Connection,
    file_name: String,
    file_hash: Hash,
    total: u64,
}

/// What the receiver UI shows before downloading anything.
#[derive(Serialize)]
pub struct FilePreview {
    pub name: String,
    pub size: u64,
}

/// Parse the ticket, connect, and read the hashed file details.
/// Shared by preview (name + size only) and receive (full download).
async fn connect_and_describe(ticket_text: &str, store_dir: &Path) -> Result<TicketTarget> {
    let ticket = BlobTicket::from_str(ticket_text.trim())
        .map_err(|_| anyhow!("That doesn't look like a fly ticket. Check for missing characters."))?;
    std::fs::create_dir_all(store_dir).context("cannot open transfer cache")?;
    let endpoint = build_endpoint().await?;
    let store = Store::from(
        FsStore::load(store_dir)
            .await
            .context("cannot open transfer cache")?,
    );
    // Held for the whole transfer: backs the `Store` handle below.
    let _protocol = BlobsProtocol::new(&store, None);
    // Full-ticket relay/direct addresses are used as-is; id-only tickets
    // resolve through discovery (DNS/Pkarr).
    let connection = timeout(
        CONNECT_TIMEOUT,
        endpoint.connect(ticket.addr().clone(), iroh_blobs::protocol::ALPN),
    )
    .await
    .map_err(|_| {
        anyhow!("Couldn't reach the sender (connection timed out). Are they still sharing?")
    })?
    .context("Couldn't reach the sender")?;
    let remote = store.remote();
    let collection = timeout(
        METADATA_TIMEOUT,
        fetch_collection(&remote, &connection, &store, ticket.hash()),
    )
    .await
    .map_err(|_| anyhow!("Timed out waiting for transfer details."))??;
    let (file_name, file_hash, total) =
        describe_collection(&collection, &store, &remote, &connection).await?;
    Ok(TicketTarget {
        endpoint,
        store,
        _protocol,
        connection,
        file_name,
        file_hash,
        total,
    })
}

/// Look up a ticket's file name + size without downloading the file.
pub async fn preview(ticket_text: &str, store_dir: &Path) -> Result<FilePreview> {
    let target = connect_and_describe(ticket_text, store_dir).await?;
    let preview = FilePreview {
        name: target.file_name.clone(),
        size: target.total,
    };
    target.endpoint.close().await;
    Ok(preview)
}

/// Download `ticket_text` into `save_dir`, using `store_dir` as the
/// persistent blob cache. Returns the received file name.
pub async fn receive(
    ticket_text: &str,
    save_dir: &Path,
    store_dir: &Path,
    events: mpsc::UnboundedSender<TransferEvent>,
) -> Result<String> {
    let target = connect_and_describe(ticket_text, store_dir).await?;
    let remote = target.store.remote();
    download_with_retry(
        &remote,
        &target.connection,
        target.file_hash,
        target.total,
        &events,
    )
    .await?;
    export_file(&target.store, target.file_hash, &target.file_name, save_dir).await?;
    let _ = events.send(TransferEvent::Finished);
    target.endpoint.close().await;
    Ok(target.file_name)
}

/// Fetch one blob fully, ignoring progress (for small metadata).
async fn fetch_to_end(
    remote: &iroh_blobs::api::remote::Remote,
    connection: &Connection,
    hash: Hash,
    format: BlobFormat,
) -> Result<()> {
    let progress = remote.fetch(connection.clone(), (hash, format));
    let mut stream = progress.stream();
    while let Some(item) = stream.next().await {
        match item {
            GetProgressItem::Done(_) => return Ok(()),
            GetProgressItem::Error(cause) => bail!("download failed: {cause}"),
            GetProgressItem::Progress(_) => {}
        }
    }
    Err(anyhow!("download ended unexpectedly"))
}

/// Fetch ONLY a collection's framing blobs (root sequence + name table),
/// then parse it. Everything must be requested as `Raw`: `HashSeq` would
/// recursively pull every child — including the whole file.
async fn fetch_collection(
    remote: &iroh_blobs::api::remote::Remote,
    connection: &Connection,
    db: &Store,
    collection_hash: Hash,
) -> Result<Collection> {
    fetch_to_end(remote, connection, collection_hash, BlobFormat::Raw).await?;
    let root_bytes = db
        .get_bytes(collection_hash)
        .await
        .context("downloaded data is incomplete")?;
    let sequence = HashSeq::try_from(root_bytes).context("transfer details are corrupt")?;
    let meta_hash = sequence
        .iter()
        .next()
        .context("transfer details are empty")?;
    fetch_to_end(remote, connection, meta_hash, BlobFormat::Raw).await?;
    Collection::load(collection_hash, db)
        .await
        .context("downloaded data is incomplete")
}

/// Split the collection into our metadata entry plus the single file entry.
/// Fetches only the tiny metadata blob; the file blob stays remote.
async fn describe_collection(
    collection: &Collection,
    db: &Store,
    remote: &iroh_blobs::api::remote::Remote,
    connection: &Connection,
) -> Result<(String, Hash, u64)> {
    let mut metadata_hash = None;
    let mut files = Vec::new();
    for (name, hash) in collection.iter() {
        if name == METADATA_NAME {
            metadata_hash = Some(*hash);
        } else {
            files.push((name.clone(), *hash));
        }
    }
    let metadata_hash = metadata_hash.context("ticket is missing file details")?;
    if files.len() != 1 {
        bail!("unsupported share: expected exactly one file");
    }
    fetch_to_end(remote, connection, metadata_hash, BlobFormat::Raw).await?;
    let bytes = db
        .get_bytes(metadata_hash)
        .await
        .context("file details are incomplete")?;
    let metadata: ShareMetadata =
        serde_json::from_slice(&bytes).context("file details are corrupt")?;
    validate_single_segment(&metadata.name)?;
    let (entry_name, entry_hash) = files.into_iter().next().expect("checked above");
    if entry_name != metadata.name {
        bail!("share contents are inconsistent");
    }
    Ok((metadata.name, entry_hash, metadata.size))
}

/// Fetch the file blob with progress, retrying transient failures.
/// Already-downloaded ranges are reused on retry automatically.
async fn download_with_retry(
    remote: &iroh_blobs::api::remote::Remote,
    connection: &Connection,
    hash: Hash,
    total: u64,
    events: &mpsc::UnboundedSender<TransferEvent>,
) -> Result<()> {
    let mut attempt = 0u32;
    let mut done = 0u64;
    loop {
        attempt += 1;
        let progress = remote.fetch(connection.clone(), (hash, BlobFormat::Raw));
        let mut stream = progress.stream();
        let mut failed: Option<anyhow::Error> = None;
        let mut finished = false;
        loop {
            match timeout(IDLE_TIMEOUT, stream.next()).await {
                Err(_) => {
                    failed = Some(anyhow!("stalled: no data for 120s"));
                    break;
                }
                Ok(None) => {
                    failed = Some(anyhow!("download ended unexpectedly"));
                    break;
                }
                Ok(Some(GetProgressItem::Progress(offset))) => {
                    // Never let the bar move backwards on retooled offsets.
                    done = done.max(offset).min(total);
                    let _ = events.send(TransferEvent::Progress { done, total });
                }
                Ok(Some(GetProgressItem::Done(_))) => {
                    finished = true;
                    break;
                }
                Ok(Some(GetProgressItem::Error(cause))) => {
                    failed = Some(anyhow!("download failed: {cause}"));
                    break;
                }
            }
        }
        if finished {
            break;
        }
        if attempt >= RETRY_LIMIT {
            return Err(failed.unwrap_or_else(|| anyhow!("download failed")));
        }
        tokio::time::sleep(RETRY_BACKOFF).await;
    }
    let _ = events.send(TransferEvent::Progress { done: total, total });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(case: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("fly-test-{}-{}-{case}", std::process::id(), nanos))
    }

    /// Full pipeline in one process: share a file, receive it elsewhere,
    /// compare bytes. Exercises import, ticket, connect, download, export.
    #[tokio::test]
    async fn share_receive_roundtrip_same_process() {
        let base = unique_test_dir("roundtrip");
        let src_dir = base.join("src");
        let out_dir = base.join("out");
        let cache_dir = base.join("cache");
        std::fs::create_dir_all(&src_dir).unwrap();
        let content: Vec<u8> = (0..65536u32).map(|i| (i.wrapping_mul(7)) as u8).collect();
        std::fs::write(src_dir.join("input.bin"), &content).unwrap();

        let (send_tx, _send_rx) = mpsc::unbounded_channel();
        let started = share::start_share(
            &src_dir.join("input.bin"),
            share::TicketType::Full,
            send_tx,
        )
        .await
        .expect("share");
        let (recv_tx, _recv_rx) = mpsc::unbounded_channel();
        let name = receive(&started.ticket, &out_dir, &cache_dir, recv_tx)
            .await
            .expect("receive");
        assert_eq!(name, "input.bin");
        assert_eq!(
            std::fs::read(out_dir.join("input.bin")).unwrap(),
            content
        );
        started.session.shutdown().await;
        std::fs::remove_dir_all(&base).ok();
    }

    fn dir_bytes(dir: &Path) -> u64 {
        let mut total = 0u64;
        let mut entries = vec![dir.to_path_buf()];
        while let Some(dir) = entries.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    entries.push(entry.path());
                } else {
                    total += meta.len();
                }
            }
        }
        total
    }

    /// Previewing a 128 MiB file must not pull its bytes: only the
    /// collection root + metadata blob (a few KB) may land in the cache.
    /// (Fetching the ticket hash as HashSeq would download everything.)
    #[tokio::test]
    async fn preview_does_not_download_file() {
        let base = unique_test_dir("preview-big");
        let src_dir = base.join("src");
        let cache_dir = base.join("cache");
        std::fs::create_dir_all(&src_dir).unwrap();
        let big: Vec<u8> = (0..(128u32 * 1024 * 1024)).map(|i| (i.wrapping_mul(7)) as u8).collect();
        std::fs::write(src_dir.join("big.bin"), &big).unwrap();
        drop(big);

        let (send_tx, _send_rx) = mpsc::unbounded_channel();
        let started = share::start_share(&src_dir.join("big.bin"), share::TicketType::Full, send_tx)
            .await
            .expect("share");
        let found = preview(&started.ticket, &cache_dir).await.expect("preview");
        assert_eq!(found.name, "big.bin");
        assert_eq!(found.size, 128 * 1024 * 1024);
        let cached = dir_bytes(&cache_dir);
        // FsStore preallocates a ~1 MiB data file, so "a few KB" reads as
        // ~1 MiB on disk. Anything near 128 MiB means file bytes leaked in.
        assert!(
            cached < 2 * 1024 * 1024,
            "preview pulled file bytes into the cache ({cached} bytes)"
        );
        started.session.shutdown().await;
        std::fs::remove_dir_all(&base).ok();
    }

    /// Preview reads the hashed name + size without downloading the file.
    #[tokio::test]
    async fn preview_reports_name_and_size() {
        let base = unique_test_dir("preview");
        let src_dir = base.join("src");
        let cache_dir = base.join("cache");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("hello.txt"), b"hello preview").unwrap();

        let (send_tx, _send_rx) = mpsc::unbounded_channel();
        let started = share::start_share(&src_dir.join("hello.txt"), share::TicketType::Id, send_tx)
            .await
            .expect("share");
        let found = preview(&started.ticket, &cache_dir).await.expect("preview");
        assert_eq!(found.name, "hello.txt");
        assert_eq!(found.size, 13);
        started.session.shutdown().await;
        std::fs::remove_dir_all(&base).ok();
    }
}
