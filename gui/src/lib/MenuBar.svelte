<script lang="ts">
  // Classic menu bar. Mouse: click opens, hovering switches while open. Keyboard: F10 focuses it,
  // Alt+<mnemonic> opens a menu, Left/Right switch menus, Escape returns focus.
  // Compact: all menus fold into one menu button (same items, grouped by menu); Alt+<mnemonic> opens it at that group.
  // A hidden copy of the full bar reports the width it needs (`needed`), so the app can fold it when it does not fit.
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";
  import { type TopMenu, foldMenus } from "./menu";

  let {
    menus,
    logo,
    name,
    compact = false,
    needed = $bindable(0),
  }: { menus: TopMenu[]; logo: string; name: string; compact?: boolean; needed?: number } = $props();

  // `open`: index of the open top menu, or -1 for the folded menu.
  let open = $state<number | null>(null);
  let focusIndex = $state(0);
  let anchor = $state({ x: 0, y: 0 });
  let bar = $state<HTMLDivElement | null>(null);
  let measure = $state<HTMLDivElement | null>(null);
  let lastFocus: HTMLElement | null = null;

  const btn = (i: number) => bar?.querySelectorAll<HTMLButtonElement>(".mb-btn")[i];
  const folded = $derived(foldMenus(menus));

  $effect(() => {
    if (!measure) return;
    const m = measure;
    const ro = new ResizeObserver(() => (needed = Math.ceil(m.scrollWidth)));
    ro.observe(m);
    return () => ro.disconnect();
  });

  function place(b: HTMLElement) {
    const r = b.getBoundingClientRect();
    const rtl = document.documentElement.dir === "rtl";
    anchor = { x: rtl ? window.innerWidth - r.right : r.left, y: r.bottom };
  }

  export function openMenu(i: number) {
    if (open === null) lastFocus = document.activeElement as HTMLElement;
    if (compact) {
      const b = btn(0);
      if (!b) return;
      place(b);
      focusIndex = folded.first[menus[i]?.id] ?? 0;
      open = -1;
      return;
    }
    const b = btn(i);
    if (!b) return;
    place(b);
    focusIndex = 0;
    open = i;
  }

  export function focusBar() {
    lastFocus = document.activeElement as HTMLElement;
    btn(0)?.focus();
  }

  export function openByMnemonic(letter: string): boolean {
    const i = menus.findIndex((m) => m.mnemonic.toLowerCase() === letter.toLowerCase());
    if (i < 0) return false;
    openMenu(i);
    return true;
  }

  function close(refocus: boolean) {
    const was = open;
    open = null;
    if (refocus && was !== null) restoreFocus();
  }

  // Back to where the user was; with no meaningful previous focus, the download list.
  function restoreFocus() {
    if (lastFocus && lastFocus !== document.body && document.contains(lastFocus) && !bar?.contains(lastFocus)) lastFocus.focus();
    else document.querySelector<HTMLElement>("[role=grid]")?.focus();
  }

  function barKey(e: KeyboardEvent, i: number) {
    const n = menus.length;
    if (e.key === "ArrowRight") btn((i + 1) % n)?.focus();
    else if (e.key === "ArrowLeft") btn((i - 1 + n) % n)?.focus();
    else if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") openMenu(i);
    else if (e.key === "Escape") restoreFocus();
    else return;
    e.preventDefault();
  }

  function compactKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") openMenu(0);
    else if (e.key === "Escape") restoreFocus();
    else return;
    e.preventDefault();
  }
</script>

{#snippet label(m: TopMenu)}{#each m.label.split("") as ch, k (k)}{#if k === m.label.toLowerCase().indexOf(m.mnemonic.toLowerCase())}<u>{ch}</u>{:else}{ch}{/if}{/each}{/snippet}

<!-- Width probe of the full bar (never visible, never focusable). -->
<div class="probe-box" aria-hidden="true" inert>
  <div class="menubar mb-measure" bind:this={measure}>
    <img class="mb-logo" src={logo} alt="" width="18" height="18" />
    <span class="mb-name">{name}</span>
    {#each menus as m (m.id)}<span class="mb-btn">{@render label(m)}</span>{/each}
  </div>
</div>

{#if compact}
  <div class="mb-compact" role="menubar" aria-label="Main menu" bind:this={bar}>
    <button
      class="mb-btn mb-burger"
      role="menuitem"
      aria-haspopup="menu"
      aria-expanded={open === -1}
      aria-label="Menu"
      title="Menu (F10, Alt+F/S/D/V/H)"
      onclick={() => (open === -1 ? close(false) : openMenu(0))}
      onkeydown={compactKey}
    ><Icon name="menu" size={18} /></button>
  </div>
{:else}
  <div class="menubar" role="menubar" aria-label="Main menu" bind:this={bar}>
    <img class="mb-logo" src={logo} alt="" width="18" height="18" />
    <span class="mb-name">{name}</span>
    {#each menus as m, i (m.id)}
      <button
        class="mb-btn"
        class:open={open === i}
        role="menuitem"
        aria-haspopup="menu"
        aria-expanded={open === i}
        onclick={() => (open === i ? close(false) : openMenu(i))}
        onmouseenter={() => open !== null && open !== i && openMenu(i)}
        onkeydown={(e) => barKey(e, i)}
      >{@render label(m)}</button>
    {/each}
  </div>
{/if}

{#if open !== null}
  <div class="menu-scrim" role="presentation" onmousedown={() => close(false)}></div>
  {#key open}
    {#if open === -1}
      <Menu items={folded.items} x={anchor.x} y={anchor.y} label="Menu" {focusIndex} onclose={close} />
    {:else}
      <Menu
        items={menus[open].items()}
        x={anchor.x}
        y={anchor.y}
        label={menus[open].label}
        onclose={close}
        onleft={() => openMenu((open! - 1 + menus.length) % menus.length)}
        onright={() => openMenu((open! + 1) % menus.length)}
      />
    {/if}
  {/key}
{/if}
