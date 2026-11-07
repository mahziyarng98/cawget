<script lang="ts">
  // Options, in tabs. Engine settings are saved as soon as a value is committed (same storage as cawget-cli
  // config); UI preferences are saved by the app. Controls without backend support are disabled ("Coming soon").
  import { open } from "@tauri-apps/plugin-dialog";
  import { type AppInfo, type SettingsDto, api } from "./api";
  import { PRODUCT_NAME } from "./brand";
  import { FILE_CATEGORIES } from "./categories";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import TabStrip from "./TabStrip.svelte";
  import { type ProxyMode, type SpeedUnit, hhmm, proxyMode, proxySetting, speedSetting, speedToEditor } from "./options";

  let {
    settings,
    info,
    theme,
    dir,
    showComplete,
    progressOnStart,
    tray,
    onchange,
    ontheme,
    ondir,
    onshowcomplete,
    onprogressonstart,
    onclosetotray,
    onerror,
    onclose,
  }: {
    settings: SettingsDto;
    info: AppInfo | null;
    theme: "dark" | "light";
    dir: "ltr" | "rtl";
    showComplete: boolean;
    progressOnStart: boolean;
    tray: { available: boolean; close_to_tray: boolean };
    onchange: () => void;
    ontheme: (t: "dark" | "light") => void;
    ondir: (d: "ltr" | "rtl") => void;
    onshowcomplete: (v: boolean) => void;
    onprogressonstart: (v: boolean) => void;
    onclosetotray: (v: boolean) => void;
    onerror: (m: string) => void;
    onclose: () => void;
  } = $props();

  const TABS = [
    { id: "general", label: "General" },
    { id: "types", label: "Categories" },
    { id: "saveto", label: "Folders" },
    { id: "connection", label: "Connection" },
    { id: "proxy", label: "Proxy" },
    { id: "logins", label: "Saved logins" },
    { id: "downloads", label: "Advanced" },
    { id: "sounds", label: "Sounds" },
  ] as const;
  type TabId = (typeof TABS)[number]["id"];
  let tab = $state<TabId>("general");

  const v = $derived(settings.values);
  let saved = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function save(key: string, value: string) {
    try {
      await api.setSetting(key, value);
      saved = true;
      clearTimeout(timer);
      timer = setTimeout(() => (saved = false), 1400);
    } catch (e) {
      onerror(String(e));
    }
    onchange();
  }

  async function pickFolder(current: string | null, apply: (p: string) => void) {
    const picked = await open({ directory: true, defaultPath: current ?? settings.default_dir });
    if (typeof picked === "string") apply(picked);
  }

  // speed limit editor
  const speed = $derived(speedToEditor(v.max_speed));
  // proxy editor (local until a valid address is committed)
  // svelte-ignore state_referenced_locally
  let pmode = $state<ProxyMode>(proxyMode(settings.values.proxy).mode);
  // svelte-ignore state_referenced_locally
  let purl = $state(proxyMode(settings.values.proxy).url);
  let proxyErr = $state("");
  function commitProxy() {
    const s = proxySetting(pmode, purl);
    if (s === null) {
      proxyErr = "Enter an address like http://proxy.example:3128 or socks5://127.0.0.1:1080";
      return;
    }
    proxyErr = "";
    save("proxy", s);
  }

  const SOON = "Coming soon";
</script>

<Modal title="Options" width={760} {onclose}>
  <TabStrip tabs={TABS} bind:value={tab} label="Option groups" tabId={(id) => `opt-tab-${id}`} panelId={() => "opt-panel"} />

  <div class="tab-panel options-panel" role="tabpanel" id="opt-panel" aria-labelledby="opt-tab-{tab}">
    {#if tab === "general"}
      <div class="opt-row"><span class="opt-label">Theme</span>
        <div class="seg-ctl" role="radiogroup" aria-label="Theme">
          <button role="radio" aria-checked={theme === "dark"} class:on={theme === "dark"} onclick={() => ontheme("dark")}>Dark</button>
          <button role="radio" aria-checked={theme === "light"} class:on={theme === "light"} onclick={() => ontheme("light")}>Light</button>
        </div>
      </div>
      <div class="opt-row"><span class="opt-label">Layout direction</span>
        <div class="seg-ctl" role="radiogroup" aria-label="Layout direction">
          <button role="radio" aria-checked={dir === "ltr"} class:on={dir === "ltr"} onclick={() => ondir("ltr")}>Left to right</button>
          <button role="radio" aria-checked={dir === "rtl"} class:on={dir === "rtl"} onclick={() => ondir("rtl")}>Right to left</button>
        </div>
      </div>
      <label class="check"><input type="checkbox" checked={tray.close_to_tray} disabled={!tray.available} onchange={(e) => onclosetotray(e.currentTarget.checked)} /> The close button hides {PRODUCT_NAME} to the system tray
        {#if !tray.available}<span class="hint soon">· no system tray available</span>{/if}</label>
      <label class="check" title={SOON}><input type="checkbox" disabled /> Start {PRODUCT_NAME} when I log in <span class="hint soon">· {SOON}</span></label>
      <label class="check" title={SOON}><input type="checkbox" disabled /> Watch the clipboard for download links <span class="hint soon">· {SOON}</span></label>
      <label class="check" title={SOON}><input type="checkbox" disabled /> Take over downloads from web browsers <span class="hint soon">· {SOON}</span></label>
    {:else if tab === "types"}
      <p class="hint">Downloads are sorted into these categories by file extension (the same table is used by cawget-cli).</p>
      <table class="conn-table types-table">
        <thead><tr><th>Category</th><th>Extensions</th></tr></thead>
        <tbody>
          {#each FILE_CATEGORIES as c (c.id)}<tr><td>{c.label}</td><td class="wrap-cell" dir="ltr">{c.ext.join(" ")}</td></tr>{/each}
        </tbody>
      </table>
      <button class="btn" disabled title={SOON}>Edit categories… <span class="soon">· {SOON}</span></button>
    {:else if tab === "saveto"}
      <div class="form-grid wide">
        <label for="opt-dir">Default folder</label>
        <div class="inline">
          <input id="opt-dir" class="input" dir="ltr" value={v.download_dir ?? ""} placeholder={settings.default_dir} onchange={(e) => save("download_dir", e.currentTarget.value)} />
          <button class="btn" onclick={() => pickFolder(v.download_dir, (p) => save("download_dir", p))}><Icon name="folder" size={14} /> <span>Browse…</span></button>
        </div>
        {#each FILE_CATEGORIES as c (c.id)}
          <label for="opt-dir-{c.id}">{c.label}</label>
          <div class="inline">
            <input id="opt-dir-{c.id}" class="input" dir="ltr" value={v.category_dirs[c.id] ?? ""} placeholder="Default folder" onchange={(e) => save(`dir_${c.id}`, e.currentTarget.value)} />
            <button class="btn" onclick={() => pickFolder(v.category_dirs[c.id] ?? null, (p) => save(`dir_${c.id}`, p))}><Icon name="folder" size={14} /> <span>Browse…</span></button>
            <button class="btn" disabled={!v.category_dirs[c.id]} onclick={() => save(`dir_${c.id}`, "")}>Clear</button>
          </div>
        {/each}
      </div>
      <p class="hint">Used when you do not pick a folder yourself. Empty means the default folder. Applies to the app and to cawget-cli.</p>
    {:else if tab === "connection"}
      <div class="opt-row"><label class="opt-label" for="opt-seg">Connections per file</label>
        <div class="range"><input id="opt-seg" type="range" min="1" max="32" value={v.segments} onchange={(e) => save("segments", e.currentTarget.value)} /><output class="num">{v.segments}</output></div></div>
      <div class="opt-row"><label class="opt-label" for="opt-jobs">Files at the same time</label>
        <div class="range"><input id="opt-jobs" type="range" min="1" max="10" value={v.max_jobs} onchange={(e) => save("max_jobs", e.currentTarget.value)} /><output class="num">{v.max_jobs}</output></div></div>
      <div class="opt-row"><span class="opt-label">Speed limit (all downloads)</span>
        <div class="inline">
          <label class="check"><input type="checkbox" checked={v.max_speed === 0} onchange={(e) => save("max_speed", e.currentTarget.checked ? "" : "500K")} /> Unlimited</label>
          <input class="input num" style="max-inline-size:100px" type="number" min="1" aria-label="Speed limit" disabled={v.max_speed === 0} value={speed.value || ""} onchange={(e) => save("max_speed", speedSetting(Number(e.currentTarget.value), speed.unit))} />
          <select class="input" style="max-inline-size:100px" aria-label="Unit" disabled={v.max_speed === 0} value={speed.unit} onchange={(e) => save("max_speed", speedSetting(speed.value || 1, e.currentTarget.value as SpeedUnit))}>
            <option value="K">KiB/s</option><option value="M">MiB/s</option>
          </select>
        </div></div>
      <div class="opt-row"><label class="opt-label" for="opt-retries">Retries per connection</label>
        <div class="range"><input id="opt-retries" type="range" min="0" max="20" value={v.retries} onchange={(e) => save("retries", e.currentTarget.value)} /><output class="num">{v.retries}</output></div></div>
      <div class="opt-row"><label class="opt-label" for="opt-timeout">Connection timeout (seconds)</label>
        <input id="opt-timeout" class="input num" style="max-inline-size:100px" type="number" min="1" max="600" value={v.connect_timeout} onchange={(e) => save("connect_timeout", e.currentTarget.value)} /></div>
      <div class="opt-row"><label class="opt-label" for="opt-minseg">Smallest piece (KiB)</label>
        <input id="opt-minseg" class="input num" style="max-inline-size:100px" type="number" min="64" value={Math.round(v.min_segment_size / 1024)} onchange={(e) => save("min_segment_size", `${e.currentTarget.value}K`)} /></div>
    {:else if tab === "proxy"}
      <div class="radio-list" role="radiogroup" aria-label="Proxy">
        <label class="check"><input type="radio" name="proxy" checked={pmode === "system"} onchange={() => { pmode = "system"; commitProxy(); }} /> Use the system proxy (environment variables such as HTTPS_PROXY and NO_PROXY)</label>
        <label class="check"><input type="radio" name="proxy" checked={pmode === "none"} onchange={() => { pmode = "none"; commitProxy(); }} /> No proxy (direct connections)</label>
        <label class="check"><input type="radio" name="proxy" checked={pmode === "manual"} onchange={() => (pmode = "manual")} /> Use this proxy:</label>
      </div>
      <div class="inline">
        <input class="input" dir="ltr" aria-label="Proxy address" placeholder="http://proxy.example:3128 or socks5://127.0.0.1:1080" disabled={pmode !== "manual"} bind:value={purl} onkeydown={(e) => e.key === "Enter" && commitProxy()} />
        <button class="btn" disabled={pmode !== "manual"} onclick={commitProxy}>Apply</button>
      </div>
      {#if proxyErr}<p class="error-text" role="alert">{proxyErr}</p>{/if}
      <p class="hint">A login can be included as http://user:password@host:port; it is kept in your system keyring (GNOME Keyring / KWallet), and only a reference is stored in the local settings. The setting applies to downloads that start after the change.</p>
    {:else if tab === "logins"}
      <p class="hint">Saved logins per site are not supported yet. A user name and password for one download can be given in Add downloads (Log in to the server).</p>
      <table class="conn-table" title={SOON}><thead><tr><th>Site</th><th>User name</th></tr></thead><tbody><tr><td colspan="2" class="hint">{SOON}</td></tr></tbody></table>
      <div class="inline"><button class="btn" disabled title={SOON}>Add…</button><button class="btn" disabled title={SOON}>Remove</button></div>
    {:else if tab === "downloads"}
      <label class="check"><input type="checkbox" checked={showComplete} onchange={(e) => onshowcomplete(e.currentTarget.checked)} /> Show a dialog when a download completes</label>
      <label class="check"><input type="checkbox" checked={progressOnStart} onchange={(e) => onprogressonstart(e.currentTarget.checked)} /> Show the progress window when I start a download</label>
      <div class="opt-row"><span class="opt-label">Only start new downloads between</span>
        <div class="inline">
          <input type="checkbox" aria-label="Use a daily start window" checked={v.window != null} onchange={(e) => save("window", e.currentTarget.checked ? "01:00-07:00" : "")} />
          <input dir="ltr" class="input num" type="time" aria-label="Window start" disabled={!v.window} value={v.window ? hhmm(v.window[0]) : "01:00"} onchange={(e) => save("window", `${e.currentTarget.value}-${hhmm(v.window ? v.window[1] : 420)}`)} />
          <span>–</span>
          <input dir="ltr" class="input num" type="time" aria-label="Window end" disabled={!v.window} value={v.window ? hhmm(v.window[1]) : "07:00"} onchange={(e) => save("window", `${hhmm(v.window ? v.window[0] : 60)}-${e.currentTarget.value}`)} />
        </div></div>
      <div class="form-grid wide">
        <label for="opt-ua">User agent</label>
        <input id="opt-ua" class="input" dir="ltr" value={v.user_agent} onchange={(e) => save("user_agent", e.currentTarget.value)} />
        <label for="opt-ca">Extra CA certificate (PEM)</label>
        <div class="inline">
          <input id="opt-ca" class="input" dir="ltr" value={v.cacert ?? ""} placeholder="System trust store only" onchange={(e) => save("cacert", e.currentTarget.value)} />
          <button class="btn" onclick={async () => { const p = await open({ filters: [{ name: "PEM certificates", extensions: ["pem", "crt"] }] }); if (typeof p === "string") save("cacert", p); }}>Browse…</button>
        </div>
      </div>
      <p class="hint">Existing file names are never overwritten when the name comes from the server: "file (1).ext" is used instead.</p>
    {:else}
      <p class="hint">{PRODUCT_NAME} has no sounds yet. Original sounds may be added later.</p>
      <label class="check" title={SOON}><input type="checkbox" disabled /> Play a sound when a download completes <span class="hint soon">· {SOON}</span></label>
      <label class="check" title={SOON}><input type="checkbox" disabled /> Play a sound when a download fails <span class="hint soon">· {SOON}</span></label>
    {/if}
  </div>

  {#snippet buttons()}
    <span class="saved" class:show={saved} role="status">Saved</span>
    {#if info}<span class="hint grow-left"><bdi>{info.db_path}</bdi></span>{/if}
    <button class="btn primary" onclick={onclose}>Close</button>
  {/snippet}
</Modal>
