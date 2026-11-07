import { describe, expect, it } from "vitest";
import { type Box, clampOffset, draggable, KEEP_VISIBLE } from "../src/lib/draggable";

// The tests run without a browser DOM, so the action is driven through a small fake of the parts it uses:
// listeners, closest/contains/querySelector, bounding boxes (the dialog's follows its `translate`) and pointer capture.

type Listener = (e: never) => void;
class Target {
  listeners = new Map<string, Set<Listener>>();
  addEventListener(type: string, fn: Listener) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type)!.add(fn);
  }
  removeEventListener(type: string, fn: Listener) {
    this.listeners.get(type)?.delete(fn);
  }
  count() {
    return [...this.listeners.values()].reduce((n, s) => n + s.size, 0);
  }
}

class FakeWindow extends Target {
  constructor(public innerWidth: number, public innerHeight: number) {
    super();
  }
  resize(w: number, h: number) {
    this.innerWidth = w;
    this.innerHeight = h;
    for (const fn of this.listeners.get("resize") ?? []) (fn as () => void)();
  }
}

// One simple selector: tag, .class, [attr] or [attr=value].
function matches(el: FakeEl, part: string): boolean {
  if (part.startsWith(".")) return el.classes.has(part.slice(1));
  const attr = /^\[([\w-]+)(?:=([\w-]+))?\]$/.exec(part);
  if (attr) return attr[1] in el.attrs && (attr[2] === undefined || el.attrs[attr[1]] === attr[2]);
  return el.tag === part;
}

class FakeEl extends Target {
  classes: Set<string>;
  children: FakeEl[] = [];
  parent: FakeEl | null = null;
  style = { translate: "" };
  captured = new Set<number>();
  classList = {
    add: (c: string) => this.classes.add(c),
    remove: (c: string) => this.classes.delete(c),
    contains: (c: string) => this.classes.has(c),
  };
  ownerDocument: { defaultView: FakeWindow };

  constructor(
    public tag: string,
    classes: string[],
    win: FakeWindow,
    public box: Box = { left: 0, top: 0, width: 0, height: 0 },
    public attrs: Record<string, string> = {},
  ) {
    super();
    this.classes = new Set(classes);
    this.ownerDocument = { defaultView: win };
  }
  add(child: FakeEl) {
    child.parent = this;
    this.children.push(child);
    return child;
  }
  is(sel: string) {
    return sel.split(",").some((p) => matches(this, p.trim()));
  }
  closest(sel: string): FakeEl | null {
    for (let e: FakeEl | null = this; e; e = e.parent) if (e.is(sel)) return e;
    return null;
  }
  contains(other: FakeEl | null) {
    for (let e = other; e; e = e.parent) if (e === this) return true;
    return false;
  }
  querySelector(sel: string): FakeEl | null {
    for (const c of this.children) {
      if (c.is(sel)) return c;
      const d = c.querySelector(sel);
      if (d) return d;
    }
    return null;
  }
  getBoundingClientRect() {
    // A translated element moves its children with it.
    let dx = 0;
    let dy = 0;
    for (let e: FakeEl | null = this; e; e = e.parent) {
      const [x = 0, y = 0] = e.style.translate ? e.style.translate.split(" ").map(parseFloat) : [];
      dx += x;
      dy += y;
    }
    const top = this.box.top + dy;
    return { ...this.box, left: this.box.left + dx, top, bottom: top + this.box.height };
  }
  setPointerCapture(id: number) {
    this.captured.add(id);
  }
  hasPointerCapture(id: number) {
    return this.captured.has(id);
  }
  releasePointerCapture(id: number) {
    this.captured.delete(id);
  }
}

/** A 400x300 dialog centred in a 1000x800 window, as Modal renders it. */
function setup() {
  const win = new FakeWindow(1000, 800);
  const dialog = new FakeEl("div", ["modal"], win, { left: 300, top: 250, width: 400, height: 300 });
  const header = dialog.add(new FakeEl("div", ["modal-title"], win, { left: 301, top: 251, width: 398, height: 32 }));
  const text = header.add(new FakeEl("span", ["modal-title-text"], win));
  const close = header.add(new FakeEl("button", ["icon-btn"], win));
  const icon = close.add(new FakeEl("svg", ["icon"], win));
  const body = dialog.add(new FakeEl("div", ["modal-body"], win));
  const tab = body.add(new FakeEl("button", ["tab"], win, undefined, { role: "tab" }));
  const action = draggable(dialog as unknown as HTMLElement, { handle: ".modal-title" });

  function fire(type: string, target: FakeEl, x = 0, y = 0, extra: Record<string, unknown> = {}) {
    const e = {
      type, target, clientX: x, clientY: y, pointerId: 1, isPrimary: true, pointerType: "mouse", button: 0,
      defaultPrevented: false,
      preventDefault() { this.defaultPrevented = true; },
      ...extra,
    };
    for (const fn of dialog.listeners.get(type) ?? []) (fn as (e: unknown) => void)(e);
    return e;
  }
  /** Press at (500, 260) on `target`, move by (dx, dy), release. */
  function dragBy(target: FakeEl, dx: number, dy: number, extra: Record<string, unknown> = {}) {
    const down = fire("pointerdown", target, 500, 260, extra);
    fire("pointermove", target, 500 + dx, 260 + dy, extra);
    fire("pointerup", target, 500 + dx, 260 + dy, extra);
    return down;
  }
  return { win, dialog, header, text, close, icon, tab, action, fire, dragBy };
}

describe("clampOffset", () => {
  const base = { left: 300, top: 250, width: 400, height: 300 };

  it("leaves offsets inside the window alone", () => {
    expect(clampOffset(120, -80, base, 32, 1000, 800)).toEqual({ x: 120, y: -80 });
  });

  it("keeps part of the title bar inside on every side", () => {
    // Left: the dialog's right edge stays KEEP_VISIBLE px inside; right: its left edge stays that far from the edge.
    expect(clampOffset(-5000, 0, base, 32, 1000, 800).x).toBe(KEEP_VISIBLE - 300 - 400);
    expect(clampOffset(5000, 0, base, 32, 1000, 800).x).toBe(1000 - KEEP_VISIBLE - 300);
    // Top: the title bar never goes above the window. Bottom: the whole 32 px title bar stays visible.
    expect(clampOffset(0, -5000, base, 32, 1000, 800).y).toBe(-250);
    expect(clampOffset(0, 5000, base, 32, 1000, 800).y).toBe(800 - 32 - 250);
  });

  it("uses the dialog's width when it is narrower than the margin", () => {
    expect(clampOffset(-5000, 0, { ...base, width: 30 }, 32, 1000, 800).x).toBe(-300);
  });

  it("keeps the top of the title bar visible in a window too short for the rule", () => {
    expect(clampOffset(0, 5000, { left: 0, top: 0, width: 400, height: 300 }, 32, 400, 20).y).toBe(0);
  });
});

describe("draggable", () => {
  it("moves the dialog when its title bar is dragged", () => {
    const { dialog, header, text, fire } = setup();
    const down = fire("pointerdown", text, 500, 260);
    expect(down.defaultPrevented).toBe(true); // no selection, no focus change
    expect(header.captured.has(1)).toBe(true);
    expect(dialog.classes.has("dragging")).toBe(true);

    fire("pointermove", text, 600, 310);
    expect(dialog.style.translate).toBe("100px 50px");

    fire("pointerup", text, 600, 310);
    expect(header.captured.size).toBe(0);
    expect(dialog.classes.has("dragging")).toBe(false);
    fire("pointermove", text, 700, 400); // released: no longer follows
    expect(dialog.style.translate).toBe("100px 50px");
  });

  it("continues from where the last drag left it", () => {
    const { dialog, header, dragBy } = setup();
    dragBy(header, 40, 10);
    dragBy(header, 20, -30);
    expect(dialog.style.translate).toBe("60px -20px");
  });

  it("does not start on the close button, a tab or the dialog body", () => {
    const { dialog, close, icon, tab, fire, dragBy } = setup();
    for (const target of [close, icon, tab, dialog.children[1]]) {
      const down = dragBy(target, 100, 100);
      expect(down.defaultPrevented).toBe(false);
      expect(fire("mousedown", target).defaultPrevented).toBe(false);
    }
    expect(dialog.style.translate).toBe("");
    expect(dialog.classes.has("dragging")).toBe(false);
  });

  it("keeps the mouse press on the title bar from focusing or selecting", () => {
    const { text, fire } = setup();
    expect(fire("mousedown", text).defaultPrevented).toBe(true);
  });

  it("works with touch and pen, but not with other mouse buttons or extra fingers", () => {
    const { dialog, header, dragBy } = setup();
    dragBy(header, 10, 10, { button: 2 });
    dragBy(header, 10, 10, { pointerType: "touch", isPrimary: false });
    expect(dialog.style.translate).toBe("");
    dragBy(header, 10, 0, { pointerType: "touch" });
    dragBy(header, 0, 10, { pointerType: "pen" });
    expect(dialog.style.translate).toBe("10px 10px");
  });

  it("ignores moves of another pointer during a drag", () => {
    const { dialog, header, fire } = setup();
    fire("pointerdown", header, 500, 260);
    fire("pointermove", header, 900, 900, { pointerId: 2 });
    expect(dialog.style.translate).toBe("");
  });

  it("clamps while dragging", () => {
    const { dialog, header, dragBy } = setup();
    dragBy(header, -5000, -5000);
    expect(dialog.style.translate).toBe(`${KEEP_VISIBLE - 700}px -250px`);
  });

  it("keeps the whole title bar, including the frame's border above it, inside the bottom edge", () => {
    const { dialog, header, dragBy } = setup();
    dragBy(header, 0, 5000);
    // The title bar ends 33 px below the dialog's top (1 px border + 32 px), so the dialog's top stops at 800 - 33.
    expect(dialog.getBoundingClientRect().top).toBe(800 - 33);
    expect(header.getBoundingClientRect().top + 32).toBeLessThanOrEqual(800);
  });

  it("re-clamps when the window shrinks", () => {
    const { win, dialog, header, dragBy } = setup();
    dragBy(header, 600, 0); // left edge at 900 of 1000
    // The scrim re-centres the dialog in the smaller window, then the resize event arrives.
    dialog.box = { left: 100, top: 150, width: 400, height: 300 };
    win.resize(600, 600);
    expect(dialog.style.translate).toBe(`${600 - KEEP_VISIBLE - 100}px 0px`);
  });

  it("keeps its position across updates and starts centred on the next opening", () => {
    const first = setup();
    first.dragBy(first.header, 80, 40);
    first.action.update({ handle: ".modal-title" }); // a re-render of the dialog
    expect(first.dialog.style.translate).toBe("80px 40px");

    first.action.destroy(); // closed
    expect(first.dialog.style.translate).toBe("");
    expect(first.dialog.count()).toBe(0);
    expect(first.win.count()).toBe(0);

    const second = setup(); // opened again: a new frame
    expect(second.dialog.style.translate).toBe("");
    second.dragBy(second.header, 10, 0);
    expect(second.dialog.style.translate).toBe("10px 0px");
  });
});
