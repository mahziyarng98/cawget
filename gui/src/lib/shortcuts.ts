// Keyboard shortcuts: one table drives both the handler and the Help > Keyboard shortcuts list.
export type ActionId =
  | "add" | "resume" | "stop" | "stopAll" | "delete" | "clearCompleted" | "options" | "open" | "openFolder"
  | "properties" | "selectAll" | "toggleTheme" | "shortcuts" | "about" | "quit" | "focusMenu" | "contextMenu"
  | "nextPane" | "prevPane" | "zoomIn" | "zoomOut" | "zoomReset" | "toggleSidebar";

export interface Shortcut {
  action: ActionId;
  keys: string; // e.g. "Ctrl+Shift+S"
  label: string;
}

export const SHORTCUTS: Shortcut[] = [
  { action: "add", keys: "Ctrl+N", label: "Add download" },
  { action: "resume", keys: "Ctrl+R", label: "Resume selected" },
  { action: "stop", keys: "Ctrl+S", label: "Stop selected" },
  { action: "stopAll", keys: "Ctrl+Shift+S", label: "Stop all" },
  { action: "delete", keys: "Delete", label: "Delete selected" },
  { action: "clearCompleted", keys: "Ctrl+Shift+Delete", label: "Clear completed" },
  { action: "open", keys: "Enter", label: "Open (completed) / progress window (incomplete)" },
  { action: "openFolder", keys: "Ctrl+E", label: "Open folder" },
  { action: "properties", keys: "Alt+Enter", label: "Properties" },
  { action: "selectAll", keys: "Ctrl+A", label: "Select all rows" },
  { action: "options", keys: "Ctrl+,", label: "Options" },
  { action: "toggleTheme", keys: "Ctrl+Shift+L", label: "Switch light / dark theme" },
  { action: "toggleSidebar", keys: "Ctrl+B", label: "Collapse / expand the categories" },
  { action: "zoomIn", keys: "Ctrl+=", label: "Zoom in (also Ctrl++)" },
  { action: "zoomOut", keys: "Ctrl+-", label: "Zoom out" },
  { action: "zoomReset", keys: "Ctrl+0", label: "Actual size" },
  { action: "contextMenu", keys: "Shift+F10", label: "Row menu (also the Menu key)" },
  { action: "focusMenu", keys: "F10", label: "Go to the menu bar (also Alt+F/S/D/V/H)" },
  { action: "nextPane", keys: "F6", label: "Next pane: categories, column headers, download list" },
  { action: "prevPane", keys: "Shift+F6", label: "Previous pane" },
  { action: "shortcuts", keys: "F1", label: "This list" },
  { action: "quit", keys: "Ctrl+Q", label: "Quit" },
];

/** Canonical "Ctrl+Shift+X" form of a keyboard event (Meta counts as Ctrl). */
export function comboOf(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">): string {
  let key = e.key;
  if (key === " ") key = "Space";
  else if (key.length === 1) key = key.toUpperCase();
  if (key === "Del") key = "Delete";
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey && key.length > 1) parts.push("Shift");
  else if (e.shiftKey && /^[A-Z]$/.test(key)) parts.push("Shift");
  parts.push(key);
  return parts.join("+");
}

export function actionFor(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">): ActionId | null {
  const c = comboOf(e);
  if (c === "ContextMenu") return "contextMenu";
  if (c === "Ctrl++") return "zoomIn";
  return SHORTCUTS.find((s) => s.keys === c)?.action ?? null;
}
