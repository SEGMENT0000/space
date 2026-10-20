// Scan-in-progress screen — one floating instrument panel on the ground.
//
// WHY this shape: ProgressPayload (types.ts, the frozen IPC contract) carries
// no total, so a determinate percentage would be a fabrication. The honest
// meter is indeterminate — an accent sweep on a dot-matrix bed — and the %
// readout at its right end stays "—" with a title explaining why. Everything
// else is verbatim payload: the scanned-bytes hero, the four counters, the
// elapsed clock in its own fixed slot in the head, and the walking path.
// `errors` is the count of entries the walk could not read — the product
// calls those "skipped".
//
// ONE commit per THROTTLE_MS, never one per event: the Rust reporter emits
// every 120 ms (scanner.rs PROGRESS_INTERVAL) while 16 walker threads
// saturate the CPU, so every event folds into refs (sample window, EMA,
// peak) and only a throttled commit carries a whole snapshot — payload
// values included — into state. Render reads that snapshot and never the
// changing `progress` prop, so a skipped commit changes nothing on screen
// and loses nothing either (the refs already hold the newer fold).
//
// Rate is NOT in the payload: it is derived here from consecutive events (EMA
// over files/second deltas, seeded by the overall average, because NTFS walks
// burst; raw per-tick deltas would jitter the readout). The strip plots the
// RAW observed deltas (not the EMA) in a ref-held sliding window whose bars
// are keyed by a stable monotonic sample index — a new sample shifts the
// window one slot, so React mounts/unmounts a single bar per commit and never
// rebuilds the other 89.
//
// Phases: idle (mounted, nothing reported yet) → scanning → cancelling (Cancel
// pressed; the button says so and stops accepting presses; Escape is the same
// action) → failed (`error` swaps the meter for a danger band — status chip +
// the engine's message verbatim, selectable because a technical user copies it
// — and trades Cancel for "Back to volumes"). Done is the shell's transition:
// `scan://done` swaps the screen into the map. Under a failure the counters
// and bars freeze on the scan's last reported observations, kept honest.
//
// Edge states are quiet words, never an overlay: a watchdog prints "No
// progress reported for Ns" and parks the sweep (the instrument stops
// claiming motion when motion stopped), an empty walk prints "0 files so far"
// only once elapsed time makes that worth stating, and `errors` prints "N
// entries could not be read" — permission-denied lands here, counted and
// named in plain text, not thrown at the reader as a raw dump.

import {
  Fragment,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ellipsizePath,
  formatCount,
  formatElapsed,
  splitBytes,
} from "../lib/format";
import type { ProgressPayload } from "../lib/types";
import { Btn } from "./ui";

/** "D:\\archive" -> "archive". The head shows the leaf; the title attribute
    on the name and on the mount line carries the full root. */
function leafFolder(root: string): string {
  const parts = root.split(/[\\/]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : root;
}

/** 812.4 -> "812.4"; 2412.3 -> "2,412" — group only once the figure is long. */
function fmtRate(r: number): string {
  return r >= 1000 ? formatCount(Math.round(r)) : r.toFixed(1);
}

/** EMA weight toward the newest instantaneous delta: responsive, still damped. */
const RATE_ALPHA = 0.3;
/** Minimum ms between samples to trust a delta — below this the division spikes. */
const MIN_SAMPLE_MS = 120;
/** Sliding window of the rate strip: one bar per trusted sample (~11s at 8Hz). */
const RATE_WINDOW = 90;
/** Height quantization of the bars: flat tops in 1/14 steps — a step chart,
    not a smooth ramp; sub-pixel bar churn between events buys nothing. */
const BAR_LEVELS = 14;
/** Floor on the gap between rendered commits. The emitter runs at 120 ms, so
    this never drops a real event; it bounds renders at 10 Hz if the cadence
    ever tightens, which is the whole throttle: no per-event React work. */
const THROTTLE_MS = 100;
/** No first report by then → the quiet "nothing reported yet" line. Real
    scans report within ~120 ms, so this only ever names a cold start. */
const WAIT_MS = 900;
/** Reports quiet for this long while still on this screen → the stall line,
    and the sweep stops (a frozen strip and a moving sweep would disagree). */
const STALL_MS = 2000;
/** Elapsed before "0 files so far" is worth stating — it is true at 0 ms too,
    but printing it on every fresh scan would flash a non-state. */
const EMPTY_MS = 1500;
/** Middle-truncation budget for the walking path: 72 mono chars fit the
    620px panel's foot at every window size the shell allows (min 1024px);
    the CSS ellipsis behind it is a last resort, not the plan. */
const PATH_MAX = 72;

/** One plotted sample. `k` is a monotonic sample index — also the bar's React
    key, so the sliding window never remounts bars it has already drawn. */
interface Sample {
  k: number;
  /** raw observed files/s for that interval */
  v: number;
}

/** The one immutable snapshot render reads: payload fields plus everything
    derived from them, committed together so counters, path and rate can
    never disagree within a frame. */
interface ScanSnap {
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  current: string;
  elapsedMs: number;
  /** last smoothed rate (files/s); null until the first trusted sample */
  rate: number | null;
  /** max of the plotted window (files/s) */
  peak: number;
  /** raw samples, oldest -> newest, capped at RATE_WINDOW (ref-copied) */
  bars: Sample[];
}

/** Shared empty window — one module-scope array, never allocated per render. */
const NO_BARS: Sample[] = [];

export function ScanView({
  root,
  progress,
  error,
  onCancel,
  onBack,
}: {
  root: string;
  progress: ProgressPayload | null;
  error?: string | null;
  onCancel: () => void;
  onBack?: () => void;
}) {
  const [snap, setSnap] = useState<ScanSnap | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [waiting, setWaiting] = useState(false);
  const [stalled, setStalled] = useState(0);
  const screenRef = useRef<HTMLDivElement | null>(null);

  const prevRef = useRef<{ files: number; ms: number } | null>(null);
  const smoothedRef = useRef(0);
  // the window lives in a ref; each commit ships an immutable copy
  const barsRef = useRef<Sample[]>([]);
  const seqRef = useRef(0);
  const lastCommitRef = useRef(0);
  const lastReportRef = useRef(0);

  const failed = !!error;

  // --------------------------------------------------------- fold + throttle
  useEffect(() => {
    if (!progress) {
      // the shell cleared the payload while we are mounted (a fresh scan is
      // being set up): start the instrument over instead of holding the
      // previous walk's numbers
      prevRef.current = null;
      smoothedRef.current = 0;
      barsRef.current = [];
      setSnap(null);
      setStalled(0);
      return;
    }
    // StrictMode double-invokes effects: a payload already folded into the
    // windows must not count twice. `elapsedMs` strictly increases across
    // real events (and across demo ticks), so equal {files, ms} = same event.
    const prev = prevRef.current;
    if (prev && prev.ms === progress.elapsedMs && prev.files === progress.files)
      return;

    const now = performance.now();
    // a payload that went backwards is a NEW scan sharing our mount — clear
    // the derived windows so the rate never averages two walks together
    if (prev && (progress.elapsedMs < prev.ms || progress.files < prev.files)) {
      smoothedRef.current = 0;
      barsRef.current = [];
    }
    prevRef.current = { files: progress.files, ms: progress.elapsedMs };
    lastReportRef.current = now;

    let inst: number | null = null;
    if (prev && progress.elapsedMs - prev.ms >= MIN_SAMPLE_MS) {
      inst =
        (progress.files - prev.files) /
        ((progress.elapsedMs - prev.ms) / 1000);
    }
    if (inst !== null && Number.isFinite(inst) && inst >= 0) {
      // a zero delta is real (a stall) and is plotted as the floor tick
      smoothedRef.current =
        smoothedRef.current === 0
          ? inst
          : smoothedRef.current + (inst - smoothedRef.current) * RATE_ALPHA;
      const bars = barsRef.current;
      bars.push({ k: seqRef.current++, v: inst });
      if (bars.length > RATE_WINDOW) bars.shift();
    } else if (
      smoothedRef.current === 0 &&
      progress.files > 0 &&
      progress.elapsedMs > 0
    ) {
      // no usable delta yet — seed from the overall average once it means
      // something; nothing to plot until a trusted delta exists
      smoothedRef.current = progress.files / (progress.elapsedMs / 1000);
    }

    // window max, computed here so renders never iterate
    let peak = 0;
    for (const b of barsRef.current) if (b.v > peak) peak = b.v;

    // throttled commit — the whole snapshot lands as one state write, so a
    // fast emitter can never outrun the render cadence
    if (now - lastCommitRef.current >= THROTTLE_MS) {
      lastCommitRef.current = now;
      setSnap({
        files: progress.files,
        dirs: progress.dirs,
        bytes: progress.bytes,
        errors: progress.errors,
        current: progress.current,
        elapsedMs: progress.elapsedMs,
        rate: smoothedRef.current === 0 ? null : smoothedRef.current,
        peak,
        bars: barsRef.current.slice(),
      });
    }
  }, [progress]);

  // --------------------------------------------------------- idle gate
  // "nothing reported yet" is only true while no payload has ever arrived;
  // the delay keeps a normal 120 ms cold start from flashing it
  useEffect(() => {
    if (progress) {
      setWaiting(false);
      return;
    }
    const t = window.setTimeout(() => setWaiting(true), WAIT_MS);
    return () => window.clearTimeout(t);
  }, [progress]);

  // --------------------------------------------------------- stall watchdog
  const hasReport = snap !== null;
  const live = hasReport && !error;
  useEffect(() => {
    if (!live) {
      setStalled(0);
      return;
    }
    const id = window.setInterval(() => {
      const quiet = performance.now() - lastReportRef.current;
      // whole seconds only: the state changes at 1 Hz, never per tick
      setStalled(quiet >= STALL_MS ? Math.floor(quiet / 1000) : 0);
    }, 500);
    return () => window.clearInterval(id);
  }, [live]);

  // --------------------------------------------------------- cancel
  const requestCancel = useCallback(() => {
    setCancelling(true);
    onCancel();
  }, [onCancel]);

  // Escape is the same action as the button while this screen owns the key.
  // The overlay guard keeps the command palette's own Escape (its input is
  // the event target then) from cancelling a scan on the way out.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      const onGround =
        !t || t === document.body || t === document.documentElement;
      if (!onGround && !screenRef.current?.contains(t)) return;
      if (failed || cancelling) return;
      e.preventDefault();
      requestCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [failed, cancelling, requestCancel]);

  // --------------------------------------------------------- render values
  const path = snap?.current || root;
  const scanned = snap ? splitBytes(snap.bytes) : null;
  const rate = snap ? snap.rate : null;
  const peak = snap ? snap.peak : 0;
  const bars = snap ? snap.bars : NO_BARS;

  // quiet status: observed facts only, in the product's own words. Never the
  // failure itself (the danger band owns that), never a live region (these
  // numbers move every commit — announcing them would flood a screen reader)
  const notes: ReactNode[] = [];
  if (stalled > 0) {
    notes.push(
      <span key="stall">
        No progress reported for <span className="num">{stalled}</span>s
      </span>,
    );
  } else if (waiting) {
    notes.push(<span key="wait">No progress reported yet</span>);
  } else if (snap && snap.files === 0 && snap.elapsedMs >= EMPTY_MS) {
    notes.push(
      <span key="empty">
        <span className="num">0</span> files so far
      </span>,
    );
  }
  if (snap && snap.errors > 0) {
    notes.push(
      <span key="skip">
        <span className="num">{formatCount(snap.errors)}</span>{" "}
        {snap.errors === 1 ? "entry" : "entries"} could not be read
      </span>,
    );
  }

  return (
    <div className="screen fade-in" ref={screenRef}>
      <div className="scan">
        <section
          className="panel scan-panel"
          aria-label={
            failed
              ? "Scan failed"
              : cancelling
                ? "Cancelling the scan"
                : "Scan in progress"
          }
        >
          {/* head: volume identity left, the one action right */}
          <header className="scan-head">
            <div className="scan-id">
              <h1 className="scan-name" title={root}>
                {leafFolder(root)}
              </h1>
              <div className="scan-mount-row">
                <span className="scan-mount num" title={root}>
                  {root}
                </span>
                {snap && (
                  <>
                    <span className="scan-sep" aria-hidden="true">
                      ·
                    </span>
                    <span
                      className="scan-elapsed num"
                      title="Elapsed since the scan started"
                    >
                      {formatElapsed(snap.elapsedMs)}
                    </span>
                  </>
                )}
              </div>
            </div>
            {!failed && (
              <Btn
                onClick={requestCancel}
                disabled={cancelling}
                title={cancelling ? "Cancel requested" : "Stop the scan (Esc)"}
              >
                {cancelling ? "Cancelling…" : "Cancel"}
                {!cancelling && (
                  <span className="kbd" aria-hidden="true">
                    esc
                  </span>
                )}
              </Btn>
            )}
          </header>

          {/* failure band: chip + the engine's message verbatim + one action */}
          {failed && (
            <div className="scan-fail" role="alert">
              <div className="scan-fail-row">
                <span className="chip">
                  <span
                    className="chip-dot"
                    style={{ background: "var(--danger)" }}
                  />
                  Scan failed
                </span>
                {onBack && <Btn onClick={onBack}>Back to volumes</Btn>}
              </div>
              <p className="scan-fail-msg">{error}</p>
            </div>
          )}

          {/* hero: scanned bytes over an indeterminate meter on a dot bed.
              The meter row exists only while the scan is live — a frozen
              fill would claim a progress state the payload cannot back —
              and `is-stalled` parks the sweep when reports go quiet. */}
          <div className="scan-hero">
            <div className="stat is-hero">
              <span className="micro">Scanned</span>
              <span className="stat-v">
                {scanned ? (
                  <>
                    <span className="slot">{scanned.num}</span>
                    <span className="u">{scanned.unit}</span>
                  </>
                ) : (
                  "—"
                )}
              </span>
            </div>
            {!failed && (
              <div className="scan-meter-row">
                <div
                  className={`scan-meter tex-dots${
                    stalled > 0 ? " is-stalled" : ""
                  }`}
                  role="progressbar"
                  aria-label="Scan progress"
                >
                  <span className="scan-meter-fill" />
                </div>
                {/* no total in the payload — the % slot stays honest */}
                <span
                  className="scan-pct num"
                  title="The scanner reports no total, so percent complete is unknown"
                >
                  —
                </span>
              </div>
            )}
          </div>

          {/* counters: four cells divided by hairlines — never cards. Each 1fr
              cell IS the figure's fixed slot (tabular mono), so growing digits
              never move a hairline; Rate carries a unit, so its figure sits in
              a fixed ch slot that keeps "files/s" pinned as digits grow. */}
          <div className="scan-kpis">
            <div className="stat is-sm scan-kpi">
              <span className="micro">Files scanned</span>
              <span className="stat-v">
                {snap ? formatCount(snap.files) : "—"}
              </span>
            </div>
            <div className="stat is-sm scan-kpi">
              <span className="micro">Directories</span>
              <span className="stat-v">
                {snap ? formatCount(snap.dirs) : "—"}
              </span>
            </div>
            <div className="stat is-sm scan-kpi">
              <span className="micro">Skipped</span>
              <span
                className="stat-v"
                title="Entries the walk could not read"
              >
                {snap ? formatCount(snap.errors) : "—"}
              </span>
            </div>
            <div className="stat is-sm scan-kpi">
              <span className="micro">Rate</span>
              <span
                className="stat-v"
                title="Files reported per second, smoothed across samples"
              >
                <span className="slot">
                  {rate === null ? "—" : fmtRate(rate)}
                </span>
                <span className="u">files/s</span>
              </span>
            </div>
          </div>

          {/* rate history: raw observed files/s per trusted delta, height
              quantized to flat steps, on a dot-matrix bed */}
          <div className="scan-rate">
            <div className="scan-rate-head">
              <span className="micro">Rate history</span>
              <span className="scan-peak">
                <span className="micro">Peak</span>
                <span className="num slot">
                  {peak > 0 ? fmtRate(peak) : "—"}
                </span>
                <span className="scan-peak-u">files/s</span>
              </span>
            </div>
            <div
              className="scan-bars tex-dots"
              role="img"
              aria-label="Rate history in files per second, one bar per interval, newest at the right"
            >
              {bars.map((b) => {
                const level =
                  peak > 0
                    ? Math.round((b.v / peak) * BAR_LEVELS) / BAR_LEVELS
                    : 0;
                return (
                  <span
                    key={b.k}
                    className="scan-bar"
                    style={{
                      transform: `scaleY(${Math.max(0.05, level).toFixed(3)})`,
                    }}
                  />
                );
              })}
            </div>
          </div>

          {/* quiet status: empty walk, unreadable entries, quiet reporter —
              one clipped line so it can never push the foot around */}
          {!failed && notes.length > 0 && (
            <p className="scan-status micro" aria-live="off">
              {notes.map((n, i) => (
                <Fragment key={i}>
                  {i > 0 && " · "}
                  {n}
                </Fragment>
              ))}
            </p>
          )}

          {/* walking readout: middle-truncated to a fixed budget so the line
              never overflows and never rewraps; the title attribute carries
              the path in full */}
          <footer className="scan-foot">
            <span className="micro">Walking</span>
            <span className="scan-path num" title={path}>
              {ellipsizePath(path, PATH_MAX)}
            </span>
          </footer>
        </section>
      </div>
    </div>
  );
}
