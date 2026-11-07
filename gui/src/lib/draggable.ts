// Move a dialog by dragging its title bar.
//
// The offset is applied with the CSS `translate` property, so the dialog keeps its place in the layout (the scrim
// still centres it) and nothing that reads `transform` is affected. The offset lives in this action, not in component
// state, so re-renders of the dialog (the progress dialog updates continuously) never reset it. A new dialog starts
// centred again because each opening mounts a new frame.
//
// Dragging starts only on the handle itself, never on a control inside it (the close button), and uses pointer
// capture so mouse, pen and touch all work. The handle never takes the keyboard focus.

/** How much of the title bar always stays inside the window, in CSS pixels. */
export const KEEP_VISIBLE = 48;

/** Pressing on one of these inside the handle never starts a drag. */
export const NO_DRAG =
  "button, input, select, textarea, a, label, table, [role=tab], [role=tablist], [contenteditable], [data-no-drag]";

/** A rectangle in viewport coordinates. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/**
 * The nearest allowed offset to (`x`, `y`) for a dialog whose untranslated box is `base` and whose title bar ends
 * `header` pixels below the dialog's top edge, in a `vw` x `vh` window: at least `keep` pixels of the title bar stay inside horizontally,
 * the top of the title bar never leaves the window, and at least `keep` pixels of it (or all of it, if lower) stay
 * above the bottom edge.
 */
export function clampOffset(
  x: number,
  y: number,
  base: Box,
  header: number,
  vw: number,
  vh: number,
  keep = KEEP_VISIBLE,
): { x: number; y: number } {
  const kx = Math.min(keep, base.width);
  const ky = Math.min(keep, header);
  return {
    x: clamp(x, kx - base.left - base.width, vw - kx - base.left),
    // The lower bound is applied last, so the top of the title bar wins in a window too short for both.
    // (`0 - top` rather than `-top`, which gives -0 for a dialog at the top edge.)
    y: Math.max(0 - base.top, Math.min(vh - ky - base.top, y)),
  };
}

export interface DraggableOptions {
  /** Selector of the drag handle inside the node (the title bar). */
  handle: string;
}

export function draggable(node: HTMLElement, options: DraggableOptions) {
  let opts = options;
  let x = 0;
  let y = 0;
  let drag: { id: number; px: number; py: number; x: number; y: number; handle: HTMLElement } | null = null;
  const win = node.ownerDocument.defaultView ?? window;

  const apply = () => {
    node.style.translate = x || y ? `${x}px ${y}px` : "";
  };

  /** Moves to the nearest allowed offset; measures every time, as the dialog may grow or the window change. */
  function moveTo(nx: number, ny: number) {
    const r = node.getBoundingClientRect();
    const base = { left: r.left - x, top: r.top - y, width: r.width, height: r.height };
    const h = node.querySelector<HTMLElement>(opts.handle)?.getBoundingClientRect();
    const header = h ? h.bottom - r.top : 0; // includes the frame's border above the title bar
    ({ x, y } = clampOffset(nx, ny, base, header, win.innerWidth, win.innerHeight));
    apply();
  }

  /** The handle under the event, if a drag may start there. */
  function handleAt(e: PointerEvent | MouseEvent): HTMLElement | null {
    const t = e.target as Element | null;
    const h = t?.closest?.<HTMLElement>(opts.handle);
    if (!t || !h || !node.contains(h)) return null;
    const blocked = t.closest(NO_DRAG);
    return blocked && node.contains(blocked) ? null : h;
  }

  function down(e: PointerEvent) {
    if (drag || !e.isPrimary || (e.pointerType === "mouse" && e.button !== 0)) return;
    const h = handleAt(e);
    if (!h) return;
    e.preventDefault(); // no text selection and no focus change
    h.setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, px: e.clientX, py: e.clientY, x, y, handle: h };
    node.classList.add("dragging");
  }

  function move(e: PointerEvent) {
    if (drag?.id !== e.pointerId) return;
    moveTo(drag.x + e.clientX - drag.px, drag.y + e.clientY - drag.py);
  }

  function up(e: PointerEvent) {
    if (drag?.id !== e.pointerId) return;
    if (drag.handle.hasPointerCapture?.(e.pointerId)) drag.handle.releasePointerCapture(e.pointerId);
    drag = null;
    node.classList.remove("dragging");
  }

  // The mouse events that follow a pointer press would focus the dialog frame and start a selection.
  function mousedown(e: MouseEvent) {
    if (handleAt(e)) e.preventDefault();
  }

  const refit = () => {
    if (x || y) moveTo(x, y);
  };
  const resized = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(refit);
  resized?.observe(node);

  node.addEventListener("pointerdown", down);
  node.addEventListener("pointermove", move);
  node.addEventListener("pointerup", up);
  node.addEventListener("pointercancel", up);
  node.addEventListener("lostpointercapture", up);
  node.addEventListener("mousedown", mousedown);
  win.addEventListener("resize", refit);

  return {
    update(next: DraggableOptions) {
      opts = next;
    },
    destroy() {
      resized?.disconnect();
      node.removeEventListener("pointerdown", down);
      node.removeEventListener("pointermove", move);
      node.removeEventListener("pointerup", up);
      node.removeEventListener("pointercancel", up);
      node.removeEventListener("lostpointercapture", up);
      node.removeEventListener("mousedown", mousedown);
      win.removeEventListener("resize", refit);
      node.classList.remove("dragging");
      node.style.translate = "";
    },
  };
}
