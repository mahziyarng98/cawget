<script lang="ts">
  // File details: what the server reported, where to save, then start now or later.
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { joinPath, saveTarget, splitPath } from "./addflow";
  import { type ProbeInfo, api, fmtBytes } from "./api";
  import { FILE_CATEGORIES, categoryOf } from "./categories";
  import Modal from "./Modal.svelte";

  let {
    url,
    probe,
    probeError,
    defaultDir,
    onstart,
    onlater,
    onclose,
  }: {
    url: string;
    probe: ProbeInfo | null;
    probeError: string | null;
    defaultDir: string;
    onstart: (target: { dir?: string; output?: string }) => void;
    onlater: (target: { dir?: string; output?: string }) => void;
    onclose: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let saveAs = $state(probe ? joinPath(defaultDir, probe.file_name) : defaultDir.replace(/\/?$/, "/"));
  let exists = $state(false);
  const category = $derived(FILE_CATEGORIES.find((c) => c.id === categoryOf(probe?.file_name ?? splitPath(saveAs).name))?.label ?? "Other");

  $effect(() => {
    const t = saveTarget(saveAs);
    if (!t.output) { exists = false; return; }
    api.fileExists(t.output).then((e) => (exists = e)).catch(() => (exists = false));
  });

  async function browse() {
    const { dir, name } = splitPath(saveAs);
    const picked = name
      ? await save({ defaultPath: saveAs || joinPath(dir || defaultDir, name) })
      : await open({ directory: true, defaultPath: dir || defaultDir });
    if (typeof picked === "string") saveAs = name ? picked : picked.replace(/\/?$/, "/");
  }
</script>

<Modal title="File details" width={600} {onclose}>
  <div class="form-grid wide">
    <label for="fi-url">Address</label>
    <input id="fi-url" class="input selectable" dir="ltr" readonly value={url} />

    <label for="fi-cat">Category</label>
    <div class="inline">
      <select id="fi-cat" class="input" disabled title="Coming soon: choosing a category (and its folder) is not supported yet">
        <option>{category}</option>
      </select>
      <span class="hint soon">Detected from the file type · choosing one: Coming soon</span>
    </div>

    <label for="fi-save">Save as</label>
    <div class="inline">
      <input id="fi-save" class="input" dir="ltr" data-autofocus bind:value={saveAs} onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); onstart(saveTarget(saveAs)); } }} />
      <button class="btn" onclick={browse}>Browse…</button>
    </div>

    <label for="fi-desc">Notes</label>
    <textarea id="fi-desc" class="input" rows="2" disabled title="Coming soon: notes are not stored yet" placeholder="Coming soon"></textarea>
  </div>

  <div class="info-box" role="status">
    {#if probeError}
      <span class="error-text">The server could not be checked: {probeError}. You can still add the download; it will be retried.</span>
    {:else if probe}
      <span>Size: <bdi>{probe.size != null ? fmtBytes(probe.size) : "unknown"}</bdi></span>
      <span>Resume: {probe.resumable ? "supported (parallel connections)" : "not supported (single connection)"}</span>
      {#if probe.final_url !== url}<span class="ellipsis" title={probe.final_url}>Redirected to <bdi>{probe.final_url}</bdi></span>{/if}
    {/if}
    {#if exists}<span class="warn-text">A file with this name exists and will be replaced when the download completes.</span>{/if}
    {#if saveAs.trim().endsWith("/")}<span class="hint">The file name will be chosen when the download starts.</span>{/if}
  </div>

  {#snippet buttons()}
    <button class="btn primary" onclick={() => onstart(saveTarget(saveAs))}>Start now</button>
    <button class="btn" onclick={() => onlater(saveTarget(saveAs))}>Start later</button>
    <button class="btn" onclick={onclose}>Cancel</button>
  {/snippet}
</Modal>
