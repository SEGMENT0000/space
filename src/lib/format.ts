// Byte / number formatting with tabular-friendly output.

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** 1536 -> "1.5 KB". Precision adapts to magnitude. */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n < 1024) return `${Math.round(n)} B`;
  const { num, unit } = splitBytes(n);
  return `${num} ${unit}`;
}

/** 1536 -> "1.5" (number only, no unit) */
export function bytesValue(n: number): { value: number; unit: string } {
  let v = Math.max(0, n);
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return { value: v, unit: UNITS[u] };
}

export function formatCount(n: number): string {
  return n.toLocaleString("en-US");
}

export function formatElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  return `${m}m ${r.toString().padStart(2, "0")}s`;
}

/** Shorten a long path for display: C:\Users\casey\AppData\Local\Temp -> C:\Users\...\Local\Temp */
export function ellipsizePath(p: string, maxLen = 64): string {
  if (p.length <= maxLen) return p;
  const parts = p.split(/[\\/]/);
  if (parts.length <= 3) return p.slice(0, maxLen - 1) + "…";
  const head = parts.slice(0, 2).join("\\");
  const tail = parts.slice(-2).join("\\");
  const mid = parts.slice(2, -2);
  let out = `${head}\\…\\${tail}`;
  if (out.length >= maxLen && mid.length > 0) {
    return `${head}\\…\\${parts[parts.length - 1]}`;
  }
  return out;
}

/** 0.153 -> "15.3%" */
export function formatPercent(frac: number): string {
  const p = frac * 100;
  if (p >= 10) return `${p.toFixed(0)}%`;
  if (p > 0) return `${p.toFixed(1)}%`;
  return "0%";
}

/** 1536 -> { num: "1.5", unit: "KB" } — for big numerals with a smaller unit. */
export function splitBytes(n: number): { num: string; unit: string } {
  const { value, unit } = bytesValue(n);
  const digits = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return { num: value.toFixed(digits), unit };
}
