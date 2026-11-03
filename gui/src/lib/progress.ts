// Numbers for the per-download progress dialog (unit-tested).
import type { Job, Part } from "./api";

/** Per-download choices in the "When completed" tab (kept for this session only). */
export interface CompletionOptions {
  openFolder: boolean;
  exitApp: boolean;
}

export interface SegmentRow {
  index: number;
  start: number;
  end: number;
  length: number;
  done: number;
  /** 0..100 */
  percent: number;
  complete: boolean;
}

/** One row per saved segment, in file order. */
export function segmentRows(parts: Part[]): SegmentRow[] {
  return [...parts]
    .sort((a, b) => a.start - b.start)
    .map((p, index) => {
      const length = p.end - p.start + 1;
      const done = Math.min(p.done, length);
      return { index: index + 1, start: p.start, end: p.end, length, done, percent: length > 0 ? (done / length) * 100 : 100, complete: done >= length };
    });
}

/** Overall percentage, or null when the size is unknown. */
export function overallPercent(j: Pick<Job, "state" | "size" | "done" | "fraction">): number | null {
  if (j.state === "done") return 100;
  if (j.fraction != null) return j.fraction * 100;
  if (j.size && j.size > 0) return Math.min(100, (j.done / j.size) * 100);
  return null;
}

/** "Yes" / "No" / "Unknown" for resume support, as far as the engine knows. */
export function resumeLabel(j: Pick<Job, "size" | "accept_ranges">): string {
  if (j.size == null) return "Unknown (not contacted yet)";
  return j.accept_ranges ? "Yes" : "No (the server does not support ranges)";
}
