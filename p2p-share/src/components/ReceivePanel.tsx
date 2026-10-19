import { useEffect, useState } from "react";
import {
  fileNameOf,
  formatBytes,
  normalizeTicket,
  pickSaveDir,
  previewTicket,
  receiveFile,
} from "../api";
import type { FilePreview, TransferEvent } from "../api";
import Progress from "./Progress";
import {
  AlertIcon,
  FolderIcon,
  LinkIcon,
  LockIcon,
  ShieldIcon,
  ZapIcon,
} from "./icons";

type Status = "idle" | "receiving" | "finished" | "error";

export default function ReceivePanel() {
  const [ticket, setTicket] = useState("");
  const [saveDir, setSaveDir] = useState<string | null>(null);
  const [preview, setPreview] = useState<FilePreview | null>(null);
  const [previewError, setPreviewError] = useState("");
  const [lookingUp, setLookingUp] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(
    null,
  );
  const [status, setStatus] = useState<Status>("idle");
  const [message, setMessage] = useState("");
  const [savedName, setSavedName] = useState<string | null>(null);

  // Look up the real file name + size shortly after the ticket stops
  // changing. An empty ticket clears the preview without a backend call.
  useEffect(() => {
    const value = normalizeTicket(ticket);
    setPreview(null);
    setPreviewError("");
    if (value === "") {
      setLookingUp(false);
      return;
    }
    setLookingUp(true);
    let cancelled = false;
    const timer = window.setTimeout(() => {
      previewTicket(value)
        .then((found) => {
          if (cancelled) return;
          setPreview(found);
          setPreviewError("");
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          setPreview(null);
          setPreviewError(err instanceof Error ? err.message : String(err));
        })
        .finally(() => {
          if (!cancelled) setLookingUp(false);
        });
    }, 600);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [ticket]);

  function handleEvent(event: TransferEvent) {
    if (event.event === "Progress") {
      setProgress({ done: event.data.done, total: event.data.total });
    } else if (event.event === "Finished") {
      setStatus("finished");
    }
  }

  async function handleChooseDir() {
    try {
      const picked = await pickSaveDir();
      if (picked) setSaveDir(picked);
    } catch (err) {
      setStatus("error");
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleStart() {
    setProgress(null);
    setSavedName(null);
    setMessage("");
    setStatus("receiving");
    try {
      const name = await receiveFile(
        normalizeTicket(ticket),
        saveDir ?? "",
        handleEvent,
      );
      setSavedName(name);
    } catch (err) {
      setStatus("error");
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

  const canStart =
    status !== "receiving" && normalizeTicket(ticket) !== "" && saveDir !== null;

  return (
    <div className="view">
      <h1 className="view-title">Receive a file</h1>
      <p className="view-subtitle">
        Enter a transfer ticket from the sender to start downloading.
      </p>

      <div className="card">
        <div className="ticket-head">
          <span className="ticket-icon">
            <LinkIcon size={20} />
          </span>
          <div>
            <h2 className="card-title">Transfer ticket</h2>
            <p className="card-sub">Paste the ticket code from the sender.</p>
          </div>
        </div>
        <input
          className="input ticket-input"
          value={ticket}
          onChange={(e) => setTicket(e.currentTarget.value)}
          placeholder="Paste ticket here…"
          spellCheck={false}
          autoComplete="off"
        />
        {normalizeTicket(ticket) !== "" && (
          <div className="preview-card">
            {preview ? (
              <>
                <div className="preview-name">{fileNameOf(preview.name)}</div>
                <div className="preview-sub">{formatBytes(preview.size)} — verified from the ticket.</div>
              </>
            ) : lookingUp ? (
              <div className="preview-sub">Looking up the file…</div>
            ) : previewError ? (
              <div className="preview-error">{previewError}</div>
            ) : null}
          </div>
        )}
        <div className="dir-row">
          <button className="btn-ghost" onClick={handleChooseDir}>
            <FolderIcon size={16} />
            {saveDir ? "Change save folder" : "Choose save folder"}
          </button>
          <span className="dir-path" title={saveDir ?? ""}>
            {saveDir ?? "No folder chosen"}
          </span>
        </div>
        <button
          className="btn-primary btn-block btn-large"
          onClick={handleStart}
          disabled={!canStart}
        >
          Start receiving
        </button>
        {status === "receiving" && !progress && (
          <div className="connecting-hint">Connecting to the sender…</div>
        )}
        {progress && (status === "receiving" || status === "finished") && (
          <div className="receive-progress">
            <Progress done={progress.done} total={progress.total} />
          </div>
        )}
        {status === "finished" && (
          <div className="banner banner-ok" role="status">
            Saved{savedName ? ` as ${savedName}` : ""}.
          </div>
        )}
        {status === "error" && message && (
          <div className="banner banner-error" role="alert">
            <AlertIcon size={16} />
            {message}
          </div>
        )}
      </div>

      <div className="card trust-row">
        <div className="trust-item">
          <LockIcon size={18} />
          <div>
            <div className="trust-title">End-to-end encrypted</div>
            <div className="trust-sub">Transferred directly between devices.</div>
          </div>
        </div>
        <div className="trust-item">
          <ZapIcon size={18} />
          <div>
            <div className="trust-title">No account needed</div>
            <div className="trust-sub">Just a ticket code.</div>
          </div>
        </div>
        <div className="trust-item">
          <ShieldIcon size={18} />
          <div>
            <div className="trust-title">Verified integrity</div>
            <div className="trust-sub">Checked as it downloads.</div>
          </div>
        </div>
      </div>
    </div>
  );
}
