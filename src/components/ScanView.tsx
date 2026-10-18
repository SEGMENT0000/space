// Scan-in-progress screen — one floating instrument panel on the ground.
//
// WHY this shape: ProgressPayload (types.ts, the frozen IPC contract) carries
// no total, so a determinate percentage would be a fabrication. The honest
// meter is indeterminate — an accent sweep on a dot-matrix bed — and the %
// readout at its right end stays "—" with a title explaining why. Everything
// else is verbatim payload: the scanned-bytes hero, the four counters, elapsed
// riding the head's meta line, and the walking path. `errors` is the count of
// entries the walk could not read — the product calls those "skipped".
//
// Rate is NOT in the payload: it is derived here from consecutive events (EMA
// over files/second deltas, seeded by the overall average, because NTFS walks
// burst; raw per-tick deltas would jitter the readout). The computation lives
// in a ref-based effect and lands as ONE state write per event, so renders
// stay pure and the render cadence never exceeds the event cadence (~8Hz).
// The strip plots the RAW observed deltas (not the EMA) in a ref-held sliding
// window whose bars are keyed by a stable monotonic sample index — a new
// sample shifts the window one slot, so React mounts/unmounts a single bar
// per event and never rebuilds the other 89.
//
// Failure contract: `error` (optional; App passes scanError) swaps the meter
// for a danger band — status chip + the engine's message verbatim, selectable
// because a technical user copies it — and trades Cancel for "Back to volumes"
// (`onBack`, optional; absent means no action is offered). Frozen counters and
// bars under a failure are the scan's last reported observations, kept honest.

import { useEffect, useRef, useState } from "react";
import { formatCount, formatElapsed, splitBytes } from "../lib/format";
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

/** One plotted sample. `k` is a monotonic sample index — also the bar's React
    key, so the sliding window never remounts bars it has already drawn. */
interface Sample {
  k: number;
  /** raw observed files/s for that interval */
  v: number;
}

/** Everything the instrument re-derives per progress event. */
interface ScanLive {
  /** last smoothed rate (files/s); null until the first trusted sample */
  rate: number | null;
  /** max of the plotted window (files/s) */
  peak: number;
  /** raw samples, oldest -> newest, capped at RATE_WINDOW (ref-copied) */
  bars: Sample[];
}

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
  const [live, setLive] = useState<ScanLive>({ rate: null, peak: 0, bars: [] });
  const prevSampleRef = useRef<{ files: number; ms: number } | null>(null);
  const smoothedRef = useRef(0);
  // the window lives in a ref; state gets an immutable copy for render
  const barsRef = useRef<Sample[]>([]);
  const seqRef = useRef(0);

  useEffect(() => {
    if (!progress) return;
    // StrictMode double-invokes effects: a payload already folded into the
    // windows must not count twice. `elapsed_ms` strictly increases across
    // real events (and across demo ticks), so equal {files, ms} = same event.
    const prev = prevSampleRef.current;
    if (prev && prev.ms === progress.elapsedMs && prev.files === progress.files)
      return;
    prevSampleRef.current = { files: progress.files, ms: progress.elapsedMs };

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

    // window max, computed here so renders never iterate (no spread in render)
    let peak = 0;
    for (const b of barsRef.current) if (b.v > peak) peak = b.v;

    // ONE state write per progress event — the events' own cadence
    setLive({
      rate: smoothedRef.current === 0 ? null : smoothedRef.current,
      peak,
      bars: barsRef.current.slice(),
    });
  }, [progress]);

  const failed = !!error;
  // before the first event the walker is on the root itself — that is a real
  // path, not a placeholder; `current` can also arrive "" early on
  const path = progress?.current || root;
  const { rate, peak, bars } = live;
  const scanned = progress ? splitBytes(progress.bytes) : null;

  return (
    <div className="screen fade-in">
      <div className="scan">
        <section
          className="panel scan-panel"
          aria-label={failed ? "Scan failed" : "Scan in progress"}
        >
          {/* head: volume identity left, quiet cancel right */}
          <header className="scan-head">
            <div className="scan-id">
              <h1 className="scan-name" title={root}>
                {leafFolder(root)}
              </h1>
              <span className="scan-mount num" title={root}>
                {root}
                {progress ? ` · ${formatElapsed(progress.elapsedMs)}` : ""}
              </span>
            </div>
            {!failed && <Btn onClick={onCancel}>Cancel</Btn>}
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
              fill would claim a progress state the payload cannot back. */}
          <div className="scan-hero">
            <div className="stat is-hero">
              <span className="micro">Scanned</span>
              <span className="stat-v">
                {scanned ? (
                  <>
                    {scanned.num}
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
                  className="scan-meter tex-dots"
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

          {/* counters: four cells divided by hairlines — never cards */}
          <div className="scan-kpis">
            <div className="stat is-sm scan-kpi">
              <span className="micro">Files scanned</span>
              <span className="stat-v">
                {progress ? formatCount(progress.files) : "—"}
              </span>
            </div>
            <div className="stat is-sm scan-kpi">
              <span className="micro">Directories</span>
              <span className="stat-v">
                {progress ? formatCount(progress.dirs) : "—"}
              </span>
            </div>
            <div className="stat is-sm scan-kpi">
              <span className="micro">Skipped</span>
              <span
                className="stat-v"
                title="Entries the walk could not read"
              >
                {progress ? formatCount(progress.errors) : "—"}
              </span>
            </div>
            <div className="stat is-sm scan-kpi">
              <span className="micro">Rate</span>
              <span className="stat-v">
                {rate === null ? "—" : fmtRate(rate)}
                {rate !== null && <span className="u">files/s</span>}
              </span>
            </div>
          </div>

          {/* rate history: raw observed files/s per trusted delta, height
              quantized to flat steps, on a dot-matrix bed */}
          <div className="scan-rate">
            <div className="scan-rate-head">
              <span className="micro">Rate history</span>
              <span className="scan-peak">
                {peak > 0 ? (
                  <>
                    <span className="micro">Peak</span>
                    <span className="num">{fmtRate(peak)}</span>
                    <span className="scan-peak-u">files/s</span>
                  </>
                ) : (
                  <span className="num">—</span>
                )}
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

          {/* walking readout: head-clipped so the entry under the walker
              stays visible; the title attribute carries the path in full */}
          <footer className="scan-foot">
            <span className="micro">Walking</span>
            <span className="scan-path num" title={path}>
              {/* the LRM pins trailing neutrals ("C:", "C:\\") to the LTR run —
                  without it the rtl head-clip mirrors them into ":C" */}
              {path + "\u200e"}
            </span>
          </footer>
        </section>
      </div>
    </div>
  );
}
