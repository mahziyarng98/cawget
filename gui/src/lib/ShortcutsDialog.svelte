<script lang="ts">
  import Modal from "./Modal.svelte";
  import { SHORTCUTS } from "./shortcuts";

  let { onclose }: { onclose: () => void } = $props();
</script>

<Modal title="Keyboard shortcuts" width={480} {onclose}>
  <table class="keys">
    <tbody>
      {#each SHORTCUTS as s (s.keys)}
        <tr><td><kbd>{s.keys}</kbd></td><td>{s.label}</td></tr>
      {/each}
      <tr><td><kbd>Ctrl/Shift + click</kbd></td><td>Add to / extend the row selection</td></tr>
      <tr><td><kbd>Ctrl+←/→</kbd></td><td>Move the focused column header</td></tr>
      <tr><td><kbd>Alt+←/→</kbd></td><td>Resize the focused column header</td></tr>
    </tbody>
  </table>
  {#snippet buttons()}
    <button class="btn primary" data-autofocus onclick={onclose}>Close</button>
  {/snippet}
</Modal>
