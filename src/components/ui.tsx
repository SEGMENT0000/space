// ui — the shared primitive layer every screen imports. STABLE API: this file
// is consumed concurrently by Drives/Scan/Explorer/Rail agents, so existing
// names keep byte-stable signatures and new names are added alongside, never
// instead. Both names render the same DOM — a consumer may migrate at any time.
//
//   controls   Button · IconButton · SegmentedControl (alias `Segmented`) ·
//              Tooltip · KeyHint
//   readouts   Meter (alias `CapacityBar`) · StatusWord · Spinner · VisuallyHidden
//   surfaces   EmptyState · Toast
//   shell      ShellContext / useShell / useShortcut — theme, status() toast,
//              global shortcut registry (provided by App.tsx)
//   legacy     Btn · IconBtn · HoverCard (+ HoverCardData / HoverAction types)
//
// A11y floor (checked, not aspirational): real <button> elements; required
// accessible names (IconButton.label, Meter.label); the global :focus-visible
// ring (corrected per control in shell.css so radius never morphs on focus);
// SegmentedControl is a real tablist with roving tabindex + arrow keys; the
// Tooltip rides mouse AND keyboard focus, never steals focus, and is
// pointer-events:none so it can never block the content it describes;
// reduced-motion clamps (primitives.css) freeze every entrance and the spin.
// Contrast: text tones below are the token ladder on --bg-elevated —
// ink 17.0:1 light / 16.3:1 dark, ink-2 4.80 / 6.85, warn 4.60 / 7.95,
// danger 5.23 / 4.85 — all ≥4.5:1. WHY no new hues: DESIGN.md gates the
// saturated budget to the 8 category colors + accent/warn/danger, so "ok"
// is carried by full ink at weight 600 rather than a green the world
// doesn't own.

import {
  cloneElement,
  createContext,
  isValidElement,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactElement, ReactNode, Ref } from "react";

const cx = (...v: (string | false | undefined | null)[]) =>
  v.filter(Boolean).join(" ");

/* ================================================================== */
/* App shell context — theme, quiet status toasts, shortcut registry. */
/* Provided by App.tsx; safe defaults live here so a component rendered */
/* outside the provider (tests, isolation) never crashes on a hook.    */
/* ================================================================== */

export type ThemeName = "light" | "dark";

export type ToastTone = "neutral" | "ok" | "warning" | "error";

export type StatusOptions = {
  /** Small leading key, sentence case ("clipboard", "runtime error"). */
  key?: string;
  tone?: ToastTone;
  /** Auto-dismiss in ms (default 2600 when called via useShell().status). */
  ms?: number;
};

/**
 * One chord. Modifier semantics are EXACT: every listed modifier must be
 * down and every unlisted one up, so "Ctrl K" can never fire as
 * "Ctrl Shift K" and steal a chord another binding owns.
 */
export type ShortcutBinding = {
  /** KeyboardEvent.key — case-insensitive for single characters ("k"/"K"). */
  key: string;
  /** Ctrl on Windows/Linux, Meta on macOS (either satisfies the chord). */
  mod?: boolean;
  ctrl?: boolean;
  meta?: boolean;
  alt?: boolean;
  shift?: boolean;
  /** Fire even when focus sits in an input/textarea/contenteditable. */
  allowInInput?: boolean;
  /** Dynamic gate evaluated at keypress time (default: always). */
  when?: () => boolean;
};

export type ShellApi = {
  theme: ThemeName;
  setTheme: (t: ThemeName) => void;
  toggleTheme: () => void;
  /**
   * Quiet shell toast (the statusbar-tier voice — NOT the error funnel).
   * Replaces any status already showing; errors keep their own sticky slot.
   */
  status: (message: string, opts?: StatusOptions) => void;
  /** Global shortcut registry. Returns an unregister fn. */
  registerShortcut: (
    binding: ShortcutBinding,
    run: (e: KeyboardEvent) => void,
  ) => () => void;
};

const shellFallback: ShellApi = {
  theme: "dark",
  setTheme: () => undefined,
  toggleTheme: () => undefined,
  status: () => undefined,
  registerShortcut,
};

export const ShellContext = createContext<ShellApi>(shellFallback);

/** Shell facts (theme, status(), shortcut registry). Never throws. */
export function useShell(): ShellApi {
  return useContext(ShellContext);
}

function isEditable(node: EventTarget | null): boolean {
  const el = node as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return el.isContentEditable === true;
}

function matchBinding(e: KeyboardEvent, b: ShortcutBinding): boolean {
  const want = b.key.length === 1 ? b.key.toLowerCase() : b.key;
  const got = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (want !== got) return false;
  const modDown = e.ctrlKey || e.metaKey;
  const ctrlOk = b.mod ? modDown : e.ctrlKey === (b.ctrl ?? false);
  const metaOk = b.mod ? modDown : e.metaKey === (b.meta ?? false);
  return (
    ctrlOk &&
    metaOk &&
    e.altKey === (b.alt ?? false) &&
    e.shiftKey === (b.shift ?? false)
  );
}

/* The registry is a module singleton, passed through the context for
   discovery — WHY: registration must survive any provider arrangement and
   keep one stable function identity, so useShortcut's effect never
   re-registers just because a context value object changed. */
type RegisteredShortcut = { b: ShortcutBinding; run: (e: KeyboardEvent) => void };
const shortcuts = new Map<number, RegisteredShortcut>();
let nextShortcutId = 0;

/** Global shortcut registry. Returns an unregister fn. */
export function registerShortcut(
  binding: ShortcutBinding,
  run: (e: KeyboardEvent) => void,
): () => void {
  const id = ++nextShortcutId;
  shortcuts.set(id, { b: binding, run });
  return () => {
    shortcuts.delete(id);
  };
}

/**
 * App.tsx's single window keydown listener calls this after the built-in
 * chords (Ctrl K / Ctrl F / Escape). First exact match claims the event
 * (preventDefault + run) and stops the scan. Returns true when handled.
 */
export function dispatchShortcuts(e: KeyboardEvent): boolean {
  for (const { b, run } of shortcuts.values()) {
    if (!matchBinding(e, b)) continue;
    // never hijack typing unless the binding opts in; never fire a chord
    // whose dynamic gate has closed
    if (!b.allowInInput && isEditable(e.target)) continue;
    if (b.when && !b.when()) continue;
    if (!e.defaultPrevented) e.preventDefault();
    run(e);
    return true;
  }
  return false;
}

/**
 * Declarative shortcut. Registers once per binding *shape*; the handler and
 * the `when` gate are read through refs at keypress time, so an inline arrow
 * closure never churns the registration (same identity discipline as
 * ExplorerView's onReady effect).
 */
export function useShortcut(
  binding: ShortcutBinding,
  run: (e: KeyboardEvent) => void,
  enabled = true,
): void {
  const { registerShortcut } = useShell();
  const runRef = useRef(run);
  runRef.current = run;
  const whenRef = useRef(binding.when);
  whenRef.current = binding.when;

  useEffect(() => {
    if (!enabled) return;
    return registerShortcut(
      {
        key: binding.key,
        mod: binding.mod,
        ctrl: binding.ctrl,
        meta: binding.meta,
        alt: binding.alt,
        shift: binding.shift,
        allowInInput: binding.allowInInput,
        when: () => (whenRef.current ? whenRef.current() : true),
      },
      (e) => runRef.current(e),
    );
    // Binding shape only — mutable closures ride the refs above.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    enabled,
    registerShortcut,
    binding.key,
    binding.mod,
    binding.ctrl,
    binding.meta,
    binding.alt,
    binding.shift,
    binding.allowInInput,
  ]);
}

/* ================================================================== */
/* Button family                                                       */
/* ================================================================== */

export type ButtonVariant = "primary" | "quiet" | "ghost";
export type ButtonSize = "sm" | "md";

export interface ButtonProps {
  children: ReactNode;
  /** quiet (default) = filled-quiet control · primary = the one accent ·
   *  ghost = bare text that fills on hover. */
  variant?: ButtonVariant;
  size?: ButtonSize;
  onClick?: () => void;
  disabled?: boolean;
  title?: string;
  type?: "button" | "submit";
  className?: string;
  ariaLabel?: string;
  ariaPressed?: boolean;
  ref?: Ref<HTMLButtonElement>;
}

export function Button({
  children,
  variant = "quiet",
  size = "md",
  onClick,
  disabled,
  title,
  type = "button",
  className,
  ariaLabel,
  ariaPressed,
  ref,
}: ButtonProps) {
  return (
    <button
      ref={ref}
      type={type}
      className={cx(
        "btn",
        size === "sm" && "btn-sm",
        variant === "primary" && "btn-primary",
        variant === "ghost" && "btn-ghost",
        className,
      )}
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={ariaLabel}
      aria-pressed={ariaPressed}
    >
      {children}
    </button>
  );
}

/** Legacy name (byte-stable): quiet/primary only, as consumed today. */
export function Btn({
  children,
  onClick,
  primary,
  disabled,
  title,
}: {
  children: ReactNode;
  onClick?: () => void;
  primary?: boolean;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <Button variant={primary ? "primary" : "quiet"} onClick={onClick} disabled={disabled} title={title}>
      {children}
    </Button>
  );
}

export interface IconButtonProps {
  children: ReactNode;
  /** Required accessible name — an icon-only button without one is a defect. */
  label: string;
  /** Native tooltip text; defaults to label (DESIGN: title AND aria-label). */
  title?: string;
  onClick?: () => void;
  disabled?: boolean;
  /** Toggle state: paints is-on and sets aria-pressed. */
  active?: boolean;
  size?: ButtonSize;
  className?: string;
  /** Explicit pressed state when it differs from `active`'s visual role. */
  ariaPressed?: boolean;
  ref?: Ref<HTMLButtonElement>;
}

export function IconButton({
  children,
  label,
  title,
  onClick,
  disabled,
  active,
  size = "md",
  className,
  ariaPressed,
  ref,
}: IconButtonProps) {
  const pressed = ariaPressed ?? (active === undefined ? undefined : active);
  return (
    <button
      ref={ref}
      type="button"
      className={cx("icon-btn", size === "sm" && "icon-btn-sm", active && "is-on", className)}
      onClick={onClick}
      disabled={disabled}
      title={title ?? label}
      aria-label={label}
      aria-pressed={pressed}
    >
      {children}
    </button>
  );
}

/** Legacy name (byte-stable): `title` doubles as the accessible name. */
export function IconBtn({
  children,
  onClick,
  title,
  disabled,
  active,
}: {
  children: ReactNode;
  onClick?: () => void;
  title?: string;
  disabled?: boolean;
  active?: boolean;
}) {
  return (
    <IconButton label={title ?? "button"} title={title} onClick={onClick} disabled={disabled} active={active}>
      {children}
    </IconButton>
  );
}

/* ================================================================== */
/* Segmented control — Size/Name/Type style switch.                    */
/* Real tablist: aria-selected (not aria-pressed — that CSS lives in    */
/* primitives.css keyed on the legacy name; shell.css carries the      */
/* aria-selected twin), roving tabindex, arrows/Home/End to move.       */
/* ================================================================== */

export interface SegmentedControlProps<T extends string> {
  options: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
  /** Names the tablist for assistive tech. */
  label?: string;
  className?: string;
}

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  label,
  className,
}: SegmentedControlProps<T>) {
  const boxRef = useRef<HTMLDivElement | null>(null);
  // value not in options (stale prop) → first option is the live tab, so
  // the roving tabindex always has exactly one stop
  const found = options.some((o) => o.id === value);
  const effective = found ? value : options[0]?.id;

  const focusTab = (i: number) => {
    const nodes = boxRef.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]');
    nodes?.[i]?.focus();
  };

  const onKeyDown = (e: ReactKeyboardEvent) => {
    const n = options.length;
    if (!n) return;
    const i = Math.max(0, options.findIndex((o) => o.id === effective));
    let next = -1;
    if (e.key === "ArrowRight") next = (i + 1) % n;
    else if (e.key === "ArrowLeft") next = (i - 1 + n) % n;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = n - 1;
    if (next < 0) return;
    e.preventDefault();
    onChange(options[next].id);
    focusTab(next);
  };

  return (
    <div ref={boxRef} className={cx("seg", className)} role="tablist" aria-label={label} onKeyDown={onKeyDown}>
      {options.map((o) => {
        const selected = o.id === effective;
        return (
          <button
            key={o.id}
            type="button"
            role="tab"
            className="seg-item"
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(o.id)}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** Legacy name (byte-stable props) for the sort switch in the topbar. */
export const Segmented = SegmentedControl;

/* ================================================================== */
/* Tooltip — mouse + keyboard, non-blocking.                           */
/* Wrapper-span approach: handlers live on the wrapping span (focus/    */
/* blur bubble as focusin/focusout in React), so ANY child works — a    */
/* component that doesn't forward props still gets its tooltip. The     */
/* bubble is pointer-events:none (can never swallow a click or hover)   */
/* and focus never moves (WCAG 1.4.13: dismissible via Esc, hoverable  */
/* because the bubble never intercepts the pointer).                    */
/* ================================================================== */

export function Tooltip({
  content,
  placement = "top",
  delayMs = 400,
  children,
}: {
  content: ReactNode;
  placement?: "top" | "bottom";
  /** Pointer reveal delay; keyboard focus reveals immediately. */
  delayMs?: number;
  children: ReactElement;
}) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const wrapRef = useRef<HTMLSpanElement | null>(null);
  const tipRef = useRef<HTMLSpanElement | null>(null);
  const timerRef = useRef(0);

  const show = useCallback(
    (immediate: boolean) => {
      window.clearTimeout(timerRef.current);
      if (immediate) setOpen(true);
      else timerRef.current = window.setTimeout(() => setOpen(true), delayMs);
    },
    [delayMs],
  );
  const hide = useCallback(() => {
    window.clearTimeout(timerRef.current);
    setOpen(false);
  }, []);
  useEffect(() => () => window.clearTimeout(timerRef.current), []);

  // Position before paint: center on the trigger, clamp to the viewport,
  // flip when the preferred side doesn't fit. Local coords because the
  // wrap is the positioned ancestor.
  useLayoutEffect(() => {
    if (!open) return;
    const wrap = wrapRef.current;
    const tip = tipRef.current;
    if (!wrap || !tip) return;
    const wr = wrap.getBoundingClientRect();
    const tw = tip.offsetWidth;
    const th = tip.offsetHeight;
    const left = Math.round(
      Math.max(8, Math.min(wr.left + wr.width / 2 - tw / 2, window.innerWidth - tw - 8)) - wr.left,
    );
    let top = placement === "top" ? wr.top - th - 6 : wr.bottom + 6;
    if (top < 8) top = wr.bottom + 6;
    else if (top + th > window.innerHeight - 8) top = wr.top - th - 6;
    tip.style.left = `${left}px`;
    tip.style.top = `${Math.round(top - wr.top)}px`;
  }, [open, placement, content]);

  // Merge describedby instead of overwriting: a child that already points
  // at its own description keeps it while the tip is closed; while open,
  // the tip id joins the list.
  const kid = isValidElement(children)
    ? cloneElement(children as ReactElement<Record<string, unknown>>, (() => {
        const own = (children.props as Record<string, unknown>)["aria-describedby"];
        const ownStr = typeof own === "string" ? own : undefined;
        return {
          "aria-describedby": open
            ? ownStr
              ? `${ownStr} ${id}`
              : id
            : ownStr,
        };
      })())
    : children;

  return (
    <span
      ref={wrapRef}
      className="ttip-wrap"
      onPointerEnter={() => show(false)}
      onPointerLeave={hide}
      onFocus={() => show(true)}
      onBlur={hide}
      onKeyDown={(e) => {
        if (open && e.key === "Escape") hide();
      }}
    >
      {kid}
      {open && (
        <span
          ref={tipRef}
          id={id}
          role="tooltip"
          className={cx("ttip", placement === "top" ? "is-top" : "is-bottom")}
        >
          {content}
        </span>
      )}
    </span>
  );
}

/* ================================================================== */
/* KeyHint — keycap chips for a chord. "Ctrl K" → [Ctrl][K].           */
/* ================================================================== */

export function KeyHint({ keys, className }: { keys: string | string[]; className?: string }) {
  const tokens = Array.isArray(keys) ? keys : keys.split(" ");
  return (
    <span className={cx("keyhint", className)}>
      {tokens.map((t, i) => (
        <span className="kbd" key={`${t}-${i}`}>
          {t}
        </span>
      ))}
    </span>
  );
}

/* ================================================================== */
/* Spinner — indeterminate activity. `label=""` renders decorative      */
/* (aria-hidden). Under reduced motion the global clamp freezes it at   */
/* its static base state: a ring with one accent segment — honest,      */
/* never a phantom percentage.                                          */
/* ================================================================== */

export function Spinner({
  size = 14,
  label = "Loading",
  className,
}: {
  size?: number;
  label?: string;
  className?: string;
}) {
  const decorative = label === "";
  return (
    <span
      className={cx("spinner", className)}
      style={{ width: size, height: size }}
      role={decorative ? undefined : "status"}
      aria-label={decorative ? undefined : label}
      aria-hidden={decorative || undefined}
    />
  );
}

/* ================================================================== */
/* Meter / CapacityBar — deterministic fill readout (role=meter).       */
/* `label` is required on purpose: a meter with no accessible name is   */
/* a violation. `valueText` carries the phrasing AT should speak ("88%   */
/* used"); the raw number alone has no units. Tone is the caller's      */
/* threshold decision (drives warn >80, crit >92).                      */
/* ================================================================== */

export type MeterTone = "neutral" | "accent" | "warning" | "critical";

export function Meter({
  value,
  max,
  tone = "neutral",
  label,
  valueText,
  size = "md",
  bed,
  className,
}: {
  value: number;
  max: number;
  tone?: MeterTone;
  label: string;
  valueText?: string;
  size?: "sm" | "md";
  /** Dot-matrix bed under the fill (the drives ribbon's texture), opt-in. */
  bed?: boolean;
  className?: string;
}) {
  const safeMax = max > 0 && Number.isFinite(max) ? max : 0;
  const finite = Number.isFinite(value) ? value : 0;
  const clamped = Math.max(0, safeMax ? Math.min(finite, safeMax) : 0);
  const pct = safeMax ? (clamped / safeMax) * 100 : 0;
  return (
    <div
      className={cx("meter", `is-${tone}`, size === "sm" && "is-sm", bed && "tex-dots", className)}
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={safeMax}
      aria-valuenow={clamped}
      aria-valuetext={valueText}
    >
      <span className="meter-fill" style={{ width: `${pct}%` }} />
    </div>
  );
}

/** Alias — same component; CapacityBar reads better at drive/volume scale. */
export const CapacityBar = Meter;

/* ================================================================== */
/* StatusWord — a word carrying a real state (low, critical, ready).    */
/* Color never arrives alone (Two-Channel): the word IS the channel;    */
/* tone only reinforces it.                                             */
/* ================================================================== */

export type StatusTone = "neutral" | "ok" | "warning" | "critical";

export function StatusWord({
  children,
  tone = "neutral",
  title,
  className,
}: {
  children: ReactNode;
  tone?: StatusTone;
  title?: string;
  className?: string;
}) {
  return (
    <span className={cx("status-word", `is-${tone}`, className)} title={title}>
      {children}
    </span>
  );
}

/* ================================================================== */
/* EmptyState — an honest empty region: what's missing + why.           */
/* No illustration, no filler (product principle 4).                    */
/* ================================================================== */

export function EmptyState({
  title,
  hint,
  icon,
  action,
  className,
}: {
  title: string;
  hint?: ReactNode;
  icon?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("empty", className)}>
      {icon && (
        <span className="empty-ico" aria-hidden="true">
          {icon}
        </span>
      )}
      <p className="empty-title">{title}</p>
      {hint && <p className="micro empty-hint">{hint}</p>}
      {action && <div className="empty-action">{action}</div>}
    </div>
  );
}

/* ================================================================== */
/* Toast — the one shell message strip. Two slots exist in App: errors  */
/* (sticky, manual dismiss — an unacknowledged error never vanishes on  */
/* a timer) and status (auto-dismiss). role=status polite by design:     */
/* pointer/keyboard feedback must not interrupt a screen reader mid-task.*/
/* ================================================================== */

export function Toast({
  message,
  tone = "neutral",
  labelKey,
  onDismiss,
  dismissAfter,
  role = "status",
  className,
}: {
  message: string;
  tone?: ToastTone;
  /** Leading key ("runtime error", "clipboard"). Omitted → message alone. */
  labelKey?: string;
  onDismiss?: () => void;
  /** Auto-dismiss in ms; clears on unmount/message change. */
  dismissAfter?: number;
  role?: "status" | "alert";
  className?: string;
}) {
  useEffect(() => {
    if (!dismissAfter || !onDismiss) return;
    const t = window.setTimeout(onDismiss, dismissAfter);
    return () => window.clearTimeout(t);
  }, [dismissAfter, onDismiss, message]);

  return (
    <div className={cx("toast", `is-${tone}`, className)} role={role}>
      {labelKey && <span className="toast-key">{labelKey}</span>}
      <span className="toast-msg">{message}</span>
      {onDismiss && (
        <button type="button" className="toast-x" aria-label="Dismiss" onClick={onDismiss}>
          ×
        </button>
      )}
    </div>
  );
}

/* ================================================================== */
/* VisuallyHidden — present in the accessibility tree, absent from      */
/* the pixels. For labels a sighted layout implies but AT needs stated. */
/* ================================================================== */

export function VisuallyHidden({ children }: { children: ReactNode }) {
  return <span className="vh">{children}</span>;
}

/* ================================================================== */
/* HoverCard — the one readout for the whole app: the complete truth    */
/* of whatever the pointer is over (treemap tile or rail row).          */
/* Identity-correct by construction: the owner passes the payload of    */
/* the item currently under the pointer, never a stale or first one.    */
/*                                                                     */
/* Placement: absolutely positioned inside the nearest positioned       */
/* ancestor (owner's container must be position:relative). The card     */
/* measures itself and flips/clamps away from every edge.               */
/* Position may be written by the owner imperatively between renders;   */
/* the component only repositions when card/x/y change.                 */
/* ================================================================== */

export type HoverAction = {
  label: string;
  title?: string;
  icon?: ReactNode;
  onRun: () => void;
};

export type HoverCardData = {
  name: string;
  /** Pre-formatted byte figure (mono hero of the card). */
  size: string;
  /** Pre-formatted share readout, e.g. "8.3% of D:". */
  share?: string;
  /** Category chip payload (dot + word — the Two-Channel Rule). */
  cat?: { label: string; color: string };
  /** Real count line, e.g. "13 entries" or "1 file". */
  count?: string;
  /** Full path, ellipsized from the head (tail always visible). */
  path?: string;
  /** Real actions only — the card never shows dead buttons. */
  actions?: HoverAction[];
};

export function HoverCard({
  card,
  x,
  y,
  interactive = false,
  ref: outerRef,
}: {
  card: HoverCardData | null;
  x: number;
  y: number;
  interactive?: boolean;
  /** Lets the owner nudge position imperatively between identity changes. */
  ref?: Ref<HTMLDivElement>;
}) {
  const inner = useRef<HTMLDivElement | null>(null);

  useLayoutEffect(() => {
    const el = inner.current;
    if (!el || !card) return;
    const host = el.offsetParent as HTMLElement | null;
    const hw = host?.clientWidth ?? window.innerWidth;
    const hh = host?.clientHeight ?? window.innerHeight;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    let left = x + 14;
    let top = y + 16;
    // flip when the card would leave its host, clamp to the host edge
    if (left + w > hw - 8) left = x - w - 14;
    if (top + h > hh - 8) top = y - h - 12;
    el.style.left = `${Math.round(Math.max(8, Math.min(left, hw - w - 8)))}px`;
    el.style.top = `${Math.round(Math.max(8, Math.min(top, hh - h - 8)))}px`;
  }, [card, x, y]);

  const setNode = (node: HTMLDivElement | null) => {
    inner.current = node;
    if (typeof outerRef === "function") outerRef(node);
    else if (outerRef && typeof outerRef === "object")
      (outerRef as { current: HTMLDivElement | null }).current = node;
  };

  const cls = [
    "hovercard float",
    card ? "is-live" : "",
    interactive && card ? "is-interactive" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      ref={setNode}
      className={cls}
      style={{ left: 0, top: 0 }}
      role="status"
      aria-live="off"
    >
      {card && (
        <>
          <div className="hc-name">{card.name}</div>
          <div className="hc-figures">
            <span className="hc-size num">{card.size}</span>
            {card.share && <span className="hc-share num">{card.share}</span>}
          </div>
          {(card.cat || card.count) && (
            <div className="hc-meta">
              {card.cat && (
                <span className="chip">
                  <span
                    className="chip-dot"
                    style={{ background: card.cat.color }}
                  />
                  {card.cat.label}
                </span>
              )}
              {card.count && <span className="micro">{card.count}</span>}
            </div>
          )}
          {card.path && <div className="hc-path num">{card.path}</div>}
          {interactive && card.actions && card.actions.length > 0 && (
            <div className="hc-actions">
              {card.actions.map((a) => (
                <button
                  key={a.label}
                  type="button"
                  className="icon-btn"
                  aria-label={a.title ?? a.label}
                  onClick={a.onRun}
                >
                  {a.icon ?? a.label}
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
