import { describe, expect, it } from "vitest";
import { COLUMNS, NAME_MIN, clampWidth, defaultLayout, dropColumn, moveColumn, moveVisible, sanitizeLayout, sortJobs, statusText, toggleHidden, toggleSort, visibleColumns } from "../src/lib/table";
import { job } from "./fixtures";

describe("column layout", () => {
  it("moves columns by keyboard delta and by drop target", () => {
    const o = defaultLayout().order;
    expect(moveColumn(o, "size", -1).indexOf("size")).toBe(o.indexOf("size") - 1);
    expect(moveColumn(o, "name", -5)[0]).toBe("name");
    expect(moveColumn(o, "description", 9).at(-1)).toBe("description");
    const d = dropColumn(o, "rate", "name");
    expect(d[0]).toBe("rate");
    expect(d).toHaveLength(o.length);
    expect(new Set(d).size).toBe(o.length);
  });

  it("clamps widths to the column minimum and a sane maximum", () => {
    expect(clampWidth("name", 10)).toBe(120);
    expect(clampWidth("size", 5000)).toBe(900);
  });

  it("rejects or repairs bad persisted layouts", () => {
    expect(sanitizeLayout(null)).toEqual(defaultLayout());
    expect(sanitizeLayout({ order: ["name"] }).order).toEqual(defaultLayout().order);
    const r = sanitizeLayout({ order: [...defaultLayout().order].reverse(), widths: { name: 5, size: "x" }, sort: { col: "lastTry", dir: 1 } });
    expect(r.order[0]).toBe("description");
    expect(r.widths.name).toBe(120);
    expect(r.widths.size).toBe(COLUMNS.find((c) => c.id === "size")!.width);
    expect(r.sort).toBeNull(); // an unsupported column cannot be the sort key
  });
});

describe("sorting", () => {
  it("cycles ascending/descending and ignores unsupported columns", () => {
    expect(toggleSort(null, "size")).toEqual({ col: "size", dir: 1 });
    expect(toggleSort({ col: "size", dir: 1 }, "size")).toEqual({ col: "size", dir: -1 });
    expect(toggleSort({ col: "size", dir: -1 }, "name")).toEqual({ col: "name", dir: 1 });
    expect(toggleSort(null, "description")).toBeNull();
  });

  it("orders rows, unknown values always last, stable by id", () => {
    const a = job({ id: 1, name: "b.zip", size: 300 });
    const b = job({ id: 2, name: "A.iso", size: null });
    const c = job({ id: 3, name: "c.pdf", size: 100 });
    expect(sortJobs([a, b, c], null).map((j) => j.id)).toEqual([3, 2, 1]);
    expect(sortJobs([a, b, c], { col: "name", dir: 1 }).map((j) => j.id)).toEqual([2, 1, 3]);
    expect(sortJobs([a, b, c], { col: "size", dir: 1 }).map((j) => j.id)).toEqual([3, 1, 2]);
    expect(sortJobs([a, b, c], { col: "size", dir: -1 }).map((j) => j.id)).toEqual([1, 3, 2]);
  });

  it("sorts status by state then progress", () => {
    const done = job({ id: 1, state: "done" });
    const act = job({ id: 2, state: "active", fraction: 0.5 });
    const act2 = job({ id: 3, state: "active", fraction: 0.2 });
    expect(sortJobs([done, act, act2], { col: "status", dir: 1 }).map((j) => j.id)).toEqual([3, 2, 1]);
  });
});

describe("status text", () => {
  it("describes every state", () => {
    expect(statusText(job({ state: "active", fraction: 0.4567, connections: 3, done: 5 }), 0)).toBe("45.7%");
    expect(statusText(job({ state: "active" }), 0)).toBe("Connecting…");
    expect(statusText(job({ state: "paused", fraction: 0.1 }), 0)).toBe("Stopped (10.0%)");
    expect(statusText(job({ state: "done" }), 0)).toBe("Completed");
    expect(statusText(job({ state: "failed" }), 0)).toBe("Failed");
    expect(statusText(job({ state: "queued" }), 0)).toBe("Queued");
    expect(statusText(job({ state: "queued", not_before: 2_000_000_000 }), 1_000)).toMatch(/^Scheduled /);
  });
});

describe("column auto-hide and user visibility", () => {
  // default widths: queue 64, size 90, status 210, eta 84, rate 90, lastTry 120, description 200 (= 858) + name min 160
  const all = defaultLayout();

  it("shows every column when there is room, with the name taking the rest", () => {
    expect(visibleColumns(all, NAME_MIN + 858)).toEqual(all.order);
  });

  it("hides Notes, Last attempt and Queue first, then Speed, Time left, Size, Status; never the name", () => {
    expect(visibleColumns(all, NAME_MIN + 857)).not.toContain("description");
    const at = (w: number) => visibleColumns(all, w);
    expect(at(NAME_MIN + 658)).toEqual(["name", "queue", "size", "status", "eta", "rate", "lastTry"]);
    expect(at(NAME_MIN + 537)).toEqual(["name", "size", "status", "eta", "rate"]);
    expect(at(NAME_MIN + 538)).toEqual(["name", "queue", "size", "status", "eta", "rate"]);
    expect(at(NAME_MIN + 474)).toEqual(["name", "size", "status", "eta", "rate"]);
    expect(at(NAME_MIN + 384)).toEqual(["name", "size", "status", "eta"]);
    expect(at(NAME_MIN + 300)).toEqual(["name", "size", "status"]);
    expect(at(NAME_MIN + 210)).toEqual(["name", "status"]);
    expect(at(100)).toEqual(["name"]);
  });

  it("keeps the user's order and never changes the stored layout", () => {
    const l = { ...all, order: dropColumn(all.order, "rate", "name") };
    const before = JSON.stringify(l);
    expect(visibleColumns(l, NAME_MIN + 300)[0]).toBe("name");
    expect(visibleColumns(l, 5000)[0]).toBe("rate");
    expect(JSON.stringify(l)).toBe(before);
  });

  it("user-hidden columns stay hidden, and free room for the others", () => {
    const l = toggleHidden(toggleHidden(all, "status"), "name");
    expect(l.hidden).toEqual(["status"]);
    expect(visibleColumns(l, 5000)).not.toContain("status");
    // 160 + 648 without Status; Notes, Last attempt and Queue go, the rest fits into 460.
    expect(visibleColumns(l, NAME_MIN + 300)).toEqual(["name", "size", "eta", "rate"]);
    expect(toggleHidden(l, "status").hidden).toEqual([]);
  });

  it("reorders among the visible columns (hidden ones are skipped)", () => {
    const vis = ["name", "size", "status"] as const;
    const o = moveVisible(all.order, [...vis], "status", -1);
    expect(o.filter((c) => (vis as readonly string[]).includes(c))).toEqual(["name", "status", "size"]);
    expect(o).toHaveLength(all.order.length);
    expect(moveVisible(all.order, [...vis], "name", -1)).toEqual(all.order);
  });

  it("restores the hidden list from storage, without the name or unknown ids", () => {
    expect(sanitizeLayout({ ...all, hidden: ["name", "rate", "bogus"] }).hidden).toEqual(["rate"]);
    expect(sanitizeLayout({ order: all.order }).hidden).toEqual([]);
  });
});
