// UI preferences in localStorage. Every access is guarded: storage can be unavailable or hold stale data.
import { PREFS_KEY } from "./brand";

export function loadPref<T>(key: string, fallback: T, valid: (v: unknown) => v is T): T {
  try {
    const raw = localStorage.getItem(`${PREFS_KEY}.${key}`);
    if (raw == null) return fallback;
    const v: unknown = JSON.parse(raw);
    return valid(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

export function savePref(key: string, value: unknown): void {
  try {
    localStorage.setItem(`${PREFS_KEY}.${key}`, JSON.stringify(value));
  } catch {
    /* preferences are a convenience only */
  }
}
