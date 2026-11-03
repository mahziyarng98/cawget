<script lang="ts">
  import { type Job, fmtBytes } from "./api";
  import Modal from "./Modal.svelte";

  let {
    job,
    onopen,
    onfolder,
    ondontshow,
    onclose,
  }: { job: Job; onopen: () => void; onfolder: () => void; ondontshow: (v: boolean) => void; onclose: () => void } = $props();
</script>

<Modal title="Download completed" width={520} {onclose}>
  <dl class="props compact">
    <dt>File</dt><dd><bdi>{job.name}</bdi></dd>
    <dt>Size</dt><dd><bdi>{job.size != null ? fmtBytes(job.size) : "—"}</bdi></dd>
    <dt>Saved to</dt><dd class="selectable"><bdi>{job.out_path}</bdi></dd>
    <dt>Address</dt><dd class="selectable"><bdi>{job.url}</bdi></dd>
  </dl>
  <label class="check"><input type="checkbox" onchange={(e) => ondontshow(e.currentTarget.checked)} /> Don't show this dialog again</label>
  {#snippet buttons()}
    <button class="btn" onclick={onopen}>Open file</button>
    <button class="btn" aria-disabled="true" title="Coming soon: choosing an application is not supported yet" onclick={() => {}}>Open with…</button>
    <button class="btn" onclick={onfolder}>Show in folder</button>
    <!-- Close has the focus: a stray Enter must not launch another application. -->
    <button class="btn primary" data-autofocus onclick={onclose}>Close</button>
  {/snippet}
</Modal>
