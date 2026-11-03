<script lang="ts">
  // Read-only facts about one download, as recorded by the engine.
  import { type Job, fmtBytes, host } from "./api";
  import { categoryOf, FILE_CATEGORIES } from "./categories";
  import Modal from "./Modal.svelte";
  import { statusText } from "./table";

  let { job, onclose }: { job: Job; onclose: () => void } = $props();
  const cat = $derived(FILE_CATEGORIES.find((c) => c.id === categoryOf(job.name))?.label ?? "Other");
  const saveTo = $derived(job.out_path.endsWith("/") || !job.out_path ? `${job.out_path || "(default folder)"} (name chosen on first connection)` : job.out_path);
  const date = (s: number) => (s > 0 ? new Date(s * 1000).toLocaleString() : "—");
  const rows = $derived<[string, string][]>([
    ["File name", job.name],
    ["Status", statusText(job, Date.now() / 1000) + (job.error ? ` — ${job.error}` : "")],
    ["Size", job.size != null ? `${fmtBytes(job.size)} (${job.size.toLocaleString()} bytes)` : "unknown"],
    ["Downloaded", `${fmtBytes(job.done)}${job.size ? ` (${(((job.fraction ?? 0) * 100)).toFixed(1)}%)` : ""}`],
    ["Category", cat],
    ["Address", job.url],
    ["Host", host(job.url)],
    ["Save to", saveTo],
    ["Can resume", job.size == null ? "unknown (not contacted yet)" : job.accept_ranges ? "yes (range requests)" : "no (single stream)"],
    ["Connections", job.segments > 0 ? String(job.segments) : "default (Options)"],
    ["Added", date(job.created)],
    ["Start after", job.not_before > 0 ? date(job.not_before) : "—"],
    ["ETag", job.etag ?? "—"],
    ["Last modified", job.last_modified ?? "—"],
    ["Expected SHA-256", job.sha256 ?? "—"],
  ]);
</script>

<Modal title="Properties" width={560} {onclose}>
  <dl class="props">
    {#each rows as [k, v] (k)}
      <dt>{k}</dt>
      <dd class="selectable">{v}</dd>
    {/each}
  </dl>
  {#snippet buttons()}
    <button class="btn primary" data-autofocus onclick={onclose}>Close</button>
  {/snippet}
</Modal>
