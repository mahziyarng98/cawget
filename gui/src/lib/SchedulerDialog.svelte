<script lang="ts">
  // Scheduler: only what the engine really does. A start time for selected downloads (the engine will not start
  // them earlier) and the daily start window. Queue-based scheduling is not supported yet.
  import { type Job, api } from "./api";
  import Modal from "./Modal.svelte";
  import { hhmm, localInputToUnix, nextLocalHour, unixToLocalInput } from "./options";
  import type { SettingsDto } from "./api";

  let {
    selected,
    jobs,
    settings,
    onchange,
    onerror,
    onclose,
  }: { selected: Job[]; jobs: Job[]; settings: SettingsDto; onchange: () => void; onerror: (m: string) => void; onclose: () => void } = $props();

  const SOON = "Coming soon";
  const eligible = $derived(selected.filter((j) => j.state !== "done"));
  // svelte-ignore state_referenced_locally
  let at = $state(unixToLocalInput(nextLocalHour(Date.now() / 1000)));
  let msg = $state("");
  const scheduled = $derived(jobs.filter((j) => j.state === "queued" && j.not_before > Date.now() / 1000).sort((a, b) => a.not_before - b.not_before));
  const w = $derived(settings.values.window);

  async function schedule(clear: boolean) {
    const t = clear ? 0 : localInputToUnix(at);
    if (t === null) { msg = "Pick a date and time"; return; }
    if (!clear && t <= Date.now() / 1000) { msg = "Pick a time in the future"; return; }
    try {
      const n = await api.scheduleJobs(eligible.map((j) => j.id), t);
      msg = clear ? `Start time cleared for ${n} download${n === 1 ? "" : "s"}; queued ones start as soon as a slot is free, stopped ones stay stopped` : `${n} download${n === 1 ? "" : "s"} will start at ${new Date(t * 1000).toLocaleString()}`;
    } catch (e) {
      onerror(String(e));
    }
  }
  async function setWindow(value: string) {
    try { await api.setSetting("window", value); } catch (e) { onerror(String(e)); }
    onchange();
  }
</script>

<Modal title="Scheduler" width={640} {onclose}>
  <fieldset class="group">
    <legend>Start selected downloads at</legend>
    <div class="inline">
      <input dir="ltr" class="input num" type="datetime-local" aria-label="Start time" bind:value={at} disabled={!eligible.length} />
      <button class="btn primary" disabled={!eligible.length} onclick={() => schedule(false)}>Schedule</button>
      <button class="btn" disabled={!eligible.length} title="Queued downloads then start as soon as a slot is free" onclick={() => schedule(true)}>Clear start time</button>
    </div>
    <p class="hint">{eligible.length ? `${eligible.length} incomplete download${eligible.length === 1 ? "" : "s"} selected. Stopped ones are queued again so that they start at that time.` : "Select incomplete downloads in the list first."}</p>
    {#if msg}<p role="status">{msg}</p>{/if}
  </fieldset>

  <fieldset class="group">
    <legend>Daily start window</legend>
    <div class="inline">
      <input type="checkbox" aria-label="Use a daily start window" checked={w != null} onchange={(e) => setWindow(e.currentTarget.checked ? "01:00-07:00" : "")} />
      <span>Only start new downloads between</span>
      <input dir="ltr" class="input num" type="time" aria-label="Window start" disabled={!w} value={w ? hhmm(w[0]) : "01:00"} onchange={(e) => setWindow(`${e.currentTarget.value}-${hhmm(w ? w[1] : 420)}`)} />
      <span>–</span>
      <input dir="ltr" class="input num" type="time" aria-label="Window end" disabled={!w} value={w ? hhmm(w[1]) : "07:00"} onchange={(e) => setWindow(`${hhmm(w ? w[0] : 60)}-${e.currentTarget.value}`)} />
    </div>
  </fieldset>

  <fieldset class="group" title={SOON}>
    <legend>Queues and stop times <span class="hint soon">· {SOON}</span></legend>
    <div class="inline"><label class="check"><input type="checkbox" disabled /> Stop downloads at</label><input dir="ltr" class="input" type="time" disabled /></div>
    <div class="inline"><label class="check"><input type="checkbox" disabled /> Repeat on</label>{#each ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] as d (d)}<label class="check"><input type="checkbox" disabled /> {d}</label>{/each}</div>
    <div class="inline"><label class="check"><input type="checkbox" disabled /> When the queue is done: shut down / exit</label></div>
  </fieldset>

  {#if scheduled.length}
    <div class="conn-table-wrap">
      <table class="conn-table">
        <thead><tr><th>Scheduled download</th><th>Starts</th></tr></thead>
        <tbody>{#each scheduled as j (j.id)}<tr><td><bdi>{j.name}</bdi></td><td><bdi>{new Date(j.not_before * 1000).toLocaleString()}</bdi></td></tr>{/each}</tbody>
      </table>
    </div>
  {/if}

  {#snippet buttons()}
    <button class="btn primary" data-autofocus onclick={onclose}>Close</button>
  {/snippet}
</Modal>
