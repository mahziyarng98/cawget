<script lang="ts">
  // Per-download window: progress (overall and per connection), speed limit, and what to do when it completes.
  import { type Job, fmtBytes, fmtEta } from "./api";
  import { PRODUCT_NAME } from "./brand";
  import Modal from "./Modal.svelte";
  import { type CompletionOptions, overallPercent, resumeLabel, segmentRows } from "./progress";
  import SegmentBar from "./SegmentBar.svelte";
  import TabStrip from "./TabStrip.svelte";
  import { statusText } from "./table";

  let {
    job,
    globalLimit,
    showCompleteDialog,
    completion,
    onstop,
    onresume,
    onoptions,
    onshowcomplete,
    oncompletion,
    onclose,
  }: {
    job: Job;
    globalLimit: number;
    showCompleteDialog: boolean;
    completion: CompletionOptions;
    onstop: () => void;
    onresume: () => void;
    onoptions: () => void;
    onshowcomplete: (v: boolean) => void;
    oncompletion: (o: CompletionOptions) => void;
    onclose: () => void;
  } = $props();

  const TABS = [
    { id: "status", label: "Progress" },
    { id: "limiter", label: "Speed limit" },
    { id: "completion", label: "When completed" },
  ] as const;
  let tab = $state<(typeof TABS)[number]["id"]>("status");

  const pct = $derived(overallPercent(job));
  const rows = $derived(segmentRows(job.parts));
  const running = $derived(job.state === "active" || job.state === "queued");
  const title = $derived(`${pct != null ? `${pct.toFixed(0)}% ` : ""}${job.name}`);

</script>

<Modal {title} width={640} {onclose}>
  <TabStrip tabs={TABS} bind:value={tab} label="Download details" tabId={(id) => `tab-${id}`} panelId={(id) => `panel-${id}`} />

  {#if tab === "status"}
    <div class="tab-panel" role="tabpanel" id="panel-status" aria-labelledby="tab-status">
      <dl class="props compact">
        <dt>Address</dt><dd class="selectable"><bdi>{job.url}</bdi></dd>
        <dt>Status</dt><dd class:error-text={job.state === "failed"}>{statusText(job, Date.now() / 1000)}{job.error ? ` — ${job.error}` : ""}</dd>
        <dt>File size</dt><dd><bdi>{job.size != null ? fmtBytes(job.size) : "unknown"}</bdi></dd>
        <dt>Downloaded</dt><dd><bdi>{fmtBytes(job.done)}{pct != null ? ` (${pct.toFixed(2)}%)` : ""}</bdi></dd>
        <dt>Speed</dt><dd><bdi>{job.state === "active" ? `${fmtBytes(job.speed)}/s` : "—"}</bdi></dd>
        <dt>Time left</dt><dd><bdi>{job.state === "active" && job.eta_secs != null ? fmtEta(job.eta_secs) : "—"}</bdi></dd>
        <dt>Can resume</dt><dd>{resumeLabel(job)}</dd>
      </dl>

      <div class="bar-label">Overall</div>
      <div class="overall-bar" role="progressbar" aria-label="Overall progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct != null ? Math.round(pct) : undefined}>
        <i style="width:{pct ?? 0}%"></i>
      </div>
      <div class="bar-label">Connections ({job.connections} open)</div>
      <SegmentBar parts={job.parts} size={job.size} fraction={job.fraction} state={job.state} />

      {#if rows.length}
        <div class="conn-table-wrap">
          <table class="conn-table">
            <thead><tr><th>#</th><th class="end">Start</th><th class="end">End</th><th class="end">Downloaded</th><th class="end">Progress</th><th>State</th></tr></thead>
            <tbody>
              {#each rows as r (r.start)}
                <tr>
                  <td>{r.index}</td>
                  <td class="end"><bdi>{r.start.toLocaleString()}</bdi></td>
                  <td class="end"><bdi>{r.end.toLocaleString()}</bdi></td>
                  <td class="end"><bdi>{fmtBytes(r.done)}</bdi></td>
                  <td class="end"><bdi>{r.percent.toFixed(1)}%</bdi></td>
                  <td>{r.complete ? "Completed" : running ? "Active" : "Pending"}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <p class="hint">Segment figures come from the saved progress, updated about once per second.</p>
      {:else}
        <p class="hint">{job.state === "done" ? "Completed. Per-connection details are not kept after a download completes." : job.size == null ? "Segments are planned once the server has been contacted." : "This download uses a single connection."}</p>
      {/if}
    </div>
  {:else if tab === "limiter"}
    <div class="tab-panel" role="tabpanel" id="panel-limiter" aria-labelledby="tab-limiter">
      <label class="check" title="Coming soon: per-download limits are not supported yet">
        <input type="checkbox" disabled /> Limit the speed of this download <span class="hint soon">· Coming soon</span>
      </label>
      <div class="inline" title="Coming soon">
        <input class="input" type="number" disabled placeholder="KiB/s" style="max-inline-size:120px" />
      </div>
      <p class="hint">A limit for all downloads together is available: currently <bdi>{globalLimit > 0 ? `${fmtBytes(globalLimit)}/s` : "unlimited"}</bdi>.</p>
      <button class="btn" onclick={onoptions}>Change the global limit…</button>
    </div>
  {:else}
    <div class="tab-panel" role="tabpanel" id="panel-completion" aria-labelledby="tab-completion">
      <label class="check"><input type="checkbox" checked={showCompleteDialog} onchange={(e) => onshowcomplete(e.currentTarget.checked)} /> Show a dialog when a download completes (all downloads)</label>
      <label class="check"><input type="checkbox" checked={completion.openFolder} onchange={(e) => oncompletion({ ...completion, openFolder: e.currentTarget.checked })} /> Show the folder when this download completes</label>
      <label class="check"><input type="checkbox" checked={completion.exitApp} onchange={(e) => oncompletion({ ...completion, exitApp: e.currentTarget.checked })} /> Quit {PRODUCT_NAME} when this download completes</label>
      <label class="check" title="Coming soon"><input type="checkbox" disabled /> Power off the computer when this download completes <span class="hint soon">· Coming soon</span></label>
      <p class="hint">The two per-download choices apply while the app stays open; they are not saved.</p>
    </div>
  {/if}

  {#snippet buttons()}
    <!-- One button that changes its label keeps the keyboard focus when the state flips. -->
    {#if running || job.state === "paused" || job.state === "failed"}
      <button class="btn" onclick={() => (running ? onstop() : onresume())}>{running ? "Stop" : "Resume"}</button>
    {/if}
    <button class="btn primary" onclick={onclose}>Close</button>
  {/snippet}
</Modal>
