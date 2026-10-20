import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import App, { resolveTheme } from "./App";

// Never restore scroll positions (the dev reloads make them stale) — manual
// mode keeps each launch opening at the very top.
if ("scrollRestoration" in history) {
  history.scrollRestoration = "manual";
}

// Paint the resolved theme BEFORE React mounts. The token blocks ship light
// at :root, so without this the dark-first window flashed light for a frame
// while App's state initializer ran after first paint. resolveTheme is the
// one source of truth (deep link > stored > dark) — same function App's
// useState uses, so the DOM and the first render can never disagree.
document.documentElement.dataset.theme = resolveTheme();

// Surface runtime errors on-screen during dev/screenshot runs instead of a blank
// page. DEV-only: production builds use the ErrorBoundary + quiet toast funnel
// (App.tsx / ErrorBoundary.tsx) — this raw red box must never reach a user.
if (import.meta.env.DEV) {
  window.addEventListener("error", (e) => {
    let box = document.getElementById("errbox");
    if (!box) {
      box = document.createElement("pre");
      box.id = "errbox";
      box.style.cssText =
        "position:fixed;inset:auto 12px 12px 12px;z-index:9999;background:#fff0f0;color:#c00;border:1px solid #f0b0b0;border-radius:8px;padding:10px 12px;font:12px/1.5 ui-monospace,monospace;white-space:pre-wrap;max-height:40vh;overflow:auto";
      document.body.appendChild(box);
    }
    box.textContent = `${e.message}\n${e.filename?.split(/[\\/]/).pop()}:${e.lineno}\n${e.error?.stack ?? ""}`;
  });
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
