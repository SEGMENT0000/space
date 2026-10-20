// App shell: screen routing (volumes → scan → explorer), theme, file actions
// (reveal / copy path / open), drag-and-drop scan targets, the command
// palette (Ctrl K), the global shortcut registry (ui.tsx chords dispatched
// after App's built-ins), and the two-slot shell toast stack (sticky error +
// transient status via useShell().status). No logo tile — the wordmark is
// set in type, and appears only in the boot state and on the volumes screen.
//
// Boot is a *rendered* state, not a blank frame: a dead div reads as a hang,
// so it carries the wordmark plus one honest status line (no spinner — we
// don't know anything to spin about yet). In the palette every row wears a
// glyph because a command list scans faster by shape than by sentence, and
// jump rows print their real byte size so the list doubles as a ranked
// readout. All of it is presentation: routing and scan wiring are untouched
// here.

import { isTauri } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  ArrowDownWideNarrow,
  ArrowLeft,
  ArrowRight,
  CaseSensitive,
  File as FileIcon,
  Files,
  Folder,
  HardDrive,
  Maximize2,
  Moon,
  PanelRight,
  RefreshCw,
  Search,
  Sun,
  ZoomIn,
  ZoomOut,
  type LucideIcon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DrivesView } from "./components/DrivesView";
// errors.css rides in with this module (index.css is the lead's manifest —
// untouched); notifyError/subscribeError are the single error funnel.
import { ErrorBoundary, notifyError, subscribeError } from "./components/ErrorBoundary";
import { ExplorerView, type ExplorerControl, type FileApi } from "./components/ExplorerView";
import { ScanView } from "./components/ScanView";
import {
  ShellContext,
  Toast,
  dispatchShortcuts,
  registerShortcut,
  type ShellApi,
  type StatusOptions,
  type ThemeName,
} from "./components/ui";
import { formatBytes } from "./lib/format";
import { cancelScan, enableDemo, onScanDone, onScanError, onScanProgress, startScan } from "./lib/ipc";
import type { ProgressPayload } from "./lib/types";

type Screen = "boot" | "drives" | "scan" | "explorer";

interface ScanInfo {
  root: string;
  rootId: number;
  totalBytes: number;
  totalFiles: number;
}

/**
 * Theme resolution — ONE source of truth, shared by App's state initializer
 * and main.tsx's pre-paint hook (which sets <html data-theme> before React
 * mounts so a dark-first window never flashes light). Precedence: the
 * ?theme= deep link (review harness — explicit beats stored) > the persisted
 * choice > dark (the world is dark-first). Never throws: storage can be
 * disabled, and a boot path must not be where the app dies.
 */
export function resolveTheme(): ThemeName {
  const q =
    typeof location !== "undefined"
      ? new URLSearchParams(location.search).get("theme")
      : null;
  if (q === "light" || q === "dark") return q;
  try {
    const saved = localStorage.getItem("space.theme");
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    /* storage unavailable — fall through to dark */
  }
  return "dark";
}

async function copyToClipboard(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
}

const fileApi: FileApi = {
  reveal: (path) => revealItemInDir(path),
  openPath: (path) => openPath(path),
  copyPath: (path) => copyToClipboard(path),
};

// Dev/screenshot deep-link: ?zoom=N zooms the map N× past fit once the
// explorer control is ready — the way to reproduce deep-zoom states
// (plate qualification, zoom-ceiling behaviour) without driving a
// pointer, e.g. http://localhost:1420/?zoom=8.
//
// Computed once at module scope ON PURPOSE: ExplorerView re-registers its
// control in an effect keyed on the onReady prop's identity, and an inline
// `const zoomParam = …` inside the body is recomputed (same value, new
// evaluation) every render — the stable piece of the handle is the
// constant itself, so it lives above the component.
const zoomParam = (() => {
  if (typeof location === "undefined") return 0;
  const n = Number(new URLSearchParams(location.search).get("zoom") ?? 0);
  return Number.isFinite(n) ? n : 0;
})();

// The default export is the OUTERMOST error net. WHY two boundaries: a
// throw in AppShell's own body (state initializers, hook wiring) happens
// above any boundary it renders itself — React would unmount the whole tree
// and white-screen the window, and main.tsx renders nothing but the root
// (it pre-paints the theme, then calls createRoot — no fallback to catch).
// This outer one degrades such a throw to the quiet panel; the inner one
// (around .app) catches screen-level throws while App's state, window
// listeners and toast subscription stay alive. onError → notifyError keeps
// both nets on the one funnel (App's subscriber renders the toast; if App
// itself is the casualty there is no subscriber left — the panel below IS
// the message).
export default function App() {
  return (
    <ErrorBoundary onError={(err) => notifyError(err.message)}>
      <AppShell />
    </ErrorBoundary>
  );
}

function AppShell() {
  const [demo, setDemo] = useState(false);
  const [screen, setScreen] = useState<Screen>("boot");
  // resolved by resolveTheme() (deep link > stored > dark) — main.tsx has
  // already painted the same value on <html data-theme>, so this state and
  // the first paint can never disagree
  const [theme, setTheme] = useState<ThemeName>(resolveTheme);
  const [scanRoot, setScanRoot] = useState("");
  const [scanInfo, setScanInfo] = useState<ScanInfo | null>(null);
  const [progress, setProgress] = useState<ProgressPayload | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [palOpen, setPalOpen] = useState(false);
  // capacity of the volume being scanned (0 = unknown / demo); the basis for
  // every "of volume" percentage in the explorer
  const [volumeTotal, setVolumeTotal] = useState(0);

  const ctlRef = useRef<ExplorerControl | null>(null);

  const zoomApplied = useRef(false);

  // ------------------------------------------------------------ runtime toast
  // Quiet runtime-error toast (FLEET_SPEC §D): every error source — the
  // ErrorBoundary onError, window "error", unhandledrejection — funnels
  // through notifyError → subscribeError → this state, so there is exactly
  // one voice and one place a failure can surface. WHY a ref-held ring of
  // the last 5 distinct messages: identical repeated throws collapse into
  // the line already on screen (dedupe), the cap bounds the memory for the
  // session, and — like the boot effect's `dead` flag — a ref needs no
  // cleanup path of its own; it dies with the component.
  const [errToast, setErrToast] = useState<string | null>(null);
  const errSeenRef = useRef<string[]>([]);
  const pushErrToast = useCallback((raw: string) => {
    // truncate before dedupe so a message whose tail varies past 140 chars
    // still counts as the same error
    const msg = raw.length > 140 ? `${raw.slice(0, 139)}…` : raw;
    const seen = errSeenRef.current;
    if (seen.includes(msg)) return;
    seen.push(msg);
    if (seen.length > 5) seen.shift();
    setErrToast(msg);
  }, []);

  // ------------------------------------------------------------ status slot
  // The quiet non-error voice every screen speaks through
  // useShell().status(msg) ("Path copied", "Rescan queued…"). WHY a second
  // slot instead of reusing the error funnel: errors are sticky — manual
  // dismiss only, an unacknowledged failure never vanishes on a timer —
  // while status notes are transient, and a transient note must never be
  // able to displace a sticky error. Both render in the .toast-stack below,
  // error nearest the bottom (the slot the old error toast occupied).
  // WHY App owns the timer, not Toast's dismissAfter: progress ticks at
  // ~120ms during a scan re-render App, and a timer keyed on a fresh
  // onDismiss closure would reset forever and never fire. An id-guarded
  // timeout in this ref only clears the status it belongs to.
  const [statusToast, setStatusToast] = useState<{
    id: number;
    msg: string;
    key?: string;
    tone?: StatusOptions["tone"];
  } | null>(null);
  const statusSeq = useRef(0);
  const statusTimer = useRef(0);
  const status = useCallback((message: string, opts?: StatusOptions) => {
    const id = ++statusSeq.current;
    setStatusToast({ id, msg: message, key: opts?.key, tone: opts?.tone });
    window.clearTimeout(statusTimer.current);
    statusTimer.current = window.setTimeout(() => {
      setStatusToast((s) => (s && s.id === id ? null : s));
    }, opts?.ms ?? 2600);
  }, []);
  useEffect(() => () => window.clearTimeout(statusTimer.current), []);

  // ------------------------------------------------------------ shell context
  // Theme + status() + the shortcut registry, handed to every screen via
  // context so components can stop threading parallel prop chains. WHY a
  // memo: the value object is a dependency of any consumer effect — an
  // inline literal re-ran e.g. ExplorerView's control registration on every
  // App render (same identity discipline as handleExplorerReady below).
  // registerShortcut is the module singleton in ui.tsx — stable forever.
  const toggleTheme = useCallback(
    () => setTheme((t) => (t === "dark" ? "light" : "dark")),
    [],
  );
  const shell = useMemo<ShellApi>(
    () => ({ theme, setTheme, toggleTheme, status, registerShortcut }),
    [theme, toggleTheme, status],
  );

  // ------------------------------------------------------------ boot

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem("space.theme", theme);
  }, [theme]);

  useEffect(() => {
    let un: UnlistenFn[] = [];
    let dead = false;

    void (async () => {
      let tauri = false;
      try {
        tauri = isTauri();
      } catch {
        tauri = false;
      }
      if (!tauri) {
        const d = enableDemo();
        setDemo(true);
        setScanInfo({
          root: d.rootPath,
          rootId: d.rootId,
          totalBytes: d.totalBytes,
          totalFiles: d.totalFiles,
        });
        // Dev/screenshot deep-links: ?screen=drives opens the volume picker
        // in demo mode instead of jumping straight into the explorer;
        // ?screen=scan parks on the scan instrument with a deterministic
        // synthetic progress ramp (hand-picked byte deltas, fixed fictional
        // paths, no randomness) so the redesigned screen can be reviewed and
        // screenshotted without a real multi-minute scan.
        //
        // WHY the ramp loops forever instead of ending: this harness never
        // emits a scan://done or scan://error payload (there is no scan), so
        // a ramp that stops leaves the meter quiet — ~2s later ScanView's
        // honesty check prints the stall line and the sweep parks, and QA
        // shots read as a dead scan. Auto-emitting a demo done would be
        // worse: it would yank the harness off the very screen ?screen=scan
        // exists to park on. Looping keeps counters, rate strip and walking
        // path live for as long as the reviewer looks — the stall line can
        // never appear, and Esc/Cancel still leaves through the normal
        // cancel beat to the volumes screen.
        const q =
          typeof location !== "undefined"
            ? new URLSearchParams(location.search)
            : null;
        const deep = q?.get("screen");
        if (deep === "scan") {
          setScanRoot("C:");
          setScreen("scan");
          // Fixed MiB-per-tick pattern so the rate sparkline has
          // honest-looking variation and the EMA settles mid-range.
          const shape = [
            14, 31, 18, 44, 26, 12, 38, 22, 48, 19, 27, 15, 35, 24, 41, 17,
          ];
          const walk = [
            "C:\\Windows\\WinSxS",
            "C:\\Users\\casey\\AppData\\Local\\Temp",
            "C:\\Program Files\\Common Files",
            "C:\\ProgramData\\Package Cache",
            "C:\\Windows\\System32",
            "C:\\Users\\casey\\Downloads",
          ];
          let step = 0;
          let bytes = 0;
          const tick = window.setInterval(() => {
            if (dead) {
              window.clearInterval(tick);
              return;
            }
            bytes += shape[step % shape.length] * 1048576;
            step += 1;
            setProgress({
              bytes,
              files: Math.round(bytes / 31200),
              dirs: Math.round(bytes / 3100000),
              errors: step >= 25 ? 2 : 0,
              current: walk[step % walk.length],
              elapsedMs: step * 140,
            });
            // NO terminal step: the ramp is the harness's terminal state —
            // it wraps (shape/walk are indexed modulo) and runs for the
            // session so the instrument never goes quiet (see the comment
            // above). The `dead` cleanup above is its only stop.
          }, 140);
          un.push(() => window.clearInterval(tick));
          return;
        }
        setScreen(deep === "drives" ? "drives" : "explorer");
        return;
      }
      if (dead) return;
      // Events subscribe before any scan can start: the ?scan deep link
      // below kicks work off after this await, so progress / done / error
      // can only ever land in handlers that already exist.
      un = await Promise.all([
        onScanProgress((p) => setProgress(p)),
        onScanDone((p) => {
          setScanInfo({
            root: p.root,
            rootId: p.rootId,
            totalBytes: p.totalBytes,
            totalFiles: p.totalFiles,
          });
          setScanError(null);
          setScreen("explorer");
        }),
        onScanError((e) => {
          setScanError(e.message);
          setScreen("scan");
        }),
      ]);
      // ?scan=<path>: boot straight into a scan, skipping the volumes
      // screen. The gate is the param's presence — never a dev flag — so
      // the packaged exe honors it too. Sanitized: absolute Windows paths
      // only (must start with a drive letter + backslash); anything else
      // quietly falls through to the volumes screen, same as no param.
      const scanParam =
        typeof location !== "undefined"
          ? new URLSearchParams(location.search).get("scan")
          : null;
      if (scanParam && /^[A-Za-z]:\\/.test(scanParam)) {
        setScanRoot(scanParam);
        setProgress(null);
        setScanError(null);
        // no picked volume capacity — the scan total carries "% of volume"
        setVolumeTotal(0);
        setScreen("scan");
        void startScan(scanParam).catch((e) => setScanError(String(e)));
      } else {
        setScreen("drives");
      }
    })();

    return () => {
      dead = true;
      for (const fn of un) fn();
    };
  }, []);

  // drag-drop a folder onto the window to scan it
  useEffect(() => {
    if (demo) return;
    let unlisten: UnlistenFn | undefined;
    try {
      void getCurrentWebview()
        .onDragDropEvent((e) => {
          if (e.payload.type === "drop" && e.payload.paths.length) {
            void pick(e.payload.paths[0]);
          }
        })
        .then((fn) => {
          unlisten = fn;
        })
        .catch(() => undefined);
    } catch {
      /* not running inside Tauri */
    }
    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [demo]);

  // ------------------------------------------------------------ actions

  const pick = useCallback((mount: string, total = 0) => {
    setScanRoot(mount);
    setProgress(null);
    setScanError(null);
    // capacity of the picked volume; dropped subfolders pass nothing and fall
    // back to the scan total for "% of volume"
    setVolumeTotal(total);
    // Demo has the finished dataset loaded at boot and no scan events to
    // receive — parking on the scan screen there would dead-end on zeros,
    // so it jumps straight to the explorer.
    if (demo) {
      setScreen("explorer");
      return;
    }
    setScreen("scan");
    void startScan(mount).catch((e) => setScanError(String(e)));
  }, [demo]);

  const rescan = useCallback(() => {
    if (demo) return;
    const root = scanInfo?.root ?? scanRoot;
    if (!root) return;
    setScanRoot(root);
    setProgress(null);
    setScanError(null);
    setScreen("scan");
    void startScan(root).catch((e) => setScanError(String(e)));
  }, [demo, scanInfo, scanRoot]);

  const cancel = useCallback(() => {
    // Best-effort fire-and-forget (unchanged semantics): a rejected
    // cancel keeps surfacing through the §D funnel — unhandledrejection →
    // quiet toast — instead of being swallowed in a catch here.
    void cancelScan();
    // THE CANCEL BEAT. ScanView disables its button and prints
    // "Cancelling…" in the SAME React batch as this call, so the old
    // synchronous setScreen unmounted that state before the frame ever
    // painted — the acknowledgment was invisible. Defer the swap by one
    // short window so the disabling state is actually shown for several
    // frames. WHY a timer, not a double rAF: rAF stops firing in a
    // backgrounded webview and would strand the window on the scan
    // screen; timers keep firing (throttled) and always land. WHY not
    // await cancelScan(): the demo branch resolves in a microtask — the
    // continuation could still coalesce into the same paint — and an
    // invoke that never answered would strand the swap entirely. The
    // functional guard makes the beat leave ONLY the scan screen: if
    // scan://done wins the race, the finished map stands and the beat
    // no-ops. Cancel path ONLY — onScanDone still swaps in its own
    // handler with zero added delay: a finished scan reaches the map
    // immediately (no fake delay on the success path).
    window.setTimeout(
      () => setScreen((s) => (s === "scan" ? "drives" : s)),
      220,
    );
  }, []);

  // ------------------------------------------------------------ explorer handle
  // A useCallback, not an inline arrow: ExplorerView registers/unregisters
  // its control in an effect keyed on the onReady prop's identity, so a
  // fresh closure on every App render (theme flip, palette open, toast)
  // churned that registration. This closes over only module-scope zoomParam
  // and refs, so its identity is stable for the whole session.
  const handleExplorerReady = useCallback(
    (ctl: ExplorerControl | null) => {
      ctlRef.current = ctl;
      if (ctl && !zoomApplied.current && zoomParam > 1) {
        zoomApplied.current = true;
        // Dev-only: at onReady no frame exists yet, and the first
        // data-arrival rebuild resets the viewport to fit
        // (ensureFrame) — an immediate zoomBy would be swallowed.
        // Apply after that rebuild has landed (its transition is
        // another 420ms; well inside screenshot budgets).
        window.setTimeout(() => ctl.zoomBy(zoomParam), 800);
      }
    },
    [zoomParam], // module-scope constant: stable, the effect never refires from App
  );

  // ------------------------------------------------------------ keys
  // One window listener, two tiers: App's built-ins claim their chords
  // first (the palette toggle must never be shadowable by a component
  // registration), then the shared registry in ui.tsx gets first refusal
  // on everything else. Escape closes the palette from anywhere — the
  // input's own Esc handler only covers focus-inside-the-input, so a Tab
  // into the palette list used to strand the overlay open.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Auto-repeat must not hammer a toggle: holding Ctrl K would flicker
      // the palette open/closed while the key bottoms out. (Explorer's own
      // listener is separate and keeps its repeat behavior — held
      // Backspace-to-go-up still repeats.)
      if (e.repeat) return;
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalOpen((v) => !v);
        return;
      }
      if (mod && e.key.toLowerCase() === "f" && screen === "explorer") {
        e.preventDefault();
        ctlRef.current?.focusSearch();
        return;
      }
      if (e.key === "Escape" && palOpen) {
        setPalOpen(false);
        return;
      }
      dispatchShortcuts(e);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [screen, palOpen]);

  // ------------------------------------------------------------ runtime errors
  // The window-level half of §D's error net: React's boundary only sees
  // render-phase throws, so everything else — event handlers, timers, IPC
  // callbacks, unhandled promise rejections — lands here and degrades to the
  // same quiet toast instead of a dead screen. WHY not preventDefault(): we
  // only observe; suppressing the event would hide errors from dev tooling
  // too. WHY the `dead` flag: same cleanup pattern as the boot effect — a
  // notify queued after unmount must not set state on a dead component.
  useEffect(() => {
    let dead = false;
    const unsub = subscribeError((m) => {
      if (!dead) pushErrToast(m);
    });
    const onWindowError = (e: ErrorEvent) => {
      notifyError(e.message || "unknown error");
    };
    const onRejection = (e: PromiseRejectionEvent) => {
      const r: unknown = e.reason;
      notifyError(
        r instanceof Error
          ? r.message
          : typeof r === "string"
            ? r
            : "unhandled promise rejection",
      );
    };
    // The renderer's rAF loop can't throw into React (it's outside the
    // render tree) — it reports via this event and stops itself; bridge it
    // into the same funnel as everything else.
    const onSpaceError = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      notifyError(typeof detail === "string" && detail ? detail : "render loop error");
    };
    window.addEventListener("error", onWindowError);
    window.addEventListener("unhandledrejection", onRejection);
    window.addEventListener("space:error", onSpaceError);
    return () => {
      dead = true;
      window.removeEventListener("error", onWindowError);
      window.removeEventListener("unhandledrejection", onRejection);
      window.removeEventListener("space:error", onSpaceError);
      unsub();
    };
  }, [pushErrToast]);

  // ------------------------------------------------------------ render

  return (
    <ShellContext.Provider value={shell}>
      {/* Inner net: a screen-level throw (drives / scan / explorer /
          palette) degrades to the quiet panel while App's state, window
          listeners and toast subscription survive — the shell never dies.
          onError → notifyError feeds the one funnel that renders the toast
          below, so boundary and window errors speak with one voice. */}
      <ErrorBoundary onError={(err) => notifyError(err.message)}>
        <div className="app">
          {screen === "boot" && (
            <div className="screen boot fade-in">
              <span className="boot-mark">space</span>
              <span className="micro">Reading volumes…</span>
            </div>
          )}
          {screen === "drives" && <DrivesView demo={demo} onPick={pick} />}
          {screen === "scan" && (
            <ScanView
              root={scanRoot}
              progress={progress}
              error={scanError}
              onCancel={cancel}
              onBack={() => setScreen("drives")}
            />
          )}
          {screen === "explorer" && scanInfo && (
            <ExplorerView
              rootId={scanInfo.rootId}
              rootPath={scanInfo.root}
              totalBytes={scanInfo.totalBytes}
              volumeTotal={volumeTotal}
              theme={theme}
              api={fileApi}
              onToggleTheme={toggleTheme}
              onRescan={demo ? () => setScreen("drives") : rescan}
              onShowDrives={() => setScreen("drives")}
              onOpenPalette={() => setPalOpen(true)}
              onReady={handleExplorerReady}
            />
          )}

          {palOpen && (
            <Palette
              explorer={screen === "explorer"}
              theme={theme}
              ctl={() => ctlRef.current}
              onClose={() => setPalOpen(false)}
              onToggleTheme={toggleTheme}
              onRescan={demo ? () => setScreen("drives") : rescan}
              onDrives={() => setScreen("drives")}
            />
          )}
        </div>
      </ErrorBoundary>
      {/* Sibling of the boundary, never its child: the stack must outlive a
          screen crash (the inner boundary swaps .app for its panel, but
          App's state stays mounted). Fixed bottom-right — clear of the
          bottom-left gesture strip and of the 30px statusbar; the error
          slot keeps the exact corner the old toast held, status notes
          stack above it. Manual dismiss for errors only: an
          unacknowledged error does not vanish on a timer. */}
      <div className="toast-stack">
        {statusToast && (
          <Toast
            key={statusToast.id}
            message={statusToast.msg}
            tone={statusToast.tone ?? "neutral"}
            labelKey={statusToast.key}
            onDismiss={() =>
              setStatusToast((s) => (s && s.id === statusToast.id ? null : s))
            }
          />
        )}
        {errToast !== null && (
          <Toast
            message={errToast}
            tone="error"
            labelKey="runtime error"
            onDismiss={() => setErrToast(null)}
          />
        )}
      </div>
    </ShellContext.Provider>
  );
}

// ------------------------------------------------------------ command palette

interface PalItem {
  id: string;
  group: string;
  label: string;
  sub?: string;
  /** real figure for jump rows (tabular, right column) — never a filler code */
  meta?: string;
  /** 14px row glyph, currentColor; shape lets the list scan before it reads */
  icon: LucideIcon;
  run: () => void;
}

function Palette({
  explorer,
  theme,
  ctl,
  onClose,
  onToggleTheme,
  onRescan,
  onDrives,
}: {
  explorer: boolean;
  theme: "light" | "dark";
  ctl: () => ExplorerControl | null;
  onClose: () => void;
  onToggleTheme: () => void;
  onRescan: () => void;
  onDrives: () => void;
}) {
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // the theme glyph shows where the toggle lands, not where it came from
  const commands: PalItem[] = [
    {
      id: "theme",
      group: "View",
      label: "Toggle light / dark",
      icon: theme === "dark" ? Sun : Moon,
      run: onToggleTheme,
    },
    { id: "drives", group: "Go", label: "Go to volumes", icon: HardDrive, run: onDrives },
    { id: "rescan", group: "Go", label: "Rescan this volume", icon: RefreshCw, run: onRescan },
  ];
  if (explorer && ctl()) {
    const c = ctl()!;
    commands.push(
      { id: "focus", group: "View", label: "Focus the filter field", icon: Search, run: () => c.focusSearch() },
      { id: "rail", group: "View", label: "Toggle the inspector rail", icon: PanelRight, run: () => c.toggleRail() },
      { id: "reset", group: "View", label: "Reset the map view", icon: Maximize2, run: () => c.resetView() },
      { id: "zin", group: "View", label: "Zoom in", icon: ZoomIn, run: () => c.zoomBy(1.4) },
      { id: "zout", group: "View", label: "Zoom out", icon: ZoomOut, run: () => c.zoomBy(1 / 1.4) },
      { id: "back", group: "Go", label: "Back a level", icon: ArrowLeft, run: () => c.back() },
      { id: "fwd", group: "Go", label: "Forward a level", icon: ArrowRight, run: () => c.forward() },
      { id: "s-size", group: "Sort", label: "Sort by size", icon: ArrowDownWideNarrow, run: () => c.setSort("size") },
      { id: "s-name", group: "Sort", label: "Sort by name", icon: CaseSensitive, run: () => c.setSort("name") },
      { id: "s-type", group: "Sort", label: "Sort by type", icon: Files, run: () => c.setSort("type") },
    );
  }

  const needle = q.trim().toLowerCase();
  const filteredCmds = needle ? commands.filter((c) => c.label.toLowerCase().includes(needle)) : commands;
  const jumps: PalItem[] =
    explorer && ctl() && needle
      ? ctl()!
          .searchEntries(q)
          .map((e) => ({
            id: `jump-${e.id}`,
            group: "Jump to",
            label: e.name,
            sub: e.isDir ? "folder" : undefined,
            // the size the scan actually measured — the reason to pick a jump
            meta: formatBytes(e.size),
            icon: e.isDir ? Folder : FileIcon,
            run: () => ctl()?.jumpTo(e.id),
          }))
      : [];

  const items = [...filteredCmds, ...jumps].slice(0, 30);
  const clampedActive = Math.min(active, Math.max(0, items.length - 1));

  const runItem = (item: PalItem) => {
    onClose();
    // let the overlay unmount before the action focuses something
    window.setTimeout(() => item.run(), 0);
  };

  let lastGroup = "";

  return (
    <div className="pal-scrim" onPointerDown={onClose}>
      <div className="pal float" onPointerDown={(e) => e.stopPropagation()}>
        <div className="pal-input-row">
          <Search size={16} aria-hidden="true" />
          <input
            ref={inputRef}
            value={q}
            placeholder="Type a command, or a file name to jump to…"
            spellCheck={false}
            onChange={(e) => {
              setQ(e.target.value);
              setActive(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") onClose();
              else if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(a + 1, items.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(a - 1, 0));
              } else if (e.key === "Enter" && items[clampedActive]) {
                runItem(items[clampedActive]);
              }
            }}
          />
          <span className="kbd">esc</span>
        </div>
        <div className="pal-list">
          {items.length === 0 && <p className="pal-empty micro">Nothing matches.</p>}
          {items.map((item, i) => {
            const Icon = item.icon;
            const head = item.group !== lastGroup ? item.group : null;
            lastGroup = item.group;
            return (
              <div key={item.id}>
                {head && <div className="pal-group">{head}</div>}
                <button
                  type="button"
                  className={i === clampedActive ? "pal-item is-active" : "pal-item"}
                  onPointerEnter={() => setActive(i)}
                  onClick={() => runItem(item)}
                >
                  <span className="ico">
                    <Icon size={14} aria-hidden="true" />
                  </span>
                  <span className="grow">{item.label}</span>
                  {item.sub && <span className="sub">{item.sub}</span>}
                  {item.meta && <span className="meta">{item.meta}</span>}
                </button>
              </div>
            );
          })}
        </div>
        <div className="pal-foot">
          <span>
            <span className="kbd">Ctrl K</span> palette
          </span>
          <span>
            <span className="kbd">Ctrl F</span> filter
          </span>
          <span>
            <span className="kbd">⌫</span> up
          </span>
          <span>
            <span className="kbd">Alt+←→</span> history
          </span>
          <span>
            <span className="kbd">Ctrl 0</span> reset view
          </span>
          <span>
            <span className="kbd">+/−</span> zoom
          </span>
        </div>
      </div>
    </div>
  );
}
