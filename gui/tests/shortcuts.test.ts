import { describe, expect, it } from "vitest";
import { SHORTCUTS, actionFor, comboOf } from "../src/lib/shortcuts";

const ev = (key: string, m: { ctrl?: boolean; shift?: boolean; alt?: boolean } = {}) => ({ key, ctrlKey: !!m.ctrl, metaKey: false, shiftKey: !!m.shift, altKey: !!m.alt });

describe("shortcuts", () => {
  it("normalises key events", () => {
    expect(comboOf(ev("n", { ctrl: true }))).toBe("Ctrl+N");
    expect(comboOf(ev("S", { ctrl: true, shift: true }))).toBe("Ctrl+Shift+S");
    expect(comboOf(ev("Delete", { ctrl: true, shift: true }))).toBe("Ctrl+Shift+Delete");
    expect(comboOf(ev("Enter", { alt: true }))).toBe("Alt+Enter");
    expect(comboOf(ev(",", { ctrl: true }))).toBe("Ctrl+,");
  });

  it("maps to actions and has no duplicate bindings", () => {
    expect(actionFor(ev("n", { ctrl: true }))).toBe("add");
    expect(actionFor(ev("Delete"))).toBe("delete");
    expect(actionFor(ev("F10", { shift: true }))).toBe("contextMenu");
    expect(actionFor(ev("ContextMenu"))).toBe("contextMenu");
    expect(actionFor(ev("x"))).toBeNull();
    expect(new Set(SHORTCUTS.map((s) => s.keys)).size).toBe(SHORTCUTS.length);
  });
});

describe("pane cycling keys", () => {
  it("F6 and Shift+F6 are bound", () => {
    expect(actionFor(ev("F6"))).toBe("nextPane");
    expect(actionFor(ev("F6", { shift: true }))).toBe("prevPane");
  });
});
