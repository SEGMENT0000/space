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
//   guess — and a failed query prints the failure claim instead of the
//   reading one, so the headline can never be stale about its own panel.
// - Rows are ranked by used share (worst first): the ledger reads in the
//   order the claim and the legend talk about, and the attention volume
//   is row one.
// - The ribbon carries a fine hairline cell grid (the dictionary of the
//   reference ledger), the fill re-encodes the printed pct, and the crit
//   tier adds a color-matched diagonal hatch — texture, not decoration;
//   color is never the only channel because each tier prints its word.
//   A volume the system reported without a capacity prints an honest
//   dashed "no reading" bed and dashes, never a fabricated 0%.
// - Row anatomy left → right: mount letter keycap, name over a quiet
//   mount/fs/kind meta, ribbon + pct + state word, two-line figure
//   readout, the primary Scan action (the row's keyboard/AT door, the
//   aria-label carries the whole readout; the row click is a mouse
//   convenience that must not double-fire). The action is transparent at
//   rest and reveals on row hover, on keyboard focus, and — where there
//   is no hover at all — at rest on coarse pointers; the reveal only
//   paints background and color, so no row ever moves.
// - Three list states, all first-class: reading (idle dot bed), no
//   mounted volumes (recovery copy), and a failed query (danger band
//   with the engine's message verbatim + Try again, mirroring the scan
//   screen's failure contract). A folder-dialog failure speaks for its
//   own action instead of mislabeling the volume query.
// - tex-hatch is absent by design: hatch marks filtered/unavailable in
//   the map world — every volume here is live and scannable, so the
//   ribbon's own tiers do the talking.
// - No delete/format path anywhere; the product is read-only by
//   construction, and the panel legend + footer say so in plain text.

import { useEffect, useRef, useState } from "react";
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

/** One voice for both failure paths: the engine's own words when it gave
 *  us words, never "[object Object]" and never an invented explanation. */
function errText(e: unknown): string {
  if (e instanceof Error && e.message) return e.message;
  if (typeof e === "string" && e) return e;
  return "the system returned no message";
}

export function DrivesView({
  demo,
  onPick,
}: {
  demo: boolean;
  onPick: (mount: string, total: number) => void;
}) {
  const [drives, setDrives] = useState<DriveInfo[] | null>(null);
  const [queryError, setQueryError] = useState<string | null>(null);
  // bumps re-run the volume query (the failure panel's Try again)
  const [queryRun, setQueryRun] = useState(0);
  const [pickError, setPickError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  // Focus follows a retry: the button that ran it unmounts while the query
  // flies, so focus is parked here and re-landed on whichever control owns
  // the outcome — the first row's Scan on success, Try again on failure —
  // instead of dropping the keyboard user back on <body>.
  const rowsRef = useRef<HTMLDivElement | null>(null);
  const failRef = useRef<HTMLDivElement | null>(null);
  const refocus = useRef(false);

  const openFolder = async () => {
    if (demo || opening) return;
    setOpening(true);
    setPickError(null);
    try {
      const path = await pickFolder();
      if (path) onPick(path, 0);
    } catch (e) {
      // the dialog's own failure, reported against its own action — the
      // volume query is healthy and must not be blamed for it
      setPickError(errText(e));
    } finally {
      setOpening(false);
    }
  };

  useEffect(() => {
    let dead = false;
    listDrives()
      .then((d) => {
        if (dead) return;
        setDrives(d);
        setQueryError(null);
      })
      .catch((e) => {
        if (dead) return;
        setQueryError(errText(e));
      });
    return () => {
      dead = true;
    };
  }, [queryRun]);

  // The retry outcome lands here once both states have settled (see the
  // refocus note above): failure → the alert band's action, success → the
  // first row's Scan, focus-visible inherited from the keyboard gesture.
  useEffect(() => {
    if (!refocus.current) return;
    if (queryError !== null) {
      refocus.current = false;
      failRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    } else if (drives !== null) {
      refocus.current = false;
      rowsRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    }
  }, [queryError, drives]);

  const retryQuery = () => {
    refocus.current = true;
    setQueryError(null);
    setDrives(null);
    setQueryRun((n) => n + 1);
  };

  // ---- the headline statement + hero figures, all real arithmetic ----
  const capacity = drives !== null ? drives.reduce((s, d) => s + d.total, 0) : 0;
  const usedSum = drives === null ? 0 : drives.reduce((s, d) => s + Math.max(0, d.total - d.free), 0);
  const withFrac =
    drives === null
      ? []
      : drives
          .map((dr) => {
            const used = Math.max(0, dr.total - dr.free);
            return { dr, used, frac: dr.total > 0 ? used / dr.total : 0 };
          })
          // ranked worst-first: the ledger reads in the order the claim
          // speaks about attention (stable sort keeps system order on ties)
          .sort((a, b) => b.frac - a.frac);
  const low = withFrac.filter((v) => v.frac >= WARN_AT);

  // claim: the state of the desk in one sentence — a failed query says so,
  // then one healthy case, one single-attention case, N-attention case,
  // plus the unknown/loading cases. Never a guess: a volume that reported
  // no capacity is excluded from any headroom claim, because we hold no
  // reading for it.
  const unknownCount = withFrac.filter((v) => v.dr.total <= 0).length;
  const claim =
    queryError !== null
      ? "Couldn't read the volume list"
      : drives === null
        ? "Reading volumes…"
        : drives.length === 0
          ? "No mounted volumes"
          : low.length === 1
            ? `${driveLetter(low[0].dr.mount)}: is ${formatPercent(low[0].frac)} used`
            : low.length > 1
              ? `${low.length} volumes are running low`
              : unknownCount === 0
                ? "Every volume has headroom"
                : unknownCount === drives.length
                  ? "No volume reported a capacity"
                  : "Every volume with a reading has headroom";

  // a failure's detail belongs to the panel below; repeating it up here
  // would be the redundant-subheading reflex
  const claimSub =
    queryError !== null || drives === null || drives.length === 0
      ? null
      : low.length === 1
        ? `${formatBytes(low[0].used)} of ${formatBytes(low[0].dr.total)} used · ${formatBytes(low[0].dr.free)} free`
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
              {drives !== null && drives.length > 0 && capacity > 0 && (
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
              {/* the dialog's own failure, attached to its own action */}
              {pickError && (
                <div className="drives-pick-fail" role="alert">
                  <span className="chip">
                    <span
                      className="chip-dot"
                      style={{ background: "var(--danger)" }}
                    />
                    Folder dialog failed
                  </span>
                  <span className="drives-pick-fail-msg">{pickError}</span>
                </div>
              )}
            </div>
          </header>

          {/* the three list states: query failure (danger band + verbatim
              message + Try again), reading, no volumes, then the ledger */}
          {queryError !== null && drives === null ? (
            <div className="panel drives-fail">
              <div className="drives-fail-band" role="alert" ref={failRef}>
                <div className="drives-fail-row">
                  <span className="chip">
                    <span
                      className="chip-dot"
                      style={{ background: "var(--danger)" }}
                    />
                    Volume query failed
                  </span>
                  <Btn
                    onClick={retryQuery}
                    title="Run the volume query again"
                  >
                    Try again
                  </Btn>
                </div>
                {/* the engine's message verbatim, selectable */}
                <p className="drives-fail-msg">{queryError}</p>
              </div>
              <p className="drives-fail-note micro">
                The system did not return a volume list. Drop a folder
                anywhere on this window, or use “Open folder or project…”
                to scan a path instead.
              </p>
            </div>
          ) : drives === null ? (
            <div className="panel drives-empty" role="status" aria-busy="true">
              <span className="drives-empty-title">Reading volumes…</span>
              <p className="drives-empty-note">
                Asking the system for the mounted volumes.
              </p>
              <div className="drives-empty-bed tex-dots" aria-hidden="true" />
            </div>
          ) : drives.length === 0 ? (
            <div className="panel drives-empty">
              <span className="drives-empty-title">No mounted volumes</span>
              <p className="drives-empty-note">
                The system reported no mounted volumes. Drop a folder anywhere
                on this window, or use “Open folder or project…” to scan a path.
              </p>
              <div className="drives-empty-bed tex-dots" aria-hidden="true" />
            </div>
          ) : (
            <div className="panel drive-panel">
              <div
                className="drive-rows"
                role="list"
                aria-label="Mounted volumes"
                ref={rowsRef}
              >
                {withFrac.map(({ dr, used, frac }) => {
                  const kind = KIND_LABEL[dr.kind];
                  // a volume the system reported without a capacity: the
                  // bed reads "no reading" (dashed), the figures print
                  // dashes — never a fabricated 0%
                  const known = dr.total > 0;
                  const level = frac >= CRIT_AT ? "is-crit" : frac >= WARN_AT ? "is-warn" : "";
                  const state = frac >= CRIT_AT ? "critical" : frac >= WARN_AT ? "low" : "";
                  // honest small fractions: 111 MB of 477 GB cannot honestly
                  // round to "0.0%" — the band prints "<0.1%"; true zero ≡ "0%".
                  const pct = !known
                    ? "—"
                    : used === 0
                      ? "0%"
                      : frac < 0.001
                        ? "<0.1%"
                        : formatPercent(frac);
                  return (
                    <div
                      key={dr.mount}
                      className="drive-row"
                      role="listitem"
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
                      <span
                        className={
                          !known
                            ? "drive-meter is-unknown"
                            : level
                              ? `drive-meter ${level}`
                              : "drive-meter"
                        }
                      >
                        <span className="usage" aria-hidden="true">
                          <span
                            className={
                              level ? `usage-fill ${level}` : "usage-fill"
                            }
                            style={{ width: `${Math.min(100, frac * 100)}%` }}
                          />
                        </span>
                        <span className="drive-pct num">{pct}</span>
                        {state && <span className="drive-state micro">{state}</span>}
                      </span>

                      {/* (d) the two-line derived readout — the fact, then the
                              consequence, both real arithmetic (dashes when
                              the capacity is unknown, never invented zeros) */}
                      <span className="drive-figs">
                        <span className="drive-fig1 num">
                          {known ? `${formatBytes(used)} used` : "—"}
                        </span>
                        <span className="drive-fig2">
                          {known
                            ? `${formatBytes(dr.free)} free · ${formatBytes(dr.total)}`
                            : "— free · — total"}
                        </span>
                      </span>

                      {/* (e) the real action; keyboard/AT door for the row.
                              Quiet at rest — the accent is rationed to real
                              warnings (One Accent per Field), so the row's
                              action reveals itself on hover/focus instead of
                              painting six orange buttons down the ledger. */}
                      <button
                        type="button"
                        className="btn drive-scan"
                        aria-label={
                          `Scan ${dr.mount}, ${dr.name}, ${dr.fs}` +
                          (kind ? `, ${kind}` : "") +
                          (known
                            ? `, ${pct} used, ${formatBytes(used)} of ${formatBytes(dr.total)}, ${formatBytes(dr.free)} free`
                            : ", capacity unknown") +
                          (state ? `, ${state}` : "")
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
              </div>

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
                <span>
                  <span className="lg-cell is-unknown" aria-hidden="true" />
                  no capacity reading
                </span>
                <span className="lg-hint">Scan a volume to see what is eating it.</span>
              </footer>
            </div>
          )}

          {/* the structural promise, in plain text, always last */}
          <footer className="drives-foot micro">
            Read-only · nothing on disk is ever modified.
          </footer>
        </div>
      </div>
    </div>
  );
}
