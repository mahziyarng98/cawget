<script lang="ts">
  // Download list (WAI-ARIA grid).
  // Columns: click a header to sort; drag its edge (or Alt+Left/Right with the header focused) to resize;
  // drag the header (or Ctrl+Left/Right) to reorder. The file name is the flexible column; when the list is narrow
  // the lowest-priority columns are hidden (visibleColumns) without changing the stored layout.
  // Compact (narrow window): two-line rows and a sort bar instead of column headers.
  // Rows: click, Ctrl+click, Shift+click; Up/Down/Home/End/PageUp/PageDown, Shift extends; double-click opens.
  import type { Snippet } from "svelte";
  import { type Job, fmtBytes, fmtEta } from "./api";
  import { categoryOf } from "./categories";
  import Icon, { type IconName } from "./Icon.svelte";
  import { COMING_SOON, type ColumnId, NAME_MIN, type TableLayout, clampWidth, columnDef, dropColumn, moveVisible, statusText, toggleSort, visibleColumns } from "./table";

  let {
    rows,
    layout = $bindable(),
    selected = $bindable(),
    cursor = $bindable(),
    compact = false,
    onopen,
    oncontext,
    empty,
  }: {
    rows: Job[];
    layout: TableLayout;
    selected: number[];
    cursor: number | null;
    compact?: boolean;
    onopen: (j: Job) => void;
    oncontext: (x: number, y: number) => void;
    empty?: Snippet;
  } = $props();

  let grid = $state<HTMLDivElement | null>(null);
  let wrap = $state<HTMLDivElement | null>(null);
  let available = $state(0);
  let anchor: number | null = null;
  // Column reordering with the pointer (HTML5 drag-and-drop is unreliable in WebKitGTK).
  let colDrag = $state<{ col: ColumnId; startX: number; moved: boolean; over: ColumnId | null } | null>(null);
  let suppressClick = false;
  // The click that ends a drag or resize must not sort. When the pointer is released over another header no click
  // follows at all, so the flag also clears itself right after the current event.
  function suppressNextClick() {
    suppressClick = true;
    setTimeout(() => (suppressClick = false), 0);
  }
  const now = () => Date.now() / 1000;

  // Width of the list (without a vertical scrollbar), observed so that columns follow live resizing.
  $effect(() => {
    if (!wrap) return;
    const w = wrap;
    const ro = new ResizeObserver(() => (available = w.clientWidth));
    ro.observe(w);
    available = w.clientWidth;
    return () => ro.disconnect();
  });

  const visible = $derived(available > 0 ? visibleColumns(layout, available) : layout.order.filter((c) => !layout.hidden.includes(c)));
  // The name column takes the free width (never below NAME_MIN); the others keep their width and may only shrink
  // to their minimum, so the grid always fits and is never cut off.
  const template = $derived(
    visible.map((c) => (c === "name" ? `minmax(${NAME_MIN}px, 1fr)` : `minmax(${columnDef(c).min}px, ${layout.widths[c]}px)`)).join(" "),
  );
  /** Extra width a column may take without pushing another column out: what the name column has above its minimum. */
  const slack = $derived(Math.max(0, available - NAME_MIN - visible.filter((c) => c !== "name").reduce((s, c) => s + layout.widths[c], 0)));
  const sel = $derived(new Set(selected));

  // ---------- selection ----------
  function indexOf(id: number | null) {
    return id == null ? -1 : rows.findIndex((r) => r.id === id);
  }
  function selectRange(from: number, to: number) {
    const [a, b] = from < to ? [from, to] : [to, from];
    selected = rows.slice(a, b + 1).map((r) => r.id);
  }
  function setCursor(i: number, extend: boolean) {
    if (!rows.length) return;
    i = Math.max(0, Math.min(rows.length - 1, i));
    cursor = rows[i].id;
    if (extend) selectRange(indexOf(anchor ?? cursor), i);
    else {
      anchor = cursor;
      selected = [cursor];
    }
    grid?.querySelector<HTMLElement>(`[data-row="${cursor}"]`)?.scrollIntoView({ block: "nearest" });
  }
  function rowClick(e: MouseEvent, j: Job) {
    if (e.shiftKey && anchor != null) {
      selectRange(indexOf(anchor), indexOf(j.id));
    } else if (e.ctrlKey || e.metaKey) {
      selected = sel.has(j.id) ? selected.filter((x) => x !== j.id) : [...selected, j.id];
      anchor = j.id;
    } else {
      selected = [j.id];
      anchor = j.id;
    }
    cursor = j.id;
    grid?.focus();
  }
  function rowContext(e: MouseEvent, j: Job) {
    e.preventDefault();
    if (!sel.has(j.id)) {
      selected = [j.id];
      anchor = j.id;
    }
    cursor = j.id;
    oncontext(e.clientX, e.clientY);
  }
  function gridKey(e: KeyboardEvent) {
    const i = indexOf(cursor);
    const rowH = grid?.querySelector<HTMLElement>(".tr")?.offsetHeight || 28;
    const page = Math.max(1, Math.floor((wrap?.clientHeight ?? 280) / rowH) - 2);
    const k = e.key;
    if (k === "ArrowDown") setCursor(i < 0 ? 0 : i + 1, e.shiftKey);
    else if (k === "ArrowUp") setCursor(i < 0 ? 0 : i - 1, e.shiftKey);
    else if (k === "Home") setCursor(0, e.shiftKey);
    else if (k === "End") setCursor(rows.length - 1, e.shiftKey);
    else if (k === "PageDown") setCursor(i + page, e.shiftKey);
    else if (k === "PageUp") setCursor(i - page, e.shiftKey);
    else if (k === " " && cursor != null) selected = sel.has(cursor) ? selected.filter((x) => x !== cursor) : [...selected, cursor];
    else return;
    e.preventDefault();
  }

  // ---------- columns ----------
  function headerKey(e: KeyboardEvent, c: ColumnId) {
    const rtl = document.documentElement.dir === "rtl";
    const grow = rtl ? "ArrowLeft" : "ArrowRight";
    const shrink = rtl ? "ArrowRight" : "ArrowLeft";
    const fwd = rtl ? -1 : 1;
    if ((e.key === "Enter" || e.key === " ") && !e.ctrlKey && !e.altKey) layout = { ...layout, sort: toggleSort(layout.sort, c) };
    else if (e.altKey && (e.key === grow || e.key === shrink) && c !== "name") {
      const w = clampWidth(c, Math.min(layout.widths[c] + slack, layout.widths[c] + (e.key === grow ? 16 : -16)));
      layout = { ...layout, widths: { ...layout.widths, [c]: w } };
    } else if (e.ctrlKey && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
      layout = { ...layout, order: moveVisible(layout.order, visible, c, (e.key === "ArrowRight" ? 1 : -1) * fwd) };
      queueMicrotask(() => grid?.querySelector<HTMLElement>(`[data-col="${c}"]`)?.focus());
    } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      const hs = Array.from(grid?.querySelectorAll<HTMLElement>("[data-col]") ?? []);
      const i = hs.indexOf(e.currentTarget as HTMLElement);
      const d = (e.key === "ArrowRight" ? 1 : -1) * fwd;
      hs[(i + d + hs.length) % hs.length]?.focus();
    } else if (e.key === "ArrowDown") grid?.focus();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function startResize(e: PointerEvent, c: ColumnId) {
    e.preventDefault();
    e.stopPropagation(); // never also start a column drag
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startW = layout.widths[c];
    // Growing stops where another column would have to be hidden: the room comes from the name column.
    const maxW = startW + slack;
    const rtl = document.documentElement.dir === "rtl";
    const moveFn = (ev: PointerEvent) => {
      const dx = (ev.clientX - startX) * (rtl ? -1 : 1);
      layout = { ...layout, widths: { ...layout.widths, [c]: clampWidth(c, Math.min(maxW, startW + dx)) } };
    };
    const up = () => {
      handle.removeEventListener("pointermove", moveFn);
      handle.removeEventListener("pointerup", up);
      suppressNextClick(); // the click that ends a resize must not sort
    };
    handle.addEventListener("pointermove", moveFn);
    handle.addEventListener("pointerup", up);
  }

  function colUnder(x: number, y: number): ColumnId | null {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-col]");
    return (el?.dataset.col as ColumnId | undefined) ?? null;
  }
  function headerDown(e: PointerEvent, c: ColumnId) {
    if (e.button !== 0) return;
    colDrag = { col: c, startX: e.clientX, moved: false, over: null };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function headerMove(e: PointerEvent) {
    if (!colDrag) return;
    if (!colDrag.moved && Math.abs(e.clientX - colDrag.startX) > 6) colDrag.moved = true;
    if (colDrag.moved) colDrag.over = colUnder(e.clientX, e.clientY);
  }
  function headerUp(e: PointerEvent) {
    if (!colDrag) return;
    if (colDrag.moved) {
      const target = colUnder(e.clientX, e.clientY);
      if (target && target !== colDrag.col) layout = { ...layout, order: dropColumn(layout.order, colDrag.col, target) };
      suppressNextClick();
    }
    colDrag = null;
  }
  function headerClick(c: ColumnId) {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    layout = { ...layout, sort: toggleSort(layout.sort, c) };
  }

  function ariaSort(c: ColumnId): "ascending" | "descending" | "none" {
    return layout.sort?.col === c ? (layout.sort.dir === 1 ? "ascending" : "descending") : "none";
  }

  const catIcon = (j: Job): IconName => categoryOf(j.name) ?? "other";

  // Compact sort bar: the same sort state as the column headers.
  const SORTABLE: { id: ColumnId | "newest"; label: string }[] = [
    { id: "newest", label: "Newest first" },
    { id: "name", label: "File name" },
    { id: "status", label: "Status" },
    { id: "size", label: "Size" },
    { id: "eta", label: "Time left" },
    { id: "rate", label: "Speed" },
  ];
  function sortBy(v: string) {
    layout = { ...layout, sort: v === "newest" ? null : { col: v as ColumnId, dir: layout.sort?.dir ?? 1 } };
  }
</script>

{#snippet bar(j: Job)}
  {#if (j.state === "active" || j.state === "paused") && j.size}
    <span class="mini-bar" aria-hidden="true">
      {#if j.parts.length > 1}
        {#each j.parts as p (p.start)}
          <span class="mini-seg" style="inset-inline-start:{(p.start / j.size) * 100}%;width:{((p.end - p.start + 1) / j.size) * 100}%"><i style="width:{(p.done / (p.end - p.start + 1)) * 100}%"></i></span>
        {/each}
      {:else}
        <span class="mini-seg" style="inset-inline-start:0;width:100%"><i style="width:{(j.fraction ?? 0) * 100}%"></i></span>
      {/if}
    </span>
  {/if}
{/snippet}

{#if compact}
  <div class="sortbar">
    <label class="sortbar-label" for="dl-sort">Sort</label>
    <select id="dl-sort" class="input" value={layout.sort?.col ?? "newest"} onchange={(e) => sortBy(e.currentTarget.value)}>
      {#each SORTABLE as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
    </select>
    <button
      class="icon-btn"
      disabled={!layout.sort}
      aria-label={layout.sort?.dir === -1 ? "Descending (click for ascending)" : "Ascending (click for descending)"}
      title={layout.sort ? (layout.sort.dir === 1 ? "Ascending" : "Descending") : "Newest first has no direction"}
      onclick={() => layout.sort && (layout = { ...layout, sort: { ...layout.sort, dir: layout.sort.dir === 1 ? -1 : 1 } })}
    ><Icon name={layout.sort?.dir === -1 ? "sortDown" : "sortUp"} size={16} /></button>
  </div>
{/if}

<div class="table-wrap" bind:this={wrap}>
  <div
    class="grid"
    class:compact
    role="grid"
    aria-label="Downloads"
    aria-multiselectable="true"
    aria-rowcount={rows.length + (compact ? 0 : 1)}
    tabindex="0"
    aria-activedescendant={cursor != null ? `dl-row-${cursor}` : undefined}
    bind:this={grid}
    onkeydown={gridKey}
    style="--cols:{template}"
  >
    {#if !compact}
    <div class="grid-head" role="row">
      {#each visible as c, ci (c)}
        {@const def = columnDef(c)}
        <div
          class="th"
          class:unsupported={!def.supported}
          class:end={def.align === "end"}
          role="columnheader"
          aria-sort={ariaSort(c)}
          tabindex={ci === 0 ? 0 : -1}
          data-col={c}
          title={!def.supported ? `${def.label}: ${COMING_SOON} (the engine does not record this yet)` : c === "name" ? `${def.label}: click to sort, drag to move (takes the free width)` : `${def.label}: click to sort, drag to move, drag the edge to resize`}
          class:dragging={colDrag?.moved && colDrag.col === c}
          class:drop-target={colDrag?.moved && colDrag.over === c && colDrag.col !== c}
          onpointerdown={(e) => headerDown(e, c)}
          onpointermove={headerMove}
          onpointerup={headerUp}
          onclick={() => headerClick(c)}
          onkeydown={(e) => headerKey(e, c)}
        >
          <span class="th-label">{def.label}</span>
          {#if layout.sort?.col === c}<Icon name={layout.sort.dir === 1 ? "sortUp" : "sortDown"} size={12} />{/if}
          {#if c !== "name"}<span class="th-resize" role="presentation" onpointerdown={(e) => startResize(e, c)}></span>{/if}
        </div>
      {/each}
    </div>
    {/if}

    {#each rows as j (j.id)}
      <!-- Keyboard interaction is handled by the grid (aria-activedescendant), not by each row. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="tr"
        id="dl-row-{j.id}"
        tabindex="-1"
        class:sel={sel.has(j.id)}
        class:cursor={cursor === j.id}
        role="row"
        aria-selected={sel.has(j.id)}
        data-row={j.id}
        onclick={(e) => rowClick(e, j)}
        ondblclick={() => onopen(j)}
        oncontextmenu={(e) => rowContext(e, j)}
      >
        {#if compact}
          <div class="td c-line" role="gridcell">
            <span class="cell-icon state-{j.state}"><Icon name={catIcon(j)} size={14} /></span>
            <bdi class="ellipsis c-name">{j.name}</bdi>
            <bdi class="c-meta">{j.size != null ? fmtBytes(j.size) : "—"}</bdi>
          </div>
          <div class="td c-line c-sub" role="gridcell">
            <span class="status state-{j.state}" title={j.error ?? undefined}>
              {@render bar(j)}
              <bdi class="ellipsis">{statusText(j, now())}{(j.state === "failed" || j.state === "queued") && j.error ? `: ${j.error}` : ""}</bdi>
            </span>
            {#if j.state === "active"}<bdi class="c-meta">{[j.speed > 0 ? `${fmtBytes(j.speed)}/s` : "", j.eta_secs != null ? fmtEta(j.eta_secs) : ""].filter(Boolean).join(" · ")}</bdi>{/if}
          </div>
        {:else}
        {#each visible as c (c)}
          {@const def = columnDef(c)}
          <div class="td" class:end={def.align === "end"} class:muted={!def.supported} role="gridcell">
            {#if c === "name"}
              <span class="cell-icon state-{j.state}"><Icon name={catIcon(j)} size={14} /></span>
              <bdi class="ellipsis">{j.name}</bdi>
            {:else if c === "size"}
              <bdi>{j.size != null ? fmtBytes(j.size) : "—"}</bdi>
            {:else if c === "status"}
              <span class="status state-{j.state}" title={j.error ?? undefined}>
                {@render bar(j)}
                <bdi class="ellipsis">{statusText(j, now())}{(j.state === "failed" || j.state === "queued") && j.error ? `: ${j.error}` : ""}</bdi>
              </span>
            {:else if c === "eta"}
              <bdi>{j.state === "active" && j.eta_secs != null ? fmtEta(j.eta_secs) : ""}</bdi>
            {:else if c === "rate"}
              <bdi>{j.state === "active" && j.speed > 0 ? `${fmtBytes(j.speed)}/s` : ""}</bdi>
            {:else}
              <span title={COMING_SOON}>—</span>
            {/if}
          </div>
        {/each}
        {/if}
      </div>
    {/each}
  </div>
  {#if rows.length === 0}{@render empty?.()}{/if}
</div>
