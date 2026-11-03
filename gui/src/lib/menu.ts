import type { IconName } from "./Icon.svelte";

/** One entry of a menu. Items without backend support are `disabled` with a `reason` shown as tooltip and inline. */
export type MenuItem =
  | {
      kind: "item";
      id: string;
      label: string;
      shortcut?: string;
      icon?: IconName;
      disabled?: boolean;
      reason?: string;
      /** For check/radio items. */
      checked?: boolean;
      role?: "check" | "radio";
      run?: () => void;
    }
  | { kind: "sep" }
  /** A non-interactive heading (used when all menus are folded into one). */
  | { kind: "group"; label: string };

export const sep: MenuItem = { kind: "sep" };
export const soon = (id: string, label: string, icon?: IconName): MenuItem => ({ kind: "item", id, label, icon, disabled: true, reason: "Coming soon" });

/** A top-level menu of the menu bar. */
export interface TopMenu {
  id: string;
  label: string;
  /** Letter used with Alt and underlined in the label. */
  mnemonic: string;
  items: () => MenuItem[];
}

/** A toolbar button. */
export interface ToolButton {
  id: string;
  label: string;
  icon: IconName;
  run?: () => void;
  disabled?: boolean;
  reason?: string;
  title?: string;
  /** Draw a separator after this button. */
  groupEnd?: boolean;
}

/**
 * All top menus folded into one list (compact menu button): a heading per menu, then its items, with a separator
 * between menus. `first[id]` is the index (among selectable entries, as the menu counts them for focus) of each
 * menu's first item, so Alt+mnemonic can jump straight to its group.
 */
export function foldMenus(menus: TopMenu[]): { items: MenuItem[]; first: Record<string, number> } {
  const items: MenuItem[] = [];
  const first: Record<string, number> = {};
  let selectable = 0;
  menus.forEach((m, i) => {
    if (i > 0) items.push(sep);
    items.push({ kind: "group", label: m.label });
    first[m.id] = selectable;
    for (const it of m.items()) {
      items.push(it);
      if (it.kind === "item") selectable++;
    }
  });
  return { items, first };
}
