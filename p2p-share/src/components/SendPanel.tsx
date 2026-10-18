import { useEffect, useState } from "react";
import {
  copyText,
  fileNameOf,
  onFileDrop,
  pickFile,
  sendFile,
  stopSharing,
} from "../api";
import type { ShareInfo, TransferEvent } from "../api";
import Fingerprint from "./Fingerprint";
import Progress from "./Progress";
import { AlertIcon, CheckIcon, CopyIcon, FileIcon, LockIcon } from "./icons";

type Status = "idle" | "sharing" | "stopped" | "finished" | "error";

function truncateMiddle(value: string, head = 14, tail = 10): string {
  if (value.length <= head + tail + 1) return value;
  return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

export default function SendPanel() {
  const [filePath, setFilePath] = useState<string | null>(null);
  const [share, setShare] = useState<ShareInfo | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(
    null,
  );
  const [status, setStatus] = useState<Status>("idle");
  const [message, setMessage] = useState("");
  const [copied, setCopied] = useState(false);
  const [dragging, setDragging] = useState(false);

  function handleEvent(event: TransferEvent) {
    if (event.event === "Progress") {
      setProgress({ done: event.data.done, total: event.data.total });
    } else if (event.event === "Finished") {
      setStatus("finished");
      setMessage("Transfer finished (fake).");
    } else if (event.data.message === "Sharing stopped.") {
      setStatus("stopped");
      setMessage("Sharing stopped.");
    } else {
      setStatus("error");
      setMessage(event.data.message);
    }
  }

  async function startShare(path: string) {
    setProgress(null);
    setMessage("");
    setStatus("sharing");
    try {
      const info = await sendFile(path, handleEvent);
      setShare(info);
    } catch (err) {
      setStatus("error");
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

  async function sharePath(path: string) {
    setShare(null);
    setFilePath(path);
    await startShare(path);
  }

  async function handleChoose() {
    try {
      const picked = await pickFile();
      if (!picked) return;
      await sharePath(picked);
    } catch (err) {
      setStatus("error");
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onFileDrop(
      (paths) => {
        if (paths[0]) void sharePath(paths[0]);
      },
      setDragging,
    )
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
    // sharePath only uses setState + backend calls, so one registration is enough.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleStop() {
    try {
      await stopSharing();
    } catch (err) {
      setStatus("error");
      setMessage(err instanceof Error ? err.message : String(err));
      return;
    }
    setStatus("stopped");
    setMessage("Sharing stopped.");
  }

  async function handleCopy() {
    if (!share) return;
    try {
      await copyText(share.ticket);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="view">
      <h1 className="view-title">Send a file</h1>
      <p className="view-subtitle">
        Share directly with another device. No cloud storage.
      </p>

      <div className="card file-card">
        <span className="file-icon">
          <FileIcon size={26} />
        </span>
        <div className="file-meta">
          <div className="file-name">{filePath ? fileNameOf(filePath) : "No file chosen"}</div>
          <div className="file-sub">
            {filePath ? (
              <span className="pill pill-green">
                <CheckIcon size={14} /> Selected
              </span>
            ) : (
              "Pick a file — or drop one anywhere on this window."
            )}
          </div>
        </div>
        <button className="btn-primary" onClick={handleChoose}>
          {filePath ? "Choose another file" : "Choose a file"}
        </button>
      </div>

      {share && (
        <div className="share-grid">
          <div className="card">
            <h2 className="card-title">Share this ticket</h2>
            <p className="card-sub">Only share it with the person receiving this file.</p>
            <div className="ticket-row">
              <code className="ticket-code" title={share.ticket}>
                {truncateMiddle(share.ticket)}
              </code>
              <button className="btn-ghost" onClick={handleCopy}>
                {copied ? <CheckIcon size={16} /> : <CopyIcon size={16} />}
                {copied ? "Copied" : "Copy"}
              </button>
            </div>
            <Fingerprint value={share.fingerprint} />
            <div className="banner banner-warn">
              <AlertIcon size={16} />
              Anyone with this ticket can download this file.
            </div>
            <button
              className="btn-danger-outline btn-block"
              onClick={handleStop}
              disabled={status === "stopped" || status === "finished"}
            >
              Stop sharing
            </button>
          </div>

          <div className="card status-card">
            <h2 className="status-heading">
              {status === "sharing" && "Waiting for receiver"}
              {status === "finished" && "Transfer finished"}
              {status === "stopped" && "Sharing stopped"}
              {status === "error" && "Something went wrong"}
              {status === "idle" && "Ready"}
            </h2>
            <p className="card-sub">
              {status === "sharing" && "Send the ticket to begin the transfer."}
              {(status === "finished" || status === "stopped") && message}
            </p>
            {progress && (status === "sharing" || status === "finished") && (
              <Progress done={progress.done} total={progress.total} />
            )}
            <div className="note-row">
              <LockIcon size={16} />
              <div>
                <div className="note-title">Encrypted connection</div>
                <div className="note-sub">
                  Direct connection when possible, relay used if needed.
                </div>
              </div>
            </div>
            <div className="banner banner-info">
              This app must stay open while the file is being downloaded.
            </div>
          </div>
        </div>
      )}

      {status === "error" && message && (
        <div className="banner banner-error" role="alert">
          <AlertIcon size={16} />
          {message}
        </div>
      )}

      {dragging && (
        <div className="drop-overlay" aria-hidden="true">
          <div className="drop-overlay-inner">
            <FileIcon size={30} />
            <div className="drop-overlay-title">Drop to send this file</div>
            <div className="drop-overlay-sub">One file at a time in v1.</div>
          </div>
        </div>
      )}
    </div>
  );
}
