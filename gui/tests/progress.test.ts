import { describe, expect, it } from "vitest";
import { overallPercent, resumeLabel, segmentRows } from "../src/lib/progress";

describe("progress dialog numbers", () => {
  it("orders segments by offset and computes per-segment progress", () => {
    const rows = segmentRows([
      { start: 500, end: 999, done: 500 },
      { start: 0, end: 499, done: 100 },
    ]);
    expect(rows.map((r) => [r.index, r.start, r.length, r.percent, r.complete])).toEqual([
      [1, 0, 500, 20, false],
      [2, 500, 500, 100, true],
    ]);
  });

  it("never reports more than the segment length", () => {
    expect(segmentRows([{ start: 0, end: 9, done: 25 }])[0]).toMatchObject({ done: 10, percent: 100 });
  });

  it("overall percent and resume labels", () => {
    expect(overallPercent({ state: "done", size: null, done: 0, fraction: null })).toBe(100);
    expect(overallPercent({ state: "active", size: 200, done: 50, fraction: 0.25 })).toBe(25);
    expect(overallPercent({ state: "active", size: null, done: 50, fraction: null })).toBeNull();
    expect(resumeLabel({ size: null, accept_ranges: false })).toMatch(/Unknown/);
    expect(resumeLabel({ size: 5, accept_ranges: true })).toBe("Yes");
    expect(resumeLabel({ size: 5, accept_ranges: false })).toMatch(/^No/);
  });
});
