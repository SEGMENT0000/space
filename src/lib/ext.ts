/** Lowercased extension without the dot; "" when the base name has none.
    Accepts a path or a bare name — the directory part is stripped first, so
    a dot inside a folder name cannot leak into the result. */
export function extOf(path: string): string {
  const base = path.slice(path.lastIndexOf("\\") + 1);
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}
