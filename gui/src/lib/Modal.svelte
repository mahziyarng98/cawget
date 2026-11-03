<script lang="ts">
  // Dialog frame: title bar, body, button row. Escape closes, Tab stays inside, focus returns on close.
  // Never wider or taller than the window: the body scrolls, the button row wraps. The frame is a size container,
  // so the contents can stack their form rows when the dialog itself is narrow.
  import { onMount, type Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let {
    title,
    width = 460,
    onclose,
    children,
    buttons,
  }: { title: string; width?: number; onclose: () => void; children: Snippet; buttons?: Snippet } = $props();

  let el = $state<HTMLDivElement | null>(null);
  const before = document.activeElement as HTMLElement | null;

  // Only controls that are rendered count (narrow dialogs swap tab strips for a select, and back).
  const shown = (e: HTMLElement) => e.getClientRects().length > 0;
  const focusables = () =>
    Array.from(el?.querySelectorAll<HTMLElement>("button:not([disabled]):not([aria-disabled=true]), input:not([disabled]), textarea, select:not([disabled]), [tabindex='0']") ?? []).filter(shown);

  onMount(() => {
    const auto = Array.from(el?.querySelectorAll<HTMLElement>("[data-autofocus]") ?? []).find(shown);
    const first = auto ?? focusables()[1] ?? focusables()[0];
    first?.focus();
    // Back to the control that opened the dialog; if it is gone (e.g. a dialog that replaced another), the list.
    return () => {
      if (before && before !== document.body && document.contains(before)) before.focus();
      else document.querySelector<HTMLElement>("[role=grid]")?.focus();
    };
  });

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if (e.key === "Tab") {
      const f = focusables();
      if (!f.length) return;
      const i = f.indexOf(document.activeElement as HTMLElement);
      if (e.shiftKey && i <= 0) { e.preventDefault(); f.at(-1)!.focus(); }
      else if (!e.shiftKey && i === f.length - 1) { e.preventDefault(); f[0].focus(); }
    }
    e.stopPropagation(); // dialogs own the keyboard while open
  }
</script>

<div class="modal-scrim" role="presentation">
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} tabindex="-1" bind:this={el} style="inline-size:min({width}px, calc(100vw - 1rem))" onkeydown={key}>
    <div class="modal-title">
      <span class="modal-title-text">{title}</span>
      <button class="icon-btn" aria-label="Close" onclick={onclose}><Icon name="close" size={14} /></button>
    </div>
    <div class="modal-body">{@render children()}</div>
    {#if buttons}<div class="modal-buttons">{@render buttons()}</div>{/if}
  </div>
</div>
