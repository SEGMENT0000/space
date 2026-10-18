// Volumes screen — the picker, rebuilt as a narrative instrument in the
// reference-grammar world (ins/ dashboards): a headline STATEMENT first
// ("C: is nearly full" / "Every volume has headroom"), a real-figure KPI
// pair beside the action, then ONE floating ledger panel of ranked rows —
// each row an instrument cell: letter, identity, a tick-hatched usage
// ribbon with inline state words ("low" / "critical"), and a two-line
// derived readout. The complete legend states what the ribbon and its
// states mean; the footer re-states the read-only promise.
//
// WHY this shape:
// - The headline is real arithmetic over the drives the system actually
//   reported: volumes are "running low" at ≥80% capacity, "critical" at
//   ≥92%; zero low volumes reads as the honest good case. While the query
//   is in flight the claim prints a neutral "Reading volumes…" — never a
//   guess.
// - The ribbon carries a fine hairline cell grid (the dictionary of the
//   reference ledger), the fill re-encodes the printed pct, and the crit
//   tier adds a color-matched diagonal hatch — texture, not decoration;
//   color is never the only channel because each tier prints its word.
// - Row anatomy left → right: mount letter keycap, name over a quiet
//   mount/fs/kind meta, ribbon + pct + state word, two-line figure readout,
//   the primary Scan action (the row's keyboard/AT door, aria-label carries
//   the whole readout; row click is a mouse convenience that must not
//   double-fire).
// - tex-hatch is absent by design: hatch marks filtered/unavailable in the
//   map world — every volume here is live and scannable, so the ribbon's
//   own tiers do the talking.
// - No delete/format path anywhere; the product is read-only by
//   construction, and the panel legend + footer say so in plain text.

import { useEffect, useState } from "react";
import { FolderOpen } from "lucide-react";
import { Btn } from "./ui";
import { formatBytes, formatPercent, splitBytes } from "../lib/format";
import { listDrives, pickFolder } from "../lib/ipc";
import type { DriveInfo } from "../lib/types";

// Tiers: warn ≥80% ("running low"), crit ≥92% ("critical"). The words print
// beside the pct — the ribbon is aria-hidden because the words are the
// channel color can never be alone in (Two-Channel Rule).
const WARN_AT = 0.8;
const CRIT_AT = 0.92;

const KIND_LABEL: Record<DriveInfo["kind"], string> = {
  ssd: "SSD",
  hdd: "HDD",
  removable: "removable",
  unknown: "",
};

/** "C:\Users" -> "C" — the mount letter, as Windows itself prints it. */
function driveLetter(mount: string): string {
  const alpha = mount.match(/[A-Za-z]/);
  if (alpha) return alpha[0].toUpperCase();
  const bare = mount.replace(/^[^\w]+/, "").charAt(0);
  return bare ? bare.toUpperCase() : "—";
}

export function DrivesView({
  demo,
  onPick,
}: {
  demo: boolean;
  onPick: (mount: string, total: number) => void;
}) {
  const [drives, setDrives] = useState<DriveInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  const openFolder = async () => {
    if (demo || opening) return;
    setOpening(true);
    try {
      const path = await pickFolder();
      if (path) onPick(path, 0);
    } catch (e) {
      setError(String(e));
    } finally {
      setOpening(false);
    }
  };

  useEffect(() => {
    listDrives()
      .then(setDrives)
      .catch((e) => setError(String(e)));
  }, []);

  // ---- the headline statement + hero figures, all real arithmetic ----
  const capacity = drives !== null ? drives.reduce((s, d) => s + d.total, 0) : 0;
  const usedSum = drives === null ? 0 : drives.reduce((s, d) => s + Math.max(0, d.total - d.free), 0);
  const withFrac = drives === null ? [] : drives.map((dr) => {
    const used = Math.max(0, dr.total - dr.free);
    return { dr, used, frac: dr.total > 0 ? used / dr.total : 0 };
  });
  const low = withFrac.filter((v) => v.frac >= WARN_AT);

  // claim: the state of the desk in one sentence — one healthy case, one
  // single-attention case, N-attention case, plus the unknown/loading case.
  const claim =
    drives === null
      ? "Reading volumes…"
      : drives.length === 0
        ? "No mounted volumes"
        : low.length === 0
          ? "Every volume has headroom"
          : low.length === 1
            ? `${driveLetter(low[0].dr.mount)}: is ${formatPercent(low[0].frac)} used`
            : `${low.length} ${low.length === 1 ? "volume is" : "volumes are"} running low`;

  const claimSub =
    drives === null
      ? null
      : drives.length === 0
        ? null
        : low.length === 1
          ? `${formatBytes(low[0].used)} of ${formatBytes(low[0].dr.total)} — ${formatBytes(low[0].dr.free)} free.`
          : low.length > 1
            ? low.map((v) => `${driveLetter(v.dr.mount)}: at ${formatPercent(v.frac)}`).join(" · ")
            : null;

  // the KPI pair beside the action: Σ used and Σ capacity (both real sums)
  const kpiUsed = drives !== null && drives.length > 0 ? splitBytes(usedSum) : null;
  const kpiCap = drives !== null && drives.length > 0 ? splitBytes(capacity) : null;

  return (
    <div className="screen fade-in">
      <div className="drives">
        <div className="drives-inner">
          {/* wordmark on the ground; the statement is the content headline */}
          <header className="drives-head">
            <div className="drives-ident">
              <div className="drives-brand">
                <h1 className="drives-h">space</h1>
                {demo && (
                  <span className="chip drives-demo">
                    <span className="chip-dot" />
                    Synthetic data (browser preview)
                  </span>
                )}
              </div>
              <h2 className="drives-claim">{claim}</h2>
              {claimSub && <p className="drives-claim-sub">{claimSub}</p>}
              {drives !== null && drives.length > 0 && (
                <p className="drives-claim-sub">
                  {kpiUsed ? `${kpiUsed.num} ${kpiUsed.unit} used` : "0 B used"} across{" "}
                  {drives.length} {drives.length === 1 ? "volume" : "volumes"} ·{" "}
                  {kpiCap ? `${kpiCap.num} ${kpiCap.unit}` : "—"} total capacity
                </p>
              )}
            </div>
            <div className="drives-actions">
              {/* demo disables this one: the browser has no folder dialog */}
              <Btn
                onClick={openFolder}
                disabled={demo || opening}
                title="Scan a specific folder or project"
              >
                <FolderOpen size={14} aria-hidden="true" />
                <span>{opening ? "Opening…" : "Open folder or project…"}</span>
              </Btn>
              <span className="drives-drop-hint micro">
                or drop a folder anywhere on this window
              </span>
            </div>
          </header>

          {/* a folder-picker failure prints above the ledger; a volume-query
              failure stands alone */}
          {error && (
            <div className="drives-error" role="alert">
              <span className="micro">Volume query failed</span>
              <span className="drives-error-msg">{error}</span>
            </div>
          )}

          {drives === null && !error ? (
            <div className="panel drives-empty">
              <span className="drives-empty-title">Reading volumes…</span>
              <div className="drives-empty-bed tex-dots" aria-hidden="true" />
            </div>
          ) : drives !== null && drives.length === 0 ? (
            <div className="panel drives-empty">
              <span className="drives-empty-title">No mounted volumes</span>
              <p className="drives-empty-note">
                The system reported no mounted volumes. Drop a folder anywhere
                on this window, or use “Open folder or project…” to scan a path.
              </p>
              <div className="drives-empty-bed tex-dots" aria-hidden="true" />
            </div>
          ) : drives !== null && drives.length > 0 ? (
            <div className="panel drive-panel">
              {withFrac.map(({ dr, used, frac }) => {
                const kind = KIND_LABEL[dr.kind];
                const level = frac >= CRIT_AT ? "is-crit" : frac >= WARN_AT ? "is-warn" : "";
                const state = frac >= CRIT_AT ? "critical" : frac >= WARN_AT ? "low" : "";
                // honest small fractions: 111 MB of 477 GB cannot honestly
                // round to "0.0%" — the band prints "<0.1%"; true zero ≡ "0%".
                const pct = used === 0 ? "0%" : frac < 0.001 ? "<0.1%" : formatPercent(frac);
                return (
                  <div
                    key={dr.mount}
                    className="drive-row"
                    onClick={() => onPick(dr.mount, dr.total)}
                  >
                    {/* (a) the mount letter, plain for every row — hatch is
                            reserved for filtered/unavailable states */}
                    <span className="drive-chip num" aria-hidden="true">
                      {driveLetter(dr.mount)}
                    </span>

                    {/* (b) identity: name primary, mount/fs/kind quiet */}
                    <span className="drive-id">
                      <span className="drive-name">{dr.name}</span>
                      <span className="drive-meta">
                        <span className="drive-meta-line">
                          <span className="num">{dr.mount}</span>
                          {dr.fs ? ` · ${dr.fs}` : ""}
                          {kind && kind !== "removable" ? ` · ${kind}` : ""}
                        </span>
                        {dr.isRemovable && (
                          <span className="chip drive-flag">removable</span>
                        )}
                      </span>
                    </span>

                    {/* (c) the usage ribbon + the pct + the tier word */}
                    <span className={level ? `drive-meter ${level}` : "drive-meter"}>
                      <span className="usage" aria-hidden="true">
                        <span
                          className={level ? `usage-fill ${level}` : "usage-fill"}
                          style={{ width: `${Math.min(100, frac * 100)}%` }}
                        />
                      </span>
                      <span className="drive-pct num">{pct}</span>
                      {state && <span className="drive-state micro">{state}</span>}
                    </span>

                    {/* (d) the two-line derived readout — the fact, then the
                            consequence, both real arithmetic */}
                    <span className="drive-figs">
                      <span className="drive-fig1 num">{formatBytes(used)} used</span>
                      <span className="drive-fig2">
                        {formatBytes(dr.free)} free · {formatBytes(dr.total)}
                      </span>
                    </span>

                    {/* (e) the real action; keyboard/AT door for the row */}
                    <button
                      type="button"
                      className="btn btn-primary drive-scan"
                      aria-label={
                        `Scan ${dr.mount}, ${dr.name}, ${dr.fs}` +
                        (kind ? `, ${kind}` : "") +
                        ` — ${formatPercent(frac)} used,` +
                        ` ${formatBytes(used)} of ${formatBytes(dr.total)},` +
                        ` ${formatBytes(dr.free)} free`
                      }
                      onClick={(e) => {
                        e.stopPropagation();
                        onPick(dr.mount, dr.total);
                      }}
                    >
                      Scan
                    </button>
                  </div>
                );
              })}

              {/* the ribbon's legend: every state word the panel above can
                  print, decoded once at the panel's edge */}
              <footer className="drive-legend micro">
                <span>
                  <span className="lg-cell" aria-hidden="true" />
                  ribbon = used share of capacity
                </span>
                <span>
                  <span className="lg-cell is-warn" aria-hidden="true" />
                  ≥80% low
                </span>
                <span>
                  <span className="lg-cell is-crit" aria-hidden="true" />
                  ≥92% critical
                </span>
                <span className="lg-hint">Scan a volume to see what is eating it.</span>
              </footer>
            </div>
          ) : null}

          {/* the structural promise, in plain text, always last */}
          <footer className="drives-foot micro">
            Read-only — nothing on disk is ever modified.
          </footer>
        </div>
      </div>
    </div>
  );
}
