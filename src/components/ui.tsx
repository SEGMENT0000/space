import { useLayoutEffect, useRef } from "react";
import type { ReactNode, Ref } from "react";

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
    <button
      type="button"
      className={primary ? "btn btn-primary" : "btn"}
      onClick={onClick}
      disabled={disabled}
      title={title}
    >
      {children}
    </button>
  );
}

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
    <button
      type="button"
      className={active ? "icon-btn is-on" : "icon-btn"}
      onClick={onClick}
      title={title}
      aria-label={title}
      disabled={disabled}
    >
      {children}
    </button>
  );
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
}) {
  return (
    <div className="seg" role="tablist">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          role="tab"
          className="seg-item"
          aria-pressed={o.id === value}
          onClick={() => onChange(o.id)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* HoverCard — the one readout for the whole app: the complete truth   */
/* of whatever the pointer is over (treemap tile or rail row).         */
/* Identity-correct by construction: the owner passes the payload of   */
/* the item currently under the pointer, never a stale or first one.   */
/*                                                                     */
/* Placement: absolutely positioned inside the nearest positioned      */
/* ancestor (owner's container must be position:relative). The card    */
/* measures itself and flips/clamps away from every edge.              */
/* Position may be written by the owner imperatively between renders;  */
/* the component only repositions when card/x/y change.                */
/* ------------------------------------------------------------------ */

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
