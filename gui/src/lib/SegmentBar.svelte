<script lang="ts">
  // One teal block per connection (segment), placed at its byte range; or a single bar without segments.
  import type { Part } from "./api";

  let { parts, size, fraction, state, height = 14 }: { parts: Part[]; size: number | null; fraction: number | null; state: string; height?: number } = $props();
</script>

<div class="seg-bar state-{state}" style="height:{height}px" role="presentation">
  {#if size && parts.length > 0}
    {#each parts as p (p.start)}
      <span class="seg-block" style="inset-inline-start:{(p.start / size) * 100}%;width:{((p.end - p.start + 1) / size) * 100}%">
        <i style="width:{Math.min(100, (p.done / (p.end - p.start + 1)) * 100)}%"></i>
      </span>
    {/each}
  {:else}
    <span class="seg-block" style="inset-inline-start:0;width:100%"><i style="width:{(fraction ?? (state === 'done' ? 1 : 0)) * 100}%"></i></span>
  {/if}
</div>
