import { useState } from "react";
import { pickSaveDir, receiveFile } from "../api";
import type { TransferEvent } from "../api";
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
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(
    null,
  );
  const [status, setStatus] = useState<Status>("idle");
  const [message, setMessage] = useState("");
  const [savedName, setSavedName] = useState<string | null>(null);

  function handleEvent(event: TransferEvent) {
    if (event.event === "Progress") {
      setProgress({ done: event.data.done, total: event.data.total });
    } else if (event.event === "Finished") {
      setStatus("finished");
    } else {
      setStatus("error");
      setMessage(event.data.message);
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
      const name = await receiveFile(ticket.trim(), saveDir ?? "", handleEvent);
      setSavedName(name);
    } catch (err) {
      setStatus("error");
      setMessage(err instanceof Error ? err.message : String(err));
    }
  }

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
        {ticket.trim() !== "" && (
          <div className="preview-card">
            <div className="preview-name">sample-document.pdf</div>
            <div className="preview-sub">
              Sample preview — the real file name arrives inside the ticket in
              Phase 3.
            </div>
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
          disabled={status === "receiving"}
        >
          Start receiving
        </button>
        {progress && (status === "receiving" || status === "finished") && (
          <div className="receive-progress">
            <Progress done={progress.done} total={progress.total} />
          </div>
        )}
        {status === "finished" && (
          <div className="banner banner-ok" role="status">
            Saved{savedName ? ` as ${savedName}` : ""} (fake).
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
