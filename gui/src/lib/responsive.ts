// Responsive layout decisions. Pure functions (unit-tested); components only measure and render.
// All sizes are CSS pixels of the app's own box, so page zoom and HiDPI scaling shift them naturally.

/** Width and height thresholds of the main window layout (CSS px). */
export const BREAKPOINTS = {
  /** Below this width the category tree becomes an icon rail (or a drawer on request). */
  rail: 720,
  /** Below this width rows use the two-line compact layout and the menu bar folds into one menu button. */
  compact: 600,
  /** Below this height the toolbar shows icons only, whatever the width. */
  shortHeight: 480,
} as const;

// ---------- generic priority fitting ----------

export interface Fittable {
  id: string;
  /** Width the item takes (including its gap/separator). */
  width: number;
  /** Lower = more important. Ignored for pinned items. */
  priority: number;
  /** Never dropped. */
  pinned?: boolean;
}

/**
 * Keeps as many items as fit into `available`, dropping the least important first (highest `priority`; on a tie
 * the later item). Pinned items are always kept, even if they alone overflow. `reserve` is extra width needed as
 * soon as anything is dropped (e.g. an overflow button). Returns the kept and dropped ids, each in input order.
 */
export function fitByPriority(items: Fittable[], available: number, reserve = 0): { kept: string[]; dropped: string[] } {
  const total = items.reduce((s, i) => s + i.width, 0);
  if (total <= available) return { kept: items.map((i) => i.id), dropped: [] };
  const order = items
    .map((it, idx) => ({ it, idx }))
    .filter((x) => !x.it.pinned)
    .sort((a, b) => b.it.priority - a.it.priority || b.idx - a.idx);
  const dropped = new Set<string>();
  let used = total + reserve;
  for (const { it } of order) {
    if (used <= available) break;
    dropped.add(it.id);
    used -= it.width;
  }
  return { kept: items.filter((i) => !dropped.has(i.id)).map((i) => i.id), dropped: items.filter((i) => dropped.has(i.id)).map((i) => i.id) };
}

// ---------- toolbar ----------

export interface ToolMeasure {
  id: string;
  /** Width with icon and label. */
  labeled: number;
  /** Width as an icon-only button. */
  icon: number;
  pinned?: boolean;
  /** Lower = more important (stays longest). */
  priority: number;
}

export interface ToolbarPlan {
  labels: boolean;
  visible: string[];
  overflow: string[];
}

/**
 * Toolbar collapse, in this order: (1) everything with labels; (2) icons only (also forced on short windows);
 * (3) icons only, least important buttons moved into the "More" menu (whose button needs `more` px).
 */
export function planToolbar(items: ToolMeasure[], available: number, o: { more: number; short?: boolean }): ToolbarPlan {
  const ids = items.map((i) => i.id);
  if (!o.short && items.reduce((s, i) => s + i.labeled, 0) <= available) return { labels: true, visible: ids, overflow: [] };
  const fit = fitByPriority(
    items.map((i) => ({ id: i.id, width: i.icon, priority: i.priority, pinned: i.pinned })),
    available,
    o.more,
  );
  return { labels: false, visible: fit.kept, overflow: fit.dropped };
}

/** Toolbar order and priorities. Pinned buttons never leave the bar; the rest leave highest number first. */
export const TOOL_PRIORITY: Record<string, { pinned?: boolean; priority: number }> = {
  add: { pinned: true, priority: 0 },
  resume: { pinned: true, priority: 0 },
  stop: { pinned: true, priority: 0 },
  delete: { pinned: true, priority: 0 },
  options: { pinned: true, priority: 0 },
  stopAll: { priority: 1 },
  clearCompleted: { priority: 2 },
  scheduler: { priority: 3 },
  runQueue: { priority: 4 },
  pauseQueue: { priority: 5 },
  pageScanner: { priority: 6 },
};

// ---------- menu bar ----------

/** The menu bar folds into one menu button on compact widths, or when its items do not fit (large text/zoom). */
export function menuBarCompact(width: number, needed: number): boolean {
  return width < BREAKPOINTS.compact || needed > width;
}

// ---------- category sidebar ----------

export type SideMode = "full" | "rail";
export type Band = "wide" | "narrow";

export interface SideState {
  /** The user's explicit choice, valid while the window stays in `band`. */
  override: SideMode | null;
  band: Band | null;
}

export const bandOf = (width: number): Band => (width < BREAKPOINTS.rail ? "narrow" : "wide");
export const autoSide = (band: Band): SideMode => (band === "wide" ? "full" : "rail");

/** Crossing a breakpoint drops the user's choice; within the same band it wins over the automatic mode. */
export function sideOnResize(s: SideState, band: Band): SideState {
  return s.band === band ? s : { override: null, band };
}

export function resolveSide(s: SideState, band: Band): SideMode {
  return s.band === band && s.override ? s.override : autoSide(band);
}

/** The user toggled the sidebar: remember the choice for the current band. */
export function sideToggle(s: SideState, band: Band): SideState {
  const now = resolveSide(s, band);
  return { override: now === "full" ? "rail" : "full", band };
}

export function sanitizeSide(raw: unknown): SideState {
  const r = (raw && typeof raw === "object" ? raw : {}) as Partial<SideState>;
  const override = r.override === "full" || r.override === "rail" ? r.override : null;
  const band = r.band === "wide" || r.band === "narrow" ? r.band : null;
  return override && band ? { override, band } : { override: null, band: null };
}

export const SIDE_WIDTH = { min: 150, max: 360, default: 200 } as const;

/** Sidebar width: within its own limits and never more than 40% of the window. */
export function clampSideWidth(w: number, windowWidth: number): number {
  const max = Math.max(SIDE_WIDTH.min, Math.min(SIDE_WIDTH.max, Math.floor(windowWidth * 0.4)));
  const v = Number.isFinite(w) ? w : SIDE_WIDTH.default;
  return Math.round(Math.min(max, Math.max(SIDE_WIDTH.min, v)));
}

// ---------- zoom ----------

export const ZOOM_STEPS = [0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2] as const;

export function nextZoom(z: number, dir: 1 | -1 | 0): number {
  if (dir === 0) return 1;
  const steps = ZOOM_STEPS as readonly number[];
  if (dir > 0) return steps.find((s) => s > z + 1e-6) ?? steps.at(-1)!;
  return [...steps].reverse().find((s) => s < z - 1e-6) ?? steps[0];
}

export const sanitizeZoom = (v: unknown): number => ((ZOOM_STEPS as readonly number[]).includes(v as number) ? (v as number) : 1);
