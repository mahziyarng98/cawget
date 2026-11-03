<script lang="ts" generics="T extends string">
  // Tabs of a dialog. A strip that scrolls sideways when it does not fit; in a narrow dialog a select replaces it
  // (CSS decides which one is rendered, so only one of them is ever focusable).
  // Keyboard on the strip: Left/Right (mirrored in RTL), Home/End.
  let {
    tabs,
    value = $bindable(),
    label,
    tabId,
    panelId,
  }: { tabs: readonly { id: T; label: string }[]; value: T; label: string; tabId: (id: T) => string; panelId: (id: T) => string } = $props();

  function key(e: KeyboardEvent, i: number) {
    const rtl = document.documentElement.dir === "rtl";
    let n = -1;
    if (e.key === (rtl ? "ArrowLeft" : "ArrowRight")) n = (i + 1) % tabs.length;
    else if (e.key === (rtl ? "ArrowRight" : "ArrowLeft")) n = (i - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") n = 0;
    else if (e.key === "End") n = tabs.length - 1;
    if (n < 0) return;
    e.preventDefault();
    value = tabs[n].id;
    const strip = (e.currentTarget as HTMLElement).parentElement;
    const b = strip?.querySelectorAll<HTMLElement>("[role=tab]")[n];
    b?.focus();
    b?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }
</script>

<div class="tabstrip">
  <div class="tabs" role="tablist" aria-label={label}>
    {#each tabs as t, i (t.id)}
      <button
        class="tab"
        role="tab"
        id={tabId(t.id)}
        aria-selected={value === t.id}
        aria-controls={panelId(t.id)}
        tabindex={value === t.id ? 0 : -1}
        data-autofocus={value === t.id ? true : undefined}
        onclick={(e) => { value = t.id; e.currentTarget.scrollIntoView({ block: "nearest", inline: "nearest" }); }}
        onkeydown={(e) => key(e, i)}>{t.label}</button>
    {/each}
  </div>
  <select class="input tab-select" aria-label={label} data-autofocus bind:value>
    {#each tabs as t (t.id)}<option value={t.id}>{t.label}</option>{/each}
  </select>
</div>
