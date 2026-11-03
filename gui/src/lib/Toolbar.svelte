<script lang="ts">
  // Icon + label toolbar. Roving focus: one tab stop, Left/Right/Home/End move between buttons.
  // Narrow: labels go first (icon-only buttons keep their name as tooltip and aria-label), then the least important
  // buttons move into the "More" menu (planToolbar). A hidden copy of all buttons provides their real widths, so
  // fonts, zoom and text scaling are measured, not guessed.
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";
  import type { MenuItem, ToolButton } from "./menu";
  import { TOOL_PRIORITY, type ToolMeasure, planToolbar } from "./responsive";

  let { buttons, short = false }: { buttons: ToolButton[]; short?: boolean } = $props();
  let focusIdx = $state(0);
  let el = $state<HTMLDivElement | null>(null);
  let probe = $state<HTMLDivElement | null>(null);
  let available = $state(0);
  let widths = $state<{ labeled: Record<string, number>; icon: number; more: number }>({ labeled: {}, icon: 36, more: 36 });
  let menuAt = $state<{ x: number; y: number } | null>(null);

  // Available width of the bar and widths of the buttons (re-measured when fonts or zoom change their size).
  $effect(() => {
    if (!el || !probe) return;
    const [bar, p] = [el, probe];
    const measure = () => {
      const cs = getComputedStyle(bar);
      available = bar.clientWidth - parseFloat(cs.paddingInlineStart) - parseFloat(cs.paddingInlineEnd);
      const gap = parseFloat(getComputedStyle(p).columnGap) || 0;
      const extra = (b: HTMLElement) => b.getBoundingClientRect().width + gap + (parseFloat(getComputedStyle(b).marginInlineEnd) || 0);
      const labeled: Record<string, number> = {};
      for (const b of p.querySelectorAll<HTMLElement>("[data-labeled]")) labeled[b.dataset.labeled!] = extra(b);
      const icon = p.querySelector<HTMLElement>("[data-icon]");
      widths = { labeled, icon: icon ? extra(icon) : 36, more: icon ? extra(icon) : 36 };
    };
    const ro = new ResizeObserver(measure);
    ro.observe(bar);
    ro.observe(p);
    measure();
    return () => ro.disconnect();
  });

  const plan = $derived.by(() => {
    const items: ToolMeasure[] = buttons.map((b) => ({
      id: b.id,
      labeled: widths.labeled[b.id] ?? 80,
      icon: widths.icon + (b.groupEnd ? 8 : 0),
      pinned: TOOL_PRIORITY[b.id]?.pinned,
      priority: TOOL_PRIORITY[b.id]?.priority ?? 9,
    }));
    return planToolbar(items, available, { more: widths.more, short });
  });
  const shown = $derived(buttons.filter((b) => plan.visible.includes(b.id)));
  const overflow = $derived(buttons.filter((b) => plan.overflow.includes(b.id)));
  const overflowItems = $derived<MenuItem[]>(
    overflow.map((b) => ({ kind: "item", id: b.id, label: b.label, icon: b.icon, disabled: b.disabled, reason: b.reason, run: b.run })),
  );
  const count = $derived(shown.length + (overflow.length ? 1 : 0));
  $effect(() => {
    if (focusIdx >= count) focusIdx = Math.max(0, count - 1);
  });

  function key(e: KeyboardEvent, i: number) {
    const list = Array.from(el?.querySelectorAll<HTMLButtonElement>(":scope > button") ?? []);
    const n = list.length;
    const rtl = document.documentElement.dir === "rtl";
    let next = -1;
    if (e.key === (rtl ? "ArrowLeft" : "ArrowRight")) next = (i + 1) % n;
    else if (e.key === (rtl ? "ArrowRight" : "ArrowLeft")) next = (i - 1 + n) % n;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = n - 1;
    if (next >= 0) {
      e.preventDefault();
      focusIdx = next;
      list[next]?.focus();
    }
  }

  function openMore(b: HTMLElement) {
    const r = b.getBoundingClientRect();
    const rtl = document.documentElement.dir === "rtl";
    menuAt = { x: rtl ? window.innerWidth - r.right : r.left, y: r.bottom };
  }
  function moreKey(e: KeyboardEvent, i: number) {
    if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      openMore(e.currentTarget as HTMLElement);
    } else key(e, i);
  }
  function closeMore(refocus: boolean) {
    menuAt = null;
    if (refocus) el?.querySelector<HTMLButtonElement>(".tb-more")?.focus();
  }
</script>

<!-- Width probe: every button with and without its label (never visible, never focusable). -->
<div class="probe-box" aria-hidden="true" inert>
  <div class="toolbar tb-probe" bind:this={probe}>
    {#each buttons as b (b.id)}
      <span class="tb-btn" class:group-end={b.groupEnd} data-labeled={b.id}><Icon name={b.icon} size={20} /><span>{b.label}</span></span>
    {/each}
    <span class="tb-btn icon-only" data-icon=""><Icon name="more" size={20} /></span>
  </div>
</div>

<div class="toolbar" class:icons={!plan.labels} role="toolbar" aria-label="Download actions" bind:this={el}>
  {#each shown as b, i (b.id)}
    <button
      class="tb-btn"
      class:icon-only={!plan.labels}
      class:group-end={b.groupEnd && i < shown.length - 1}
      tabindex={i === focusIdx ? 0 : -1}
      aria-disabled={b.disabled ? "true" : undefined}
      aria-label={plan.labels ? undefined : b.label}
      title={b.disabled ? `${b.label}: ${b.reason ?? "not available"}` : (b.title ?? b.label)}
      onclick={() => !b.disabled && b.run?.()}
      onfocus={() => (focusIdx = i)}
      onkeydown={(e) => key(e, i)}
    >
      <Icon name={b.icon} size={20} />
      {#if plan.labels}<span>{b.label}</span>{/if}
    </button>
  {/each}
  {#if overflow.length}
    <button
      class="tb-btn icon-only tb-more"
      tabindex={shown.length === focusIdx ? 0 : -1}
      aria-label="More actions"
      aria-haspopup="menu"
      aria-expanded={menuAt != null}
      title={`More: ${overflow.map((b) => b.label).join(", ")}`}
      onclick={(e) => (menuAt ? closeMore(false) : openMore(e.currentTarget))}
      onfocus={() => (focusIdx = shown.length)}
      onkeydown={(e) => moreKey(e, shown.length)}
    ><Icon name="more" size={20} /></button>
  {/if}
</div>

{#if menuAt}
  <div class="menu-scrim" role="presentation" onmousedown={() => closeMore(false)}></div>
  <Menu items={overflowItems} x={menuAt.x} y={menuAt.y} label="More actions" onclose={closeMore} />
{/if}
