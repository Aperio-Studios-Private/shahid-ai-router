import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

/**
 * Global error boundary (Phase 13 hardening): a crash in the interface now
 * renders the error on screen instead of an undiagnosable white window.
 */
class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  componentDidCatch(error: Error) {
    console.error("UI crashed:", error);
  }
  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 24, fontFamily: "ui-monospace, monospace", fontSize: 13 }}>
          <h2 style={{ fontWeight: 600 }}>The interface failed to load</h2>
          <p style={{ color: "#666", marginTop: 8 }}>
            Please report this text — it explains exactly what went wrong.
          </p>
          <pre style={{ whiteSpace: "pre-wrap", marginTop: 12, background: "#f4f4f5", padding: 12, borderRadius: 8 }}>
            {String(this.state.error?.stack ?? this.state.error)}
          </pre>
          <button
            type="button"
            onClick={() => this.setState({ error: null })}
            style={{ marginTop: 12, padding: "6px 12px", borderRadius: 6 }}
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

// Top-level (outside React) failures — module load/parse errors etc. — paint
// the same overlay into #root so a white window can never hide the cause.
// CAPTURE phase is mandatory: module <script> failures fire on the element
// and do NOT bubble to window without it (this was the white-screen blind
// spot).
window.addEventListener("error", reportFatal, true);
window.addEventListener("unhandledrejection", (event) => {
  reportFatal({ message: String(event.reason ?? "unhandled rejection") } as unknown as ErrorEvent);
});
function reportFatal(event: ErrorEvent) {
  const root = document.getElementById("root");
  if (!root || root.childElementCount > 0) return; // React boundary handles in-app errors
  const msg = event.message || String(event.error ?? "unknown script error");
  root.innerHTML =
    '<div style="padding:24px;font-family:ui-monospace,monospace;font-size:13px">' +
    '<h2 style="font-weight:600">The interface failed to load</h2>' +
    '<pre style="white-space:pre-wrap;margin-top:12px;background:#f4f4f5;padding:12px;border-radius:8px">' +
    msg.replace(/[<>&]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;" })[c] ?? c) +
    "</pre></div>";
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
