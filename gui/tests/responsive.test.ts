import { describe, expect, it } from "vitest";
import { type MenuItem, type TopMenu, foldMenus, sep } from "../src/lib/menu";
import {
  BREAKPOINTS,
  SIDE_WIDTH,
  TOOL_PRIORITY,
  type ToolMeasure,
  autoSide,
  bandOf,
  clampSideWidth,
  fitByPriority,
  menuBarCompact,
  nextZoom,
  planToolbar,
  resolveSide,
  sanitizeSide,
  sanitizeZoom,
  sideOnResize,
  sideToggle,
} from "../src/lib/responsive";

describe("fitByPriority", () => {
  const items = [
    { id: "a", width: 100, priority: 0, pinned: true },
    { id: "b", width: 100, priority: 3 },
    { id: "c", width: 100, priority: 1 },
    { id: "d", width: 100, priority: 3 },
  ];
  it("keeps everything when it fits", () => {
    expect(fitByPriority(items, 400)).toEqual({ kept: ["a", "b", "c", "d"], dropped: [] });
  });
  it("drops the least important first (later item on a tie), keeping input order", () => {
    expect(fitByPriority(items, 399)).toEqual({ kept: ["a", "b", "c"], dropped: ["d"] });
    expect(fitByPriority(items, 250)).toEqual({ kept: ["a", "c"], dropped: ["b", "d"] });
  });
  it("counts the reserve once anything is dropped, and never drops pinned items", () => {
    expect(fitByPriority(items, 350, 60).kept).toEqual(["a", "c"]);
    expect(fitByPriority(items, 10).kept).toEqual(["a"]);
  });
});

describe("toolbar plan", () => {
  const ids = ["add", "resume", "stop", "stopAll", "delete", "clearCompleted", "options", "scheduler", "runQueue", "pauseQueue", "pageScanner"];
  const items: ToolMeasure[] = ids.map((id) => ({ id, labeled: 70, icon: 36, ...TOOL_PRIORITY[id] }));
  const labeledTotal = 70 * ids.length; // 770
  const iconTotal = 36 * ids.length; // 396

  it("shows labels when everything fits", () => {
    expect(planToolbar(items, labeledTotal, { more: 36 })).toEqual({ labels: true, visible: ids, overflow: [] });
  });
  it("hides labels before moving anything into the overflow menu", () => {
    expect(planToolbar(items, labeledTotal - 1, { more: 36 })).toEqual({ labels: false, visible: ids, overflow: [] });
    expect(planToolbar(items, iconTotal, { more: 36 }).overflow).toEqual([]);
  });
  it("short windows are icon-only even when labels would fit", () => {
    expect(planToolbar(items, 2000, { more: 36, short: true }).labels).toBe(false);
  });
  it("moves buttons out in priority order: Page scanner, Pause/Run queue, Scheduler, Clear completed, Stop all", () => {
    const out = (w: number) => planToolbar(items, w, { more: 36 }).overflow;
    // The More button costs as much as one button, so the first step moves two (one alone would save nothing).
    expect(out(iconTotal - 1)).toEqual(["pauseQueue", "pageScanner"]);
    expect(out(iconTotal - 37)).toEqual(["runQueue", "pauseQueue", "pageScanner"]);
    expect(new Set(out(250))).toEqual(new Set(["stopAll", "clearCompleted", "scheduler", "runQueue", "pauseQueue", "pageScanner"]));
  });
  it("always keeps Add, Resume, Stop, Delete and Options", () => {
    const p = planToolbar(items, 50, { more: 36 });
    expect(p.visible).toEqual(["add", "resume", "stop", "delete", "options"]);
    expect(p.overflow).toHaveLength(6);
  });
});

describe("menu bar", () => {
  it("folds below the compact breakpoint or when its items do not fit", () => {
    expect(menuBarCompact(BREAKPOINTS.compact - 1, 300)).toBe(true);
    expect(menuBarCompact(BREAKPOINTS.compact, 300)).toBe(false);
    expect(menuBarCompact(900, 901)).toBe(true);
  });
  it("folds all menus into one list with group headings and the first item of each group", () => {
    const item = (id: string): MenuItem => ({ kind: "item", id, label: id });
    const menus: TopMenu[] = [
      { id: "a", label: "A", mnemonic: "A", items: () => [item("a1"), sep, item("a2")] },
      { id: "b", label: "B", mnemonic: "B", items: () => [item("b1")] },
    ];
    const f = foldMenus(menus);
    expect(f.items.map((i) => (i.kind === "item" ? i.id : i.kind === "group" ? `#${i.label}` : "-"))).toEqual(["#A", "a1", "-", "a2", "-", "#B", "b1"]);
    expect(f.first).toEqual({ a: 0, b: 2 });
  });
});

describe("category sidebar", () => {
  const narrow = BREAKPOINTS.rail - 1;
  it("is full on wide windows and a rail on narrow ones", () => {
    expect(bandOf(BREAKPOINTS.rail)).toBe("wide");
    expect(bandOf(narrow)).toBe("narrow");
    expect(autoSide("wide")).toBe("full");
    expect(autoSide("narrow")).toBe("rail");
  });
  it("the user's choice wins until the window crosses the breakpoint", () => {
    let s = sideOnResize({ override: null, band: null }, "wide");
    expect(resolveSide(s, "wide")).toBe("full");
    s = sideToggle(s, "wide"); // user collapses on a wide window
    expect(resolveSide(s, "wide")).toBe("rail");
    s = sideOnResize(s, "wide"); // resizing within the band keeps it
    expect(resolveSide(s, "wide")).toBe("rail");
    s = sideOnResize(s, "narrow"); // crossing the breakpoint: automatic again
    expect(s.override).toBeNull();
    expect(resolveSide(s, "narrow")).toBe("rail");
    s = sideToggle(s, "narrow"); // user opens the full tree on a narrow window (drawer)
    expect(resolveSide(s, "narrow")).toBe("full");
    s = sideOnResize(s, "wide");
    expect(resolveSide(s, "wide")).toBe("full");
    expect(s.override).toBeNull();
  });
  it("sideOnResize returns the same object while the band does not change (no update loop)", () => {
    const s = { override: "rail" as const, band: "wide" as const };
    expect(sideOnResize(s, "wide")).toBe(s);
  });
  it("persists the override through JSON and rejects bad stored data", () => {
    const s = sideToggle({ override: null, band: "wide" }, "wide");
    const back = sanitizeSide(JSON.parse(JSON.stringify(s)));
    expect(back).toEqual({ override: "rail", band: "wide" });
    expect(resolveSide(back, "wide")).toBe("rail"); // still wins after a restart in the same band
    expect(resolveSide(sideOnResize(back, "narrow"), "narrow")).toBe("rail");
    expect(sanitizeSide(null)).toEqual({ override: null, band: null });
    expect(sanitizeSide({ override: "huge", band: "wide" })).toEqual({ override: null, band: null });
    expect(sanitizeSide({ override: "full" })).toEqual({ override: null, band: null });
  });
  it("clamps the splitter width to its limits and to 40% of the window", () => {
    expect(clampSideWidth(10, 1920)).toBe(SIDE_WIDTH.min);
    expect(clampSideWidth(999, 1920)).toBe(SIDE_WIDTH.max);
    expect(clampSideWidth(300, 600)).toBe(240);
    expect(clampSideWidth(300, 300)).toBe(SIDE_WIDTH.min);
    expect(clampSideWidth(Number.NaN, 1920)).toBe(SIDE_WIDTH.default);
  });
});

describe("zoom", () => {
  it("steps through 100, 110, 125, 150 % and back, with limits and reset", () => {
    expect(nextZoom(1, 1)).toBe(1.1);
    expect(nextZoom(1.1, 1)).toBe(1.25);
    expect(nextZoom(1.25, 1)).toBe(1.5);
    expect(nextZoom(1.5, -1)).toBe(1.25);
    expect(nextZoom(2, 1)).toBe(2);
    expect(nextZoom(0.8, -1)).toBe(0.8);
    expect(nextZoom(1.5, 0)).toBe(1);
    expect(nextZoom(1.2, 1)).toBe(1.25); // off-grid values snap to the next step
  });
  it("accepts only known steps from storage", () => {
    expect(sanitizeZoom(1.25)).toBe(1.25);
    expect(sanitizeZoom(7)).toBe(1);
    expect(sanitizeZoom("1.5")).toBe(1);
  });
});
