<script lang="ts">
  // Category tree (WAI-ARIA tree): Up/Down move, Left collapses / goes to parent, Right expands, Enter/Space select.
  // Rail: the same tree as a narrow column of icons with the counts as badges (labels stay for screen readers and
  // tooltips); all categories are listed, so nothing is out of reach.
  import { type CategoryId, FILE_CATEGORIES } from "./categories";
  import Icon, { type IconName } from "./Icon.svelte";

  let {
    selected,
    counts,
    rail = false,
    onselect,
  }: { selected: CategoryId; counts: Record<CategoryId, number>; rail?: boolean; onselect: (c: CategoryId) => void } = $props();

  let expanded = $state(true);
  let el = $state<HTMLDivElement | null>(null);

  interface Node {
    id: CategoryId;
    label: string;
    icon: IconName;
    level: number;
    disabled?: string;
  }
  const nodes = $derived<Node[]>([
    { id: "all", label: "All", icon: "all", level: 1 },
    ...(expanded || rail ? FILE_CATEGORIES.map((c) => ({ id: c.id, label: c.label, icon: c.id as IconName, level: 2 })) : []),
    { id: "unfinished", label: "Incomplete", icon: "unfinished", level: 1 },
    { id: "finished", label: "Completed", icon: "finished", level: 1 },
    { id: "queues", label: "Queues", icon: "queues", level: 1, disabled: "Coming soon: named queues are not supported by the engine yet" },
  ]);

  function focusNode(id: CategoryId) {
    el?.querySelector<HTMLElement>(`[data-id="${id}"]`)?.focus();
  }

  function key(e: KeyboardEvent, i: number) {
    const n = nodes[i];
    const rtl = document.documentElement.dir === "rtl";
    const fwd = rtl ? "ArrowLeft" : "ArrowRight";
    const back = rtl ? "ArrowRight" : "ArrowLeft";
    if (e.key === "ArrowDown") focusNode(nodes[Math.min(nodes.length - 1, i + 1)].id);
    else if (e.key === "ArrowUp") focusNode(nodes[Math.max(0, i - 1)].id);
    else if (e.key === "Home") focusNode(nodes[0].id);
    else if (e.key === "End") focusNode(nodes[nodes.length - 1].id);
    else if (e.key === fwd && n.id === "all") expanded = true;
    else if (e.key === back && n.id === "all") expanded = false;
    else if (e.key === back && n.level === 2) focusNode("all");
    else if ((e.key === "Enter" || e.key === " ") && !n.disabled) onselect(n.id);
    else return;
    e.preventDefault();
  }
</script>

<div class="tree" class:rail role="tree" aria-label="Categories" bind:this={el}>
  {#each nodes as n, i (n.id)}
    <div
      class="tree-item"
      class:sel={selected === n.id}
      class:disabled={!!n.disabled}
      role="treeitem"
      aria-level={n.level}
      aria-selected={selected === n.id}
      aria-expanded={n.id === "all" && !rail ? expanded : undefined}
      aria-disabled={n.disabled ? "true" : undefined}
      title={rail ? (n.disabled ?? `${n.label}: ${counts[n.id]}`) : n.disabled}
      data-id={n.id}
      tabindex={selected === n.id ? 0 : -1}
      style={rail ? undefined : `padding-inline-start:${n.level === 2 ? 1.625 : 0.375}rem`}
      onclick={() => !n.disabled && onselect(n.id)}
      onkeydown={(e) => key(e, i)}
    >
      {#if n.id === "all" && !rail}
        <button class="twisty" tabindex="-1" aria-label={expanded ? "Collapse" : "Expand"} onclick={(e) => { e.stopPropagation(); expanded = !expanded; }}>
          <Icon name={expanded ? "chevronDown" : "chevron"} size={12} />
        </button>
      {/if}
      <Icon name={n.icon} />
      <span class="tree-label" class:sr-only={rail}>{n.label}</span>
      {#if !n.disabled}<span class="tree-count" class:badge={rail}>{counts[n.id]}</span>{/if}
    </div>
  {/each}
</div>
