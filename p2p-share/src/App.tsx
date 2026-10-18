import { useState } from "react";
import { APP_NAME } from "./api";
import Diagnostics from "./components/Diagnostics";
import ReceivePanel from "./components/ReceivePanel";
import SendPanel from "./components/SendPanel";
import { ArrowDownIcon, ArrowUpIcon, LogoMark } from "./components/icons";
import "./App.css";

type View = "send" | "receive";

export default function App() {
  const [view, setView] = useState<View>("send");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <LogoMark />
          <span className="brand-name">{APP_NAME}</span>
        </div>
        <nav className="nav" aria-label="Main">
          <button
            className={view === "send" ? "nav-item active" : "nav-item"}
            onClick={() => setView("send")}
          >
            <ArrowUpIcon size={17} />
            Send
          </button>
          <button
            className={view === "receive" ? "nav-item active" : "nav-item"}
            onClick={() => setView("receive")}
          >
            <ArrowDownIcon size={17} />
            Receive
          </button>
        </nav>
        <div className="sidebar-footer">
          <div className="status-line">
            <span className="status-dot" aria-hidden="true" />
            <div>
              <div className="status-title">Ready</div>
              <div className="status-sub">Fake transfers · Phase 2</div>
            </div>
          </div>
          <button
            className="btn-ghost btn-small btn-block"
            onClick={() => setDiagnosticsOpen(true)}
          >
            Diagnostics
          </button>
        </div>
      </aside>
      <main className="content">
        {view === "send" ? <SendPanel /> : <ReceivePanel />}
      </main>
      <Diagnostics
        open={diagnosticsOpen}
        onClose={() => setDiagnosticsOpen(false)}
      />
    </div>
  );
}
