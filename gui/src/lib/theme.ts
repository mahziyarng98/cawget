// Theme preference and the system colour-scheme bridge (pure helpers, unit-tested).

/** What the user chose in Options: follow the desktop, or always light/dark. */
export type ThemeMode = "system" | "light" | "dark";
/** The tokens actually applied to the document. */
export type ColorTheme = "light" | "dark";

export const isThemeMode = (v: unknown): v is ThemeMode => v === "system" || v === "light" || v === "dark";

/** System follows the desktop; a missing dark preference means light. */
export const resolveTheme = (mode: ThemeMode, systemDark: boolean): ColorTheme =>
  mode === "system" ? (systemDark ? "dark" : "light") : mode;

const DARK_QUERY = "(prefers-color-scheme: dark)";

/** The desktop preference as WebKitGTK reports it (GTK's dark preference, set from the XDG portal). */
export function mediaPrefersDark(): boolean {
  try {
    return typeof window !== "undefined" && window.matchMedia?.(DARK_QUERY).matches === true;
  } catch {
    return false;
  }
}

/** Notify on changes to the desktop preference; returns the cleanup function. */
export function watchMediaTheme(apply: (dark: boolean) => void): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => {};
  let query: MediaQueryList;
  try {
    query = window.matchMedia(DARK_QUERY);
  } catch {
    return () => {};
  }
  const onChange = (event: MediaQueryListEvent | MediaQueryList) => apply(event.matches);
  // Older WebKitGTK only has the deprecated listener API.
  if (typeof query.addEventListener === "function") {
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }
  query.addListener(onChange);
  return () => query.removeListener(onChange);
}
