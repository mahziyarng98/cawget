// Typed wrappers around the Tauri commands exposed by src-tauri/src/main.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type JobState = "queued" | "active" | "paused" | "done" | "failed";

export interface Part {
  start: number;
  end: number;
  done: number;
}

export interface Job {
  id: number;
  url: string;
  out_path: string;
  state: JobState;
  error: string | null;
  size: number | null;
  accept_ranges: boolean;
  etag: string | null;
  last_modified: string | null;
  sha256: string | null;
  referer: string | null;
  headers: string[];
  /** Requested connections for this job (0 = the global setting). */
  segments: number;
  /** Bytes written so far. */
  done: number;
  /** Bytes per second. */
  speed: number;
  connections: number;
  /** 0..1, null while the size is unknown. */
  fraction: number | null;
  eta_secs: number | null;
  name: string;
  not_before: number;
  created: number;
  parts: Part[];
}

export interface SettingsValues {
  segments: number;
  max_jobs: number;
  min_segment_size: number;
  max_speed: number;
  retries: number;
  connect_timeout: number;
  cacert: string | null;
  user_agent: string;
  download_dir: string | null;
  window: [number, number] | null;
  /** Folder per category id; missing = default folder. */
  category_dirs: Record<string, string>;
  /** "" = system proxy, "none" = direct, otherwise a proxy URL. */
  proxy: string;
}

export interface SettingsDto {
  values: SettingsValues;
  info: { key: string; default: string; help: string }[];
  default_dir: string;
}

export interface AppInfo {
  version: string;
  db_path: string;
  engine_error: string | null;
  /** Old database copied to `db_path` at this start (the old file is unchanged). */
  migrated_from: string | null;
  /** Old database in use by another process; used in place for this run. */
  migration_deferred: string | null;
}

export interface ProbeInfo {
  size: number | null;
  resumable: boolean;
  file_name: string;
  final_url: string;
}

export interface AddOptions {
  dir?: string;
  /** Full path of the file (single address only). */
  output?: string;
  segments?: number;
  startInSecs?: number;
  /** "Start later": add stopped. */
  paused?: boolean;
  login?: string;
  password?: string;
}

export type EngineEvent =
  | { type: "done"; id: number; path: string }
  | { type: "failed"; id: number; error: string };

export const api = {
  listJobs: () => invoke<Job[]>("list_jobs"),
  addDownloads: (urls: string, o: AddOptions = {}) =>
    invoke<number[]>("add_downloads", {
      urls,
      dir: o.dir ?? null,
      output: o.output ?? null,
      segments: o.segments ?? null,
      startInSecs: o.startInSecs ?? null,
      paused: o.paused ?? null,
      login: o.login ?? null,
      password: o.password ?? null,
    }),
  probeUrl: (url: string, login?: string, password?: string) => invoke<ProbeInfo>("probe_url", { url, login: login ?? null, password: password ?? null }),
  fileExists: (path: string) => invoke<boolean>("file_exists", { path }),
  suggestFolder: (fileName: string) => invoke<string>("suggest_folder", { fileName }),
  scheduleJobs: (ids: number[], at: number) => invoke<number>("schedule_jobs", { ids, at }),
  trayState: () => invoke<{ available: boolean; close_to_tray: boolean }>("tray_state"),
  setCloseToTray: (on: boolean) => invoke<void>("set_close_to_tray", { on }),
  quitApp: () => invoke<void>("quit_app"),
  pause: (id: number) => invoke<void>("pause_job", { id }),
  resume: (id: number) => invoke<void>("resume_job", { id }),
  remove: (id: number, deleteFile: boolean) => invoke<void>("remove_job", { id, deleteFile }),
  clearFinished: () => invoke<number>("clear_finished"),
  openFolder: (id: number) => invoke<void>("open_folder", { id }),
  openFile: (id: number) => invoke<void>("open_file", { id }),
  getSettings: () => invoke<SettingsDto>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  appInfo: () => invoke<AppInfo>("app_info"),
};

export function onJobs(cb: (jobs: Job[]) => void): Promise<UnlistenFn> {
  return listen<Job[]>("jobs", (e) => cb(e.payload));
}
export function onEngineEvent(cb: (e: EngineEvent) => void): Promise<UnlistenFn> {
  return listen<EngineEvent>("download-event", (e) => cb(e.payload));
}
export function onTrayAdd(cb: () => void): Promise<UnlistenFn> {
  return listen("tray-add", () => cb());
}
export function onEngineError(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("engine-error", (e) => cb(e.payload));
}

// ---- formatting ----

export function fmtBytes(v: number): string {
  const u = ["B", "KiB", "MiB", "GiB", "TiB"];
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${Math.round(v)} ${u[i]}` : `${v.toFixed(v >= 100 ? 0 : 1)} ${u[i]}`;
}

export function fmtEta(secs: number | null): string {
  if (secs == null) return "--:--";
  if (secs >= 3600) return `${Math.floor(secs / 3600)}:${String(Math.floor((secs / 60) % 60)).padStart(2, "0")}:${String(secs % 60).padStart(2, "0")}`;
  return `${String(Math.floor(secs / 60)).padStart(2, "0")}:${String(secs % 60).padStart(2, "0")}`;
}

export function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

export function extOf(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 && i < name.length - 1 ? name.slice(i + 1).toLowerCase() : "";
}
