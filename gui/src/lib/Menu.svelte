<script lang="ts">
  // A popup menu (dropdown of the menu bar, the folded menu, the toolbar overflow or the row context menu).
  // Keyboard: Up/Down/Home/End move, Enter/Space activate, Escape closes, Left/Right are passed to the menu bar.
  // Taller than the window, it scrolls; the focused item is kept in view.
  import { onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import type { MenuItem } from "./menu";

  let {
    items,
    x,
    y,
    label,
    focusIndex = 0,
    onclose,
    onleft,
    onright,
  }: {
    items: MenuItem[];
    x: number;
    y: number;
    label: string;
    /** Which selectable entry gets the focus first. */
    focusIndex?: number;
    onclose: (refocus: boolean) => void;
    onleft?: () => void;
    onright?: () => void;
  } = $props();

  let el = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 0, y: 0 });

  const buttons = () => Array.from(el?.querySelectorAll<HTMLButtonElement>("button[role^=menuitem]") ?? []);

  onMount(async () => {
    // keep the menu inside the window
    pos = { x, y };
    await tick();
    if (el) {
      const r = el.getBoundingClientRect();
      pos = { x: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)), y: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)) };
    }
    focusAt(buttons()[focusIndex] ?? buttons()[0]);
  });

  function focusAt(b: HTMLButtonElement | undefined) {
    b?.focus();
    b?.scrollIntoView({ block: "nearest" });
  }

  function move(delta: number) {
    const b = buttons();
    const i = b.indexOf(document.activeElement as HTMLButtonElement);
    focusAt(b[(i + delta + b.length) % b.length]);
  }

  function keydown(e: KeyboardEvent) {
    const k = e.key;
    if (k === "ArrowDown") move(1);
    else if (k === "ArrowUp") move(-1);
    else if (k === "Home") focusAt(buttons()[0]);
    else if (k === "End") focusAt(buttons().at(-1));
    else if (k === "Escape") onclose(true);
    else if (k === "ArrowLeft" && onleft) onleft();
    else if (k === "ArrowRight" && onright) onright();
    else if (k === "Tab") onclose(false);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function activate(item: Extract<MenuItem, { kind: "item" }>) {
    if (item.disabled) return;
    onclose(true);
    item.run?.();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="menu" role="menu" aria-label={label} tabindex="-1" bind:this={el} style="inset-inline-start:{pos.x}px;top:{pos.y}px" onkeydown={keydown}>
  {#each items as item, i (i)}
    {#if item.kind === "sep"}
      <div class="menu-sep" role="separator"></div>
    {:else if item.kind === "group"}
      <div class="menu-group" role="presentation">{item.label}</div>
    {:else}
      <button
        class="menu-item"
        role={item.role === "check" ? "menuitemcheckbox" : item.role === "radio" ? "menuitemradio" : "menuitem"}
        aria-checked={item.role ? !!item.checked : undefined}
        aria-disabled={item.disabled ? "true" : undefined}
        title={item.disabled ? item.reason : undefined}
        onclick={() => activate(item)}
        tabindex="-1"
      >
        <span class="mi-icon">
          {#if item.role === "check" && item.checked}<Icon name="check" />{:else if item.role === "radio" && item.checked}<Icon name="dot" />{:else if item.icon}<Icon name={item.icon} />{/if}
        </span>
        <span class="mi-label">{item.label}{#if item.disabled && item.reason}<span class="mi-reason"> · {item.reason}</span>{/if}</span>
        {#if item.shortcut}<span class="mi-key">{item.shortcut}</span>{/if}
      </button>
    {/if}
  {/each}
</div>
