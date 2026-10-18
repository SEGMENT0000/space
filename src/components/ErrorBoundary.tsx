// Render-phase error net (FLEET_SPEC §D): a thrown error anywhere under the
// boundary degrades to a quiet instrument panel instead of a white screen.
// It must be a class — React only supports error boundaries as classes
// (hooks cannot catch render throws).
//
// WHY a module-level notifyError/subscribeError registry rather than App
// passing a setState closure down: every error source needs ONE funnel.
// Sources are three — this boundary's onError, window "error", and
// unhandledrejection — and only App owns toast state. A tiny registry lets
// all three converge on the single toast in App (single source of truth)
// without prop-drilling a callback into a component whose whole job is to
// stay policy-free. If no subscriber exists yet (a throw before App mounts),
// the fallback panel below is itself the message — the toast is redundancy,
// not the primary channel — so dropping is safe.
//
// errors.css is imported HERE, not from index.css: index.css is the lead's
// import manifest (untouched by this agent), and the spec's pattern is
// component-owned stylesheets (previewPlate.ts → preview.css, §G).

import { Component, type ReactNode } from "react";
import "../styles/errors.css";

type ErrorListener = (msg: string) => void;

// WHY a Set: App subscribes exactly once, but the registry must tolerate
// zero subscribers (pre-mount throws) and future subscribers without
// leaking — unsubscribe is an explicit closure returned to the caller.
const listeners = new Set<ErrorListener>();

/** Subscribe to the app-wide error funnel. Returns an unsubscribe fn. */
export function subscribeError(fn: ErrorListener): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/**
 * Signal a runtime error to every subscriber (App renders it as the quiet
 * toast). Safe to call from anywhere: a listener that throws inside the
 * error path must never become a second error, so each call is isolated.
 */
export function notifyError(msg: string): void {
  for (const fn of [...listeners]) {
    try {
      fn(msg);
    } catch {
      // fail-closed: swallow so the reporter can't cascade
    }
  }
}

interface ErrorBoundaryProps {
  children?: ReactNode;
  /**
   * App funnels this into notifyError, so a render-phase failure and a
   * window-level failure surface through the same toast — one path, one
   * voice, no double reporting.
   */
  onError?: (err: Error) => void;
}

interface ErrorBoundaryState {
  err: Error | null;
}

// The fallback renders REAL error data, but never unbounded: a message is
// one quiet line (control characters stripped, whitespace collapsed, capped
// at 280 chars), a stack is capped at 4000 chars. CSS still wraps/clips —
// the caps are defense against pathological payloads, not the layout plan.
const MSG_MAX = 280;
const STACK_MAX = 4000;

function sanitizeMessage(raw: string): string {
  const s = raw
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return s.length > MSG_MAX ? `${s.slice(0, MSG_MAX - 1)}…` : s;
}

function sanitizeStack(raw: string): string {
  const s = raw.replace(/\u0000/g, "").trim();
  return s.length > STACK_MAX ? `${s.slice(0, STACK_MAX)}\n… truncated` : s;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { err: null };

  static getDerivedStateFromError(err: Error): ErrorBoundaryState {
    return { err };
  }

  componentDidCatch(err: Error): void {
    // WHY onError and not notifyError directly: the boundary stays
    // policy-free (it only reports); App decides how the failure is
    // surfaced. try/catch because an onError that throws would otherwise
    // become a second error inside the error path.
    try {
      this.props.onError?.(err);
    } catch {
      // fail-closed
    }
  }

  render(): ReactNode {
    const { err } = this.state;
    // Healthy path renders a fragment — zero DOM, so the .app shell's
    // layout is byte-identical to a world without this boundary.
    if (!err) return this.props.children;

    // The instrument-panel reading of a crash: one floating panel on the
    // ground, a danger band up top (the warning spends red where it means
    // something), the real message in mono below, actions behind a hairline.
    // "Continue" re-renders children in place — safe by construction, since
    // a persistent fault re-trips getDerivedStateFromError instantly.
    return (
      <div className="err-boundary" role="alert">
        <div className="panel err-panel fade-in">
          <div className="err-band">
            <span className="chip err-chip">
              <span className="chip-dot err-dot" aria-hidden="true" />
              Something broke
            </span>
          </div>
          <div className="err-body">
            <p className="err-msg">{sanitizeMessage(err.message || String(err))}</p>
            {err.stack && (
              <details className="err-details">
                {/* native disclosure — one system control instead of a custom
                    toggle that could itself fail */}
                <summary className="err-summary">stack</summary>
                <pre className="err-stack">{sanitizeStack(err.stack)}</pre>
              </details>
            )}
          </div>
          <div className="err-actions">
            <button
              type="button"
              className="btn"
              onClick={() => this.setState({ err: null })}
            >
              Continue
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => window.location.reload()}
            >
              Reload
            </button>
          </div>
        </div>
      </div>
    );
  }
}
