<script lang="ts">
  import { APP_VERSION, PRODUCT_NAME } from "./brand";
  import Modal from "./Modal.svelte";

  let { logo, dbPath, migratedFrom = null, migrationDeferred = null, onclose }: { logo: string; dbPath: string; migratedFrom?: string | null; migrationDeferred?: string | null; onclose: () => void } = $props();
</script>

<Modal title={`About ${PRODUCT_NAME}`} width={420} {onclose}>
  <div class="about">
    <img src={logo} alt="" width="96" height="96" />
    <div>
      <h2>{PRODUCT_NAME}</h2>
      <p>Version {APP_VERSION}</p>
      <p class="hint">{PRODUCT_NAME} Download Manager: a fast segmented download manager for Linux, with parallel connections, resume, queue and scheduling.</p>
      <p class="hint">Copyright © 2026 mahziyarng98. Licence: GPL-3.0-or-later.</p>
      <p class="hint">Engine: cawget-core (Rust).</p>
      <p class="hint selectable">Queue database: <bdi>{dbPath}</bdi></p>
      {#if migratedFrom}<p class="hint selectable">Copied at this start from <bdi>{migratedFrom}</bdi> (the old file was left unchanged).</p>{/if}
      {#if migrationDeferred}<p class="hint">The old queue is in use by another program, so it is used in place for now; it will be copied on a later start.</p>{/if}
    </div>
  </div>
  {#snippet buttons()}
    <button class="btn primary" data-autofocus onclick={onclose}>Close</button>
  {/snippet}
</Modal>
