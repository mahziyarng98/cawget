// The download table model: columns, layout (order, widths, sort) and row ordering. Pure functions, unit-tested.
import type { Job, JobState } from "./api";

export type ColumnId = "name" | "queue" | "size" | "status" | "eta" | "rate" | "lastTry" | "description";
export type SortDir = 1 | -1;

export interface ColumnDef {
  id: ColumnId;
  label: string;
  width: number;
  min: number;
  align: "start" | "end";
  /** False when the engine has no data for the column (rendered as a dash, header explains). */
  supported: boolean;
}

export const COMING_SOON = "Coming soon";

export const COLUMNS: ColumnDef[] = [
  { id: "name", label: "File name", width: 300, min: 120, align: "start", supported: true },
  { id: "queue", label: "Queue", width: 64, min: 52, align: "start", supported: false },
  { id: "size", label: "Size", width: 90, min: 60, align: "end", supported: true },
  { id: "status", label: "Status", width: 210, min: 90, align: "start", supported: true },
  { id: "eta", label: "Time left", width: 84, min: 60, align: "end", supported: true },
  { id: "rate", label: "Speed", width: 90, min: 60, align: "end", supported: true },
  { id: "lastTry", label: "Last attempt", width: 120, min: 80, align: "start", supported: false },
  { id: "description", label: "Notes", width: 200, min: 60, align: "start", supported: false },
];

export const columnDef = (id: ColumnId): ColumnDef => COLUMNS.find((c) => c.id === id)!;

export interface TableLayout {
  order: ColumnId[];
  widths: Record<ColumnId, number>;
  sort: { col: ColumnId; dir: SortDir } | null;
  /** Columns the user switched off (View > Columns). The file name cannot be hidden. */
  hidden: ColumnId[];
}

export function defaultLayout(): TableLayout {
  return {
    order: COLUMNS.map((c) => c.id),
    widths: Object.fromEntries(COLUMNS.map((c) => [c.id, c.width])) as Record<ColumnId, number>,
    sort: null,
    hidden: [],
  };
}

/** The file name is the flexible column: it takes the free width and never gets narrower than this. */
export const NAME_MIN = 160;

/** When space is short, columns are hidden in this order (the file name never is). */
export const HIDE_ORDER: ColumnId[] = ["description", "lastTry", "queue", "rate", "eta", "size", "status"];

/**
 * Columns to show, in the user's order: the user's own hidden columns stay hidden, then the lowest-priority
 * columns are hidden until the fixed-width columns plus the name's minimum fit into `available`. Nothing in the
 * stored layout changes, so the user's widths and choices come back when there is room again.
 */
export function visibleColumns(layout: TableLayout, available: number): ColumnId[] {
  const shown = new Set(layout.order.filter((c) => c === "name" || !layout.hidden.includes(c)));
  const need = () => NAME_MIN + [...shown].filter((c) => c !== "name").reduce((s, c) => s + layout.widths[c], 0);
  for (const c of HIDE_ORDER) {
    if (need() <= available) break;
    shown.delete(c);
  }
  return layout.order.filter((c) => shown.has(c));
}

/** Moves column `id` by `delta` places among the visible columns (hidden ones are skipped). */
export function moveVisible(order: ColumnId[], visible: ColumnId[], id: ColumnId, delta: number): ColumnId[] {
  const i = visible.indexOf(id);
  if (i < 0) return order;
  const target = visible[Math.max(0, Math.min(visible.length - 1, i + delta))];
  return target === id ? order : dropColumn(order, id, target);
}

/** Switches a column on or off for the user (not the file name). */
export function toggleHidden(layout: TableLayout, id: ColumnId): TableLayout {
  if (id === "name") return layout;
  const hidden = layout.hidden.includes(id) ? layout.hidden.filter((c) => c !== id) : [...layout.hidden, id];
  return { ...layout, hidden };
}

/** Accepts persisted data only if it is a complete, valid layout; repairs what it can. */
export function sanitizeLayout(raw: unknown): TableLayout {
  const def = defaultLayout();
  if (!raw || typeof raw !== "object") return def;
  const r = raw as Partial<TableLayout>;
  const ids = COLUMNS.map((c) => c.id);
  const order = Array.isArray(r.order) && r.order.length === ids.length && ids.every((id) => r.order!.includes(id)) ? [...r.order] : def.order;
  const widths = { ...def.widths };
  if (r.widths && typeof r.widths === "object") {
    for (const c of COLUMNS) {
      const w = (r.widths as Record<string, unknown>)[c.id];
      if (typeof w === "number" && Number.isFinite(w)) widths[c.id] = clampWidth(c.id, w);
    }
  }
  const s = r.sort;
  const sort = s && ids.includes(s.col) && (s.dir === 1 || s.dir === -1) && columnDef(s.col).supported ? { col: s.col, dir: s.dir } : null;
  const hidden = Array.isArray(r.hidden) ? ids.filter((id) => id !== "name" && r.hidden!.includes(id)) : [];
  return { order, widths, sort, hidden };
}

export function clampWidth(id: ColumnId, w: number): number {
  return Math.round(Math.min(900, Math.max(columnDef(id).min, w)));
}

/** Moves column `id` by `delta` positions (negative = towards the start). */
export function moveColumn(order: ColumnId[], id: ColumnId, delta: number): ColumnId[] {
  const from = order.indexOf(id);
  if (from < 0) return order;
  const to = Math.max(0, Math.min(order.length - 1, from + delta));
  const next = order.filter((c) => c !== id);
  next.splice(to, 0, id);
  return next;
}

/** Places column `id` where `target` is (drag and drop). */
export function dropColumn(order: ColumnId[], id: ColumnId, target: ColumnId): ColumnId[] {
  if (id === target) return order;
  return moveColumn(order, id, order.indexOf(target) - order.indexOf(id));
}

/** Header click: unsorted -> ascending -> descending -> ascending... Unsupported columns do not sort. */
export function toggleSort(sort: TableLayout["sort"], col: ColumnId): TableLayout["sort"] {
  if (!columnDef(col).supported) return sort;
  if (sort?.col === col) return { col, dir: sort.dir === 1 ? -1 : 1 };
  return { col, dir: 1 };
}

const STATE_ORDER: Record<JobState, number> = { active: 0, queued: 1, paused: 2, failed: 3, done: 4 };

function cmpNullable(a: number | null, b: number | null): number {
  if (a == null && b == null) return 0;
  if (a == null) return 1; // unknown values last, whatever the direction
  if (b == null) return -1;
  return a - b;
}

/** Rows in display order. Without an explicit sort: newest first, as a download list usually shows. */
export function sortJobs(jobs: Job[], sort: TableLayout["sort"]): Job[] {
  const rows = [...jobs];
  if (!sort) return rows.sort((a, b) => b.id - a.id);
  const { col, dir } = sort;
  const key = (j: Job): number | string | null => {
    switch (col) {
      case "name":
        return j.name.toLocaleLowerCase();
      case "size":
        return j.size;
      case "status":
        return STATE_ORDER[j.state] * 1000 + Math.round((j.fraction ?? 0) * 100);
      case "eta":
        return j.state === "active" ? j.eta_secs : null;
      case "rate":
        return j.state === "active" ? j.speed : null;
      default:
        return null;
    }
  };
  return rows.sort((a, b) => {
    const ka = key(a), kb = key(b);
    let c: number;
    if (typeof ka === "string" && typeof kb === "string") c = ka.localeCompare(kb) * dir;
    else if (ka == null || kb == null) c = cmpNullable(ka as number | null, kb as number | null);
    else c = ((ka as number) - (kb as number)) * dir;
    return c !== 0 ? c : a.id - b.id;
  });
}

/** Text of the Status column. */
export function statusText(j: Job, nowSecs: number): string {
  switch (j.state) {
    case "active":
      if (j.connections === 0 && j.done === 0) return "Connecting…";
      return j.fraction != null ? `${(j.fraction * 100).toFixed(1)}%` : "Downloading…";
    case "queued":
      if (j.not_before > nowSecs) {
        const d = new Date(j.not_before * 1000);
        return `Scheduled ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
      }
      return "Queued";
    case "paused":
      return j.fraction != null && j.fraction > 0 ? `Stopped (${(j.fraction * 100).toFixed(1)}%)` : "Stopped";
    case "done":
      return "Completed";
    case "failed":
      return "Failed";
  }
}
