<script lang="ts">
  // Status bar. Items drop out by priority when the bar is narrow (fitByPriority), so it never wraps; the transfer
  // rate and an engine error always stay (the error text is shortened with an ellipsis).
  import { fmtBytes } from "./api";
  import { fitByPriority } from "./responsive";

  let {
    total,
    active,
    queued,
    selected,
    speed,
    limit,
    engineError,
  }: { total: number; active: number; queued: number; selected: number; speed: number; limit: number; engineError: string | null } = $props();

  let bar = $state<HTMLElement | null>(null);
  let probe = $state<HTMLElement | null>(null);
  let available = $state(0);
  let widths = $state<Record<string, number>>({});

  interface Item {
    id: string;
    priority: number;
    pinned?: boolean;
    end?: boolean;
    text: string;
    title?: string;
    cls?: string;
  }
  const items = $derived<Item[]>([
    ...(engineError
      ? [{ id: "error", priority: 0, pinned: true, text: `Engine stopped: ${engineError}`, title: engineError, cls: "sb-error" }]
      : [
          { id: "total", priority: 1, text: `${total} download${total === 1 ? "" : "s"}` },
          { id: "active", priority: 2, text: `${active} active` },
          { id: "waiting", priority: 4, text: `${queued} waiting` },
        ]),
    { id: "selected", priority: 3, text: `${selected} selected` },
    { id: "speed", priority: 0, pinned: true, end: true, text: `${fmtBytes(speed)}/s`, cls: "num" },
    { id: "limit", priority: 5, end: true, text: limit > 0 ? `Limit ${fmtBytes(limit)}/s` : "No speed limit", title: "Change in Options" },
  ]);

  $effect(() => {
    if (!bar || !probe) return;
    const [b, p] = [bar, probe];
    const measure = () => {
      const cs = getComputedStyle(b);
      available = b.clientWidth - parseFloat(cs.paddingInlineStart) - parseFloat(cs.paddingInlineEnd);
      const w: Record<string, number> = {};
      for (const el of p.querySelectorAll<HTMLElement>("[data-id]")) w[el.dataset.id!] = Math.ceil(el.getBoundingClientRect().width);
      widths = w;
    };
    const ro = new ResizeObserver(measure);
    ro.observe(b);
    ro.observe(p);
    measure();
    return () => ro.disconnect();
  });

  const kept = $derived(new Set(fitByPriority(items.map((i) => ({ id: i.id, width: widths[i.id] ?? 0, priority: i.priority, pinned: i.pinned })), available).kept));
  const start = $derived(items.filter((i) => !i.end && kept.has(i.id)));
  const end = $derived(items.filter((i) => i.end && kept.has(i.id)));
</script>

<!-- Width probe (never visible). -->
<div class="probe-box" aria-hidden="true" inert>
  <div class="statusbar sb-probe" bind:this={probe}>
    {#each items as i (i.id)}<span class="sb-item {i.cls ?? ''}" data-id={i.id}><bdi>{i.text}</bdi></span>{/each}
  </div>
</div>

<footer class="statusbar" aria-label="Status" bind:this={bar}>
  {#each start as i (i.id)}<span class="sb-item {i.cls ?? ''}" title={i.title}><bdi>{i.text}</bdi></span>{/each}
  <span class="sb-grow"></span>
  {#each end as i (i.id)}<span class="sb-item {i.cls ?? ''}" title={i.title}><bdi>{i.text}</bdi></span>{/each}
</footer>
