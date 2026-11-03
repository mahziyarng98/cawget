// Pure helpers for the Options and Scheduler dialogs (unit-tested).

export type SpeedUnit = "K" | "M";

/** Bytes/s -> editor value; 0 = unlimited. MiB when it divides evenly, else KiB. */
export function speedToEditor(bps: number): { value: number; unit: SpeedUnit } {
  if (bps <= 0) return { value: 0, unit: "K" };
  if (bps % 1048576 === 0) return { value: bps / 1048576, unit: "M" };
  return { value: Math.round(bps / 1024), unit: "K" };
}

/** Editor value -> setting string ("" = unlimited). */
export function speedSetting(value: number, unit: SpeedUnit): string {
  return Number.isFinite(value) && value > 0 ? `${value}${unit}` : "";
}

export type ProxyMode = "system" | "none" | "manual";

export function proxyMode(v: string): { mode: ProxyMode; url: string } {
  if (v === "") return { mode: "system", url: "" };
  if (v === "none") return { mode: "none", url: "" };
  return { mode: "manual", url: v };
}

/** Setting value for a proxy mode; null when the manual address is not a usable URL. */
export function proxySetting(mode: ProxyMode, url: string): string | null {
  if (mode === "system") return "";
  if (mode === "none") return "none";
  const u = url.trim();
  return /^(https?|socks5h?):\/\/[^\s/:]+(:\d{1,5})?\/?$/i.test(u) || /^(https?|socks5h?):\/\/[^\s@/]+@[^\s/:]+(:\d{1,5})?\/?$/i.test(u) ? u : null;
}

export const hhmm = (m: number) => `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;

/** `<input type="datetime-local">` value (local time) -> unix seconds, or null. */
export function localInputToUnix(v: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(v)) return null;
  const t = new Date(v).getTime();
  return Number.isFinite(t) ? Math.floor(t / 1000) : null;
}

/** The next full hour in local time (timezones with a 30/45-minute offset included), as unix seconds. */
export function nextLocalHour(nowSecs: number): number {
  const d = new Date(nowSecs * 1000);
  d.setMinutes(0, 0, 0);
  d.setHours(d.getHours() + 1);
  return Math.floor(d.getTime() / 1000);
}

/** unix seconds -> `<input type="datetime-local">` value in local time. */
export function unixToLocalInput(s: number): string {
  const d = new Date(s * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}
