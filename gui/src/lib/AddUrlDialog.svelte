<script lang="ts">
  // Add downloads: address(es) and an optional HTTP login. Continue goes to the file details step for a single address
  // (several addresses are added directly); Start later adds them stopped.
  import { type Auth, authError, parseUrls } from "./addflow";
  import Modal from "./Modal.svelte";

  let {
    initial = "",
    busy = false,
    error = "",
    onok,
    onlater,
    onclose,
  }: {
    initial?: string;
    busy?: boolean;
    error?: string;
    onok: (urls: string[], auth: Auth | null) => void;
    onlater: (urls: string[], auth: Auth | null) => void;
    onclose: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(initial); // the prefill is read once, then the field is the user's
  let useAuth = $state(false);
  let login = $state("");
  let password = $state("");
  let local = $state("");
  const urls = $derived(parseUrls(text));

  function check(): Auth | null | false {
    if (!urls.length) {
      local = "Enter an address that starts with http:// or https://";
      return false;
    }
    const a = authError(useAuth, { login, password });
    if (a) {
      local = a;
      return false;
    }
    local = "";
    return useAuth ? { login, password } : null;
  }
  function ok() {
    const a = check();
    if (a !== false) onok(urls, a);
  }
  function later() {
    const a = check();
    if (a !== false) onlater(urls, a);
  }
</script>

<Modal title="Add downloads" width={540} {onclose}>
  <label class="field-label" for="add-url">Address</label>
  <textarea
    id="add-url"
    class="input"
    dir="ltr"
    rows="3"
    data-autofocus
    bind:value={text}
    placeholder="https://example.org/file.iso"
    onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ok(); } }}
  ></textarea>
  <p class="hint">{urls.length > 1 ? `${urls.length} addresses: they are added directly, without the file details step.` : "One address per line (Shift+Enter for a new line)."}</p>

  <fieldset class="group">
    <legend><label class="check"><input type="checkbox" bind:checked={useAuth} /> Log in to the server</label></legend>
    <div class="form-grid">
      <label for="add-login">User name</label>
      <input id="add-login" class="input" autocomplete="off" disabled={!useAuth} bind:value={login} onkeydown={(e) => e.key === "Enter" && ok()} />
      <label for="add-password">Password</label>
      <input id="add-password" class="input" type="password" autocomplete="off" disabled={!useAuth} bind:value={password} onkeydown={(e) => e.key === "Enter" && ok()} />
    </div>
    <p class="hint">Sent as HTTP Basic authorization. It is kept in your system keyring (GNOME Keyring / KWallet); the queue database only stores a reference.</p>
  </fieldset>

  {#if local || error}<p class="error-text" role="alert">{local || error}</p>{/if}
  {#snippet buttons()}
    <button class="btn primary" disabled={busy} onclick={ok}>{busy ? "Checking…" : "Continue"}</button>
    <button class="btn" disabled={busy} onclick={later}>Start later</button>
    <button class="btn" onclick={onclose}>Cancel</button>
  {/snippet}
</Modal>
