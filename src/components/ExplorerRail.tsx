// Inspector rail — the instrument panel's right column, three hairline-
// stacked cells: selection/current-folder readout, the composition
// (one row per category; a row click filters the map), and the scan's
// largest files under a local name filter — a rank list, index over name
// over mono size.
//
// Layout law: exactly ONE scroll region — the file list (flex:1, min 0).
// The readout and composition are fixed sections; the rail itself never
// scrolls, so the hero figure and the toggles stay in view on any window.
//
// Type register (anti-slop): labels are sentence-case Geist — section
// titles 11.5/500 ink-2, the mode line the same step, the item name
// 14/500, rows 12.5/16. Mono tabular (.num) is reserved for FIGURES
// (sizes, counts, percentages, the hero value, the rank index) and
// PATHS. Never uppercase-mono on a label.
//
// States, all quiet: no data is an EmptyState (what's missing + why),
// loading is static --fill-quiet skeleton rows (no shimmer — a skeleton
// that sits still is honest about not knowing), and a failure is a
// StatusWord plus the engine's own message — never a raw error overlay.
// Missing figures print the product's dash; nothing is fabricated.
//
// Hover truth: each file row opens the shared HoverCard (ui.tsx) with THAT
// row's payload — anchored at enter coordinates, interactive, kept alive by
// a 150ms leave-grace so the card's real actions stay clickable. Rows carry
// no native tooltips; the readout's icon-only actions carry title +
// aria-label (DESIGN's icon-button rule) via the shared IconButton.

import { Copy, FolderOpen, Search, SquareArrowOutUpRight } from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
} from "react";
import { CATEGORY_LABELS, categoryStyle } from "../lib/colors";
import {
  ellipsizePath,
  formatBytes,
  formatCount,
  formatPercent,
  plural,
  splitBytes,
} from "../lib/format";
import { demoState, openWithSystem, revealInExplorer } from "../lib/ipc";
import type { LargestFile } from "../lib/types";
import {
  EmptyState,
  HoverCard,
  IconButton,
  StatusWord,
  VisuallyHidden,
  type HoverCardData,
} from "./ui";

export interface RailDetail {
  id: number;
  name: string;
  size: number;
  isDir: boolean;
  cat: number;
  fileCount: number;
  kidCount: number;
  path: string;
}

export interface RailSelection {
  ids: number[];
  total: number;
  detail: RailDetail | null;
}

/** How long the pointer may be "off" the row (or the card) before the card
 *  closes — long enough to walk the 8px gap into the card's action buttons. */
const LEAVE_GRACE_MS = 150;

/** A row's open card: identity-correct payload (that row's `LargestFile`),
 *  frozen at the pointer's enter coordinates relative to the rail. */
type RowHover = { f: LargestFile; x: number; y: number };

/** Rank-list name budget: the name column measures ~165px (300 rail −
 *  section padding − rank/size columns − the stable scrollbar gutter), and
 *  Geist 12.5 runs ≈6.5px/char, so 25 characters sit inside it. Longer names
 *  are cut from the LEFT — the tail (extension included) is the identity,
 *  the same contract ellipsizePath() applies to paths — and CSS ellipsis
 *  stays as the backstop for unusually wide glyphs. */
const NAME_TAIL_MAX = 25;

function tailName(name: string, max = NAME_TAIL_MAX): string {
  if (name.length <= max) return name;
  return `…${name.slice(Math.max(0, name.length - (max - 1)))}`;
}

function catLabel(cat: number): string {
  return CATEGORY_LABELS[cat] ?? CATEGORY_LABELS[CATEGORY_LABELS.length - 1];
}

/** Hero pair: the value in display-size mono, the unit in the small `.u`.
 *  A measured zero reads as an honest "0 B"; an unknown (not-yet-loaded or
 *  non-finite) figure prints the product's dash — never a fabricated 0. */
function heroOf(n: number): { num: string; unit: string } {
  if (!Number.isFinite(n) || n < 0) return { num: "—", unit: "" };
  if (n === 0) return { num: "0", unit: "B" };
  return splitBytes(n);
}

export function ExplorerRail({
  selection,
  current,
  totalScan,
  categories,
  hiddenCats,
  onToggleCat,
  largest,
  onPickLargest,
  onReveal,
  onCopyPath,
  onOpen,
  selectedId,
  currentId,
  theme,
  loading = false,
  error = null,
}: {
  selection: RailSelection | null;
  current: { name: string; size: number; items: number; path: string };
  totalScan: number;
  categories: number[];
  hiddenCats: ReadonlySet<number>;
  onToggleCat: (cat: number) => void;
  largest: LargestFile[];
  onPickLargest: (id: number) => void;
  onReveal: (id: number) => void;
  onCopyPath: (id: number) => void;
  onOpen: (id: number) => void;
  selectedId: number | null;
  currentId: number;
  theme: "light" | "dark";
  /** True while the explorer's folder read is in flight: the rail prints
   *  dashes instead of stale figures and swaps both data sections for
   *  skeleton rows. Optional — absent, the rail renders as data-ready. */
  loading?: boolean;
  /** A real failure message (engine text, verbatim). Rendered as a quiet
   *  StatusWord + message in the readout cell — never an overlay. */
  error?: string | null;
}) {
  // Filter is the rail's only persisted local state. `largest` is a frozen
  // prop (≤30 rows fetched once per scan), so filtering is a per-keystroke
  // slice of an in-memory array — no IPC round-trip, no memo needed.
  const [query, setQuery] = useState("");

  // ---- row hover card ----------------------------------------------------
  // One open card at a time, positioned at the row's enter coordinates.
  // `cardOver` tracks whether the pointer sits on the card itself (it is
  // interactive — actions are real), and every close is a cancelable grace
  // timer so row → card → row walks never blink.
  const [hover, setHover] = useState<RowHover | null>(null);
  const railRef = useRef<HTMLElement | null>(null);
  const cardOverRef = useRef(false);
  const closeTimer = useRef(0);
  // Newest card node: the keyboard keep-alive test needs it RIGHT NOW when a
  // row's blur fires (Tab from a row into the card's own action buttons must
  // close nothing), and the WeakSet inside cardNode keeps the listener wiring
  // idempotent while this ref tracks the freshest node identity.
  const cardElRef = useRef<HTMLDivElement | null>(null);

  const cancelClose = () => {
    if (closeTimer.current) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = 0;
    }
  };
  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = window.setTimeout(() => {
      closeTimer.current = 0;
      if (!cardOverRef.current) setHover(null);
    }, LEAVE_GRACE_MS);
  };
  // Row blur: when focus moved INTO the card (Tab onto one of its action
  // buttons) the card must stay open — cancel any armed close timer and do
  // not re-arm. Any other blur closes on the usual leave-grace.
  const blurKeepOrClose = (e: FocusEvent<HTMLButtonElement>) => {
    const el = cardElRef.current;
    if (el && e.relatedTarget instanceof Node && el.contains(e.relatedTarget)) {
      cancelClose();
      return;
    }
    scheduleClose();
  };
  const clearCard = () => {
    cancelClose();
    cardOverRef.current = false;
    setHover(null);
  };

  // A fresh `largest` prop (rescan, remount) invalidates any open card: an
  // id could survive a rescan while its bytes changed, so the only honest
  // reset is to close and let the next enter re-open with fresh data.
  useEffect(() => {
    clearCard();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [largest]);
  useEffect(() => () => window.clearTimeout(closeTimer.current), []);

  const detail = selection?.detail ?? null;
  const multi = (selection?.ids.length ?? 0) > 1;
  const total = totalScan || 1;
  const catTotal = categories.reduce((a, b) => a + b, 0);
  const catSum = catTotal || 1; // shares stay defined while the scan is empty

  // Demo/browser mode: reveal + open resolve false (ipc.ts guards), so the
  // card omits its action row — the card never shows dead buttons.
  const realShell = demoState() === null;

  const needle = query.trim().toLowerCase();
  const shown = needle
    ? largest.filter((f) => f.name.toLowerCase().includes(needle))
    : largest;

  // True rank in the volume-wide list (1-based, zero-padded for the mono
  // column). Anchored to `largest`, not `shown`, so a filtered view keeps
  // each row's honest rank instead of renumbering it into a lie.
  const rankOf = useMemo(() => {
    const m = new Map<number, number>();
    largest.forEach((f, i) => m.set(f.id, i + 1));
    return m;
  }, [largest]);

  // The hovered id must still be a visible row (filter changes, list swaps,
  // a loading pass that replaced the list with skeletons). Dropped = closed;
  // nothing stale is ever displayed.
  const hoverLive =
    !loading && hover && shown.some((s) => s.id === hover.f.id) ? hover : null;

  const buildCard = (f: LargestFile): HoverCardData => ({
    name: f.name,
    size: formatBytes(f.size),
    share: totalScan > 0 ? `${formatPercent(f.size / total)} of volume` : undefined,
    cat: { label: catLabel(f.cat), color: categoryStyle(f.cat, theme).css },
    path: f.path,
    actions: realShell
      ? [
          {
            label: "Reveal in Explorer",
            icon: <FolderOpen size={14} />,
            onRun: () => {
              void revealInExplorer(f.path);
            },
          },
          {
            label: "Open",
            icon: <SquareArrowOutUpRight size={14} />,
            onRun: () => {
              void openWithSystem(f.path);
            },
          },
        ]
      : undefined,
  });

  // Attach pointer watchers straight onto the card node via the shared
  // component's ref. HoverCard re-creates its own ref callback inline, so
  // React detaches/re-attaches this one on each of the card's re-renders —
  // the WeakSet makes the wiring idempotent (attach once per node), and the
  // stable useCallback keeps the closure pinned for the rail's lifetime.
  // (The closure touches only refs + setState, both stable.)
  const cardBoundRef = useRef<WeakSet<HTMLDivElement> | null>(null);
  const cardNode = useCallback((node: HTMLDivElement | null) => {
    cardElRef.current = node; // detach clears it; re-attach repins the node
    if (!node) return; // detach cycle — the WeakSet already owns this node
    if (cardBoundRef.current === null) cardBoundRef.current = new WeakSet();
    if (cardBoundRef.current.has(node)) return;
    cardBoundRef.current.add(node);
    node.addEventListener("pointerenter", () => {
      cardOverRef.current = true;
      cancelClose();
    });
    node.addEventListener("pointerleave", () => {
      cardOverRef.current = false;
      scheduleClose();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Row-anchored, not pointer-anchored: the card parks just below the row
  // the pointer entered and does not move until another row is entered —
  // an instrument that holds still reads calmer than one that chases the
  // cursor, and it can never straddle the pointer's own hit area.
  const enterRow = (f: LargestFile, e: PointerEvent<HTMLButtonElement>) => {
    if (e.pointerType === "touch") return; // touch taps select; no ghost card
    const rail = railRef.current;
    if (!rail) return;
    const rr = rail.getBoundingClientRect();
    const row = e.currentTarget.getBoundingClientRect();
    cancelClose();
    setHover((cur) =>
      cur && cur.f.id === f.id
        ? cur
        : { f, x: row.left - rr.left + 8, y: row.bottom - rr.top + 4 },
    );
  };

  // Keyboard parity: focusing a row opens the card at the row's own rect —
  // but never re-anchors a card already open for that same row (a click on a
  // hovered row would move it mid-hover).
  const focusRow = (f: LargestFile, e: FocusEvent<HTMLButtonElement>) => {
    const rail = railRef.current;
    if (!rail) return;
    const rr = rail.getBoundingClientRect();
    const row = e.currentTarget.getBoundingClientRect();
    cancelClose();
    setHover((cur) =>
      cur && cur.f.id === f.id
        ? cur
        : { f, x: row.left - rr.left + 8, y: row.top - rr.top },
    );
  };

  // Hero figure for the readout cell. With no selection the figure is the
  // current folder's own size; while a folder read is in flight that number
  // is unknown (stale or not yet reported), so it prints the dash.
  const hero = detail
    ? heroOf(multi && selection ? selection.total : detail.size)
    : heroOf(loading ? Number.NaN : current.size);

  return (
    <aside className="rail" ref={railRef}>
      {/* ------------------------------------------------ selection readout
          A quiet sentence-case line names the readout mode (Selection /
          N items selected / Current folder); the hero is the display step
          (`stat is-hero`, 34px tabular mono) because this number is the
          rail's reason to exist, and it carries the multi-total in
          selection mode. Sub-line words are Geist (plural() keeps "1 file"
          honest), its figures mono tabular, the category is a chip (color
          dot + word — the Two-Channel Rule) and the path a single mono
          line, ellipsized tail-true. A failure sits here as a quiet word
          plus the engine's message. Nothing is measured nowhere. */}
      <section className="rail-sec">
        {error && (
          <div className="rail-error">
            <StatusWord tone="critical">error</StatusWord>
            <p className="rail-error-msg">{error}</p>
          </div>
        )}
        {detail ? (
          <div className="stat is-hero">
            <p className="sel-label">
              {multi
                ? `${formatCount(selection!.ids.length)} ${plural(
                    selection!.ids.length,
                    "item",
                  )} selected`
                : "Selection"}
            </p>
            <p className="sel-name">{detail.name}</p>
            <p className="stat-v">
              {hero.num}
              {hero.unit && <span className="u">{hero.unit}</span>}
            </p>
            {multi ? (
              <p className="sel-sub">
                <span className="num">{formatCount(selection!.ids.length)}</span>{" "}
                {plural(selection!.ids.length, "item")}
                {totalScan > 0 && (
                  <>
                    {" · "}
                    <span className="num">{formatPercent(selection!.total / total)}</span> of volume
                  </>
                )}
              </p>
            ) : (
              <>
                {(totalScan > 0 || detail.isDir) && (
                  <p className="sel-sub">
                    {totalScan > 0 && (
                      <span className="num">{formatPercent(detail.size / total)}</span>
                    )}
                    {totalScan > 0 && " of volume"}
                    {detail.isDir && (
                      <>
                        {totalScan > 0 && " · "}
                        <span className="num">{formatCount(detail.fileCount)}</span>{" "}
                        {plural(detail.fileCount, "file")}
                        {" · "}
                        <span className="num">{formatCount(detail.kidCount)}</span>{" "}
                        {plural(detail.kidCount, "item")}
                      </>
                    )}
                  </p>
                )}
                <div className="sel-meta">
                  <span className="chip">
                    <span
                      aria-hidden="true"
                      className="chip-dot"
                      style={{ background: categoryStyle(detail.cat, theme).css }}
                    />
                    {catLabel(detail.cat)}
                  </span>
                </div>
                <p className="sel-path num">{ellipsizePath(detail.path, 44)}</p>
                <div className="sel-actions">
                  <IconButton
                    label="Reveal in Explorer"
                    onClick={() => onReveal(detail.id)}
                  >
                    <FolderOpen size={14} />
                  </IconButton>
                  <IconButton label="Copy path" onClick={() => onCopyPath(detail.id)}>
                    <Copy size={14} />
                  </IconButton>
                  {realShell && !detail.isDir && (
                    <IconButton label="Open file" onClick={() => onOpen(detail.id)}>
                      <SquareArrowOutUpRight size={14} />
                    </IconButton>
                  )}
                </div>
              </>
            )}
          </div>
        ) : (
          <div className="stat is-hero">
            <p className="sel-label">Current folder</p>
            <p className="sel-name">{current.name}</p>
            <p className="stat-v">
              {hero.num}
              {hero.unit && <span className="u">{hero.unit}</span>}
            </p>
            {loading ? (
              <p className="sel-sub">Reading folder…</p>
            ) : (
              <p className="sel-sub">
                <span className="num">{formatCount(current.items)}</span>{" "}
                {plural(current.items, "item")}
                {totalScan > 0 && (
                  <>
                    {" · "}
                    <span className="num">{formatPercent(current.size / total)}</span> of volume
                  </>
                )}
              </p>
            )}
            <p className="sel-path num">{ellipsizePath(current.path, 44)}</p>
          </div>
        )}
      </section>

      {/* ------------------------------------------------ composition
          One row per category: chip-style swatch, sentence-case label, and
          the measured size + share as mono figures up right; a 4px share
          meter on a dot-matrix bed sits under each row so proportions read
          without holding every number in mind — real shares only. Rows are
          real toggle buttons (aria-pressed); a click lifts to the treemap
          filter. A filtered category wears the tex-hatch over its swatch
          and drops to ink-3 (the Two-Channel Rule: texture + label, never
          hue alone). Shares divide by the sum of ALL categories — the
          legend reports the volume, not the visible subset. While the
          folder read is in flight the rows are quiet skeletons: no stale
          totals, no shimmer. */}
      <section className="rail-sec">
        <h3 className="rail-title">
          Categories
          {!loading && categories.length > 0 && (
            <span className="count num">{formatBytes(catTotal)}</span>
          )}
        </h3>
        {loading ? (
          <>
            <VisuallyHidden>Loading category totals…</VisuallyHidden>
            <div className="rail-skel skel-cats" aria-hidden="true">
              {Array.from({ length: 8 }, (_, i) => (
                <div className="sk-row" key={i}>
                  <span className="sk-line">
                    <span className="sk sk-sw" />
                    <span className="sk sk-nm" />
                    <span className="sk sk-vl" />
                  </span>
                  <span className="sk sk-bar" />
                </div>
              ))}
            </div>
          </>
        ) : categories.length === 0 ? (
          <EmptyState title="No scan data yet." />
        ) : (
          categories.map((bytes, cat) => {
            const off = hiddenCats.has(cat);
            const cs = categoryStyle(cat, theme);
            const share = (bytes / catSum) * 100;
            return (
              <button
                key={cat}
                type="button"
                className={off ? "cat-row is-off" : "cat-row"}
                onClick={() => onToggleCat(cat)}
                aria-pressed={!off}
              >
                <span className="cat-main">
                  <span
                    aria-hidden="true"
                    className={cs.hatch ? "cat-swatch is-hatch" : "cat-swatch"}
                    style={{ background: cs.css }}
                  />
                  <span className="cat-lbl">{CATEGORY_LABELS[cat]}</span>
                  <span className="cat-size num">{formatBytes(bytes)}</span>
                  <span className="cat-pct num">{formatPercent(bytes / catSum)}</span>
                </span>
                <span aria-hidden="true" className="cat-bar tex-dots">
                  <span
                    className="cat-bar-fill"
                    style={{
                      width: `${share}%`,
                      minWidth: bytes > 0 ? 2 : undefined,
                      background: cs.css,
                    }}
                  />
                </span>
              </button>
            );
          })
        )}
      </section>

      {/* ------------------------------------------------ largest files
          Volume-wide outliers the current level may not contain, so the
          list keeps its own filter (case-insensitive name match, local
          state) with the "visible of total" count in the title. Rank rows:
          mono rank index (true rank, held across filtering), tail-true
          name, mono size on one shared right axis; rows 12.5/16 at 28px.
          Hover opens the interactive HoverCard for THAT row at the enter
          coordinates — its category chip, full path and two real actions
          (reveal / open; omitted in demo where they resolve false). The
          list alone scrolls; a scroll under an open card closes it, so the
          readout can never sit over a row it does not describe. */}
      <section className="rail-sec rail-files">
        <h3 className="rail-title">
          Largest files
          {!loading && largest.length > 0 && (
            <span className="count">
              <span className="num">{formatCount(shown.length)}</span> of{" "}
              <span className="num">{formatCount(largest.length)}</span>
            </span>
          )}
        </h3>
        {!loading && largest.length > 0 && (
          <div className="rail-search">
            <Search size={12.5} aria-hidden="true" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key !== "Escape") return;
                // Two-stage escape: first press clears the filter and holds
                // focus for the next keystroke (stopPropagation keeps the
                // app-level escape from firing); second press blurs.
                if (query) {
                  e.preventDefault();
                  e.stopPropagation();
                  setQuery("");
                } else {
                  e.currentTarget.blur();
                }
              }}
              placeholder="Filter largest files"
              aria-label="Filter largest files"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        )}
        <div className="rail-list" onScroll={clearCard}>
          {loading ? (
            <>
              <VisuallyHidden>Loading largest files…</VisuallyHidden>
              <div className="rail-skel skel-files" aria-hidden="true">
                {Array.from({ length: 7 }, (_, i) => (
                  <div className="sk-row" key={i}>
                    <span className="sk sk-rk" />
                    <span className="sk sk-nm" />
                    <span className="sk sk-vl" />
                  </div>
                ))}
              </div>
            </>
          ) : largest.length === 0 ? (
            <EmptyState
              title="Nothing scanned yet."
              hint="Largest files appear after a scan completes."
            />
          ) : shown.length === 0 ? (
            <EmptyState
              title="No matches."
              hint={
                <>
                  Nothing in the largest files matches “{query.trim()}”.
                </>
              }
            />
          ) : (
            shown.map((f) => {
              const rank = rankOf.get(f.id) ?? 0;
              const active = f.id === selectedId || f.id === currentId;
              return (
                <button
                  key={f.id}
                  type="button"
                  className={active ? "file-row is-sel" : "file-row"}
                  onClick={() => onPickLargest(f.id)}
                  onPointerEnter={(e) => enterRow(f, e)}
                  onPointerLeave={scheduleClose}
                  onFocus={(e) => focusRow(f, e)}
                  onBlur={blurKeepOrClose}
                >
                  {/* Visual rank only: AT already reads list order, and a
                      bare "01" would announce as noise. */}
                  <span aria-hidden="true" className="file-rank num">
                    {String(rank).padStart(2, "0")}
                  </span>
                  <span className="file-nm">{tailName(f.name)}</span>
                  <span className="file-sz num">{formatBytes(f.size)}</span>
                </button>
              );
            })
          )}
        </div>
      </section>

      {/* The rail-side hover card: positioned in rail coordinates (the rail
          is the offsetParent), floating layer rules apply — menu shadow only
          while visible. */}
      <HoverCard
        card={hoverLive ? buildCard(hoverLive.f) : null}
        x={hoverLive?.x ?? 0}
        y={hoverLive?.y ?? 0}
        interactive={!!hoverLive}
        ref={cardNode}
      />
    </aside>
  );
}
