import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// Linux has no OS backdrop effect, so the CSS frost layer carries the whole
// job there (see the `platform-linux` override in App.css). The webview is
// OS-pinned, so this check is deterministic.
if (navigator.platform.startsWith("Linux")) {
  document.documentElement.classList.add("platform-linux");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
