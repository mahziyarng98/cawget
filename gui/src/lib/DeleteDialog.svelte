<script lang="ts">
  import type { Job } from "./api";
  import Modal from "./Modal.svelte";

  let { jobs, onconfirm, onclose }: { jobs: Job[]; onconfirm: (deleteFiles: boolean) => void; onclose: () => void } = $props();
  let deleteFiles = $state(false);
  const finished = $derived(jobs.filter((j) => j.state === "done").length);
  const unfinished = $derived(jobs.length - finished);
</script>

<Modal title="Delete downloads" width={440} {onclose}>
  <p>Remove {jobs.length === 1 ? `“${jobs[0].name}”` : `${jobs.length} downloads`} from the list?</p>
  {#if unfinished > 0}
    <p class="hint">Partial data of {unfinished === 1 ? "the incomplete download" : `${unfinished} incomplete downloads`} is deleted.</p>
  {/if}
  {#if finished > 0}
    <label class="check"><input type="checkbox" bind:checked={deleteFiles} /> Also delete the {finished === 1 ? "downloaded file" : `${finished} downloaded files`} from disk</label>
  {/if}
  {#snippet buttons()}
    <button class="btn primary" data-autofocus onclick={() => onconfirm(deleteFiles)}>OK</button>
    <button class="btn" onclick={onclose}>Cancel</button>
  {/snippet}
</Modal>
