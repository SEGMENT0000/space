// Inspector rail — the instrument panel's right column, three hairline-
// stacked cells: selection readout, the volume's composition (one row per
// category; a row click filters the map), and the scan's largest files
// under a local name filter.
//
// Layout law: exactly ONE scroll region — the file list (flex:1, min 0).
// The readout and composition are fixed sections; the rail itself never
// scrolls, so the hero figure and the toggles stay in view on any window.
//
// Type register (anti-slop): labels are sentence-case Geist — section
// titles 12/600 ink-2, the mode line a quiet 12px 500 ink-2. Mono tabular
// (.num) is reserved for FIGURES (sizes, counts, percentages, the hero
// value) and PATHS. Never uppercase-mono on a label.
//
// Hover truth: each file row opens the shared HoverCard (ui.tsx) with THAT
// row's payload — anchored at enter coordinates, interactive, kept alive by
// a 150ms leave-grace so the card's real actions stay clickable. The wrong-
// row native `title` tooltips are gone; no native tooltip exists here.

import {
  Copy,
  Database,
  File,
  FileArchive,
  FileCode,
  FileImage,
  FileMusic,
  FileSpreadsheet,
  FileText,
  Film,
  FolderOpen,
  Package,
  Search,
  SquareArrowOutUpRight,
  type LucideIcon,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { CATEGORY_LABELS, categoryStyle } from "../lib/colors";
import {
  ellipsizePath,
  formatBytes,
  formatCount,
  formatPercent,
  splitBytes,
} from "../lib/format";
import { demoState, openWithSystem, revealInExplorer } from "../lib/ipc";
import type { LargestFile } from "../lib/types";
import { HoverCard, type HoverCardData } from "./ui";

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

/** Extension → glyph table: shape reads faster than reading the name, and a
 *  generic "file" icon is only the fallback, never the only voice. Only lucide
 *  names confirmed present in the installed build. */
const EXT_ICONS: [exts: string, icon: LucideIcon][] = [
  ["png jpg jpeg gif webp bmp svg ico heic tif tiff avif", FileImage],
  ["mp4 mkv avi mov webm wmv flv m4v mpg mpeg", Film],
  ["mp3 wav flac aac ogg m4a wma opus mid midi", FileMusic],
  ["zip rar 7z tar gz bz2 xz iso cab", FileArchive],
  [
    "js mjs cjs ts jsx tsx py java c cpp h hpp cs go rs rb php swift kt html htm css scss vue sql json xml",
    FileCode,
  ],
  ["pdf doc docx txt md rtf odt log", FileText],
  ["xls xlsx csv tsv ods", FileSpreadsheet],
  ["sqlite sqlite3 db db3 accdb", Database],
  ["exe msi msix appx dll so dylib", Package],
];

function fileIcon(name: string): LucideIcon {
  const i = name.lastIndexOf(".");
  const ext = i > 0 ? name.slice(i + 1).toLowerCase() : "";
  for (const [exts, icon] of EXT_ICONS)
    if (` ${exts} `.includes(` ${ext} `)) return icon;
  return File;
}

function catLabel(cat: number): string {
  return CATEGORY_LABELS[cat] ?? CATEGORY_LABELS[CATEGORY_LABELS.length - 1];
}

/** Hero pair: the value in display-size mono, the unit in the small `.u`.
 *  A zero/unknown figure reads as an honest "0 B" — never a dash pretending
 *  to be data this instrument did not measure. */
function heroOf(n: number): { num: string; unit: string } {
  if (!Number.isFinite(n) || n <= 0) return { num: "0", unit: "B" };
  return splitBytes(n);
}

/** Selection-readout icon button: aria-labelled, no native tooltip — the
 *  button's icon + word-level access (keyboard focus ring) carries it. */
function RailIconAction({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button type="button" className="icon-btn" aria-label={label} onClick={onClick}>
      {children}
    </button>
  );
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

  // The hovered id must still be a visible row (filter changes, list swaps).
  // Dropped = closed; nothing stale is ever displayed.
  const hoverLive =
    hover && shown.some((s) => s.id === hover.f.id) ? hover : null;

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

  return (
    <aside className="rail" ref={railRef}>
      {/* ------------------------------------------------ selection readout
          A quiet sentence-case line names the readout mode (Selection /
          N items selected / Current folder); the hero is the display step
          (`stat is-hero`, 34px) because this number is the rail's reason to
          exist, and it carries the multi-total in selection mode. Sub-line
          words are Geist, its figures mono tabular, the category is a chip
          (color dot + word — the Two-Channel Rule) and the path a single
          mono line. Nothing here is measured nowhere. */}
      <section className="rail-sec">
        {detail ? (
          <div className="stat is-hero">
            <p className="sel-label">
              {multi
                ? `${formatCount(selection!.ids.length)} items selected`
                : "Selection"}
            </p>
            <p className="sel-name">{detail.name}</p>
            <p className="stat-v">
              {heroOf(multi ? selection!.total : detail.size).num}
              <span className="u">{heroOf(multi ? selection!.total : detail.size).unit}</span>
            </p>
            {multi ? (
              <p className="sel-sub">
                <span className="num">{formatCount(selection!.ids.length)}</span> items
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
                        <span className="num">{formatCount(detail.fileCount)}</span> files
                        {" · "}
                        <span className="num">{formatCount(detail.kidCount)}</span> items
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
                  <RailIconAction
                    label="Reveal in Explorer"
                    onClick={() => onReveal(detail.id)}
                  >
                    <FolderOpen size={14} />
                  </RailIconAction>
                  <RailIconAction label="Copy path" onClick={() => onCopyPath(detail.id)}>
                    <Copy size={14} />
                  </RailIconAction>
                  {realShell && !detail.isDir && (
                    <RailIconAction label="Open file" onClick={() => onOpen(detail.id)}>
                      <SquareArrowOutUpRight size={14} />
                    </RailIconAction>
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
              {heroOf(current.size).num}
              <span className="u">{heroOf(current.size).unit}</span>
            </p>
            <p className="sel-sub">
              <span className="num">{formatCount(current.items)}</span> items
              {totalScan > 0 && (
                <>
                  {" · "}
                  <span className="num">{formatPercent(current.size / total)}</span> of volume
                </>
              )}
            </p>
            <p className="sel-path num">{ellipsizePath(current.path, 44)}</p>
          </div>
        )}
      </section>

      {/* ------------------------------------------------ composition
          One row per category: chip-style swatch, sentence-case label, and
          the measured size + share as mono figures up right. A 4px share
          bar sits on a dot-matrix bed under each row so proportions read
          without holding every number in mind — real shares only. Row click
          lifts to the treemap filter; a filtered category wears the tex-
          hatch over its swatch and drops to ink-3 (the Two-Channel Rule:
          texture + label, never hue alone). Shares divide by the sum of ALL
          categories — the legend reports the volume, not the visible subset. */}
      <section className="rail-sec">
        <h3 className="rail-title">
          Categories
          {categories.length > 0 && (
            <span className="count num">{formatBytes(catTotal)}</span>
          )}
        </h3>
        {categories.length === 0 && (
          <p className="rail-empty">No scan data yet.</p>
        )}
        {categories.map((bytes, cat) => {
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
        })}
      </section>

      {/* ------------------------------------------------ largest files
          Volume-wide outliers the current level may not contain, so the
          list keeps its own filter (case-insensitive name match, local
          state) with the "visible of total" count in the title. Rows are
          28px: extension glyph, ellipsized name, mono size on one shared
          right axis. Hover opens the interactive HoverCard for THAT row at
          the enter coordinates — its category chip, full path and two real
          actions (reveal / open; omitted in demo where they resolve false).
          The list alone scrolls; a scroll under an open card closes it, so
          the readout can never sit over a row it does not describe. */}
      <section className="rail-sec rail-files">
        <h3 className="rail-title">
          Largest files
          {largest.length > 0 && (
            <span className="count">
              <span className="num">{formatCount(shown.length)}</span> of{" "}
              <span className="num">{formatCount(largest.length)}</span>
            </span>
          )}
        </h3>
        {largest.length > 0 && (
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
          {largest.length === 0 && (
            <p className="rail-empty">Nothing scanned yet.</p>
          )}
          {largest.length > 0 && shown.length === 0 && (
            <p className="rail-empty">No matches.</p>
          )}
          {shown.map((f) => {
            const Ic = fileIcon(f.name);
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
                <span aria-hidden="true" className="file-ico">
                  <Ic size={13} />
                </span>
                <span className="file-nm">{f.name}</span>
                <span className="file-sz num">{formatBytes(f.size)}</span>
              </button>
            );
          })}
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
