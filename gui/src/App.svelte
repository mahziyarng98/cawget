<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import AboutDialog from "./lib/AboutDialog.svelte";
  import type { Auth } from "./lib/addflow";
  import AddUrlDialog from "./lib/AddUrlDialog.svelte";
  import { type AddOptions, type AppInfo, type Job, type ProbeInfo, type SettingsDto, api, onEngineError, onEngineEvent, onJobs, onTrayAdd } from "./lib/api";
  import SchedulerDialog from "./lib/SchedulerDialog.svelte";
  import CompleteDialog from "./lib/CompleteDialog.svelte";
  import FileInfoDialog from "./lib/FileInfoDialog.svelte";
  import type { CompletionOptions } from "./lib/progress";
  import ProgressDialog from "./lib/ProgressDialog.svelte";
  import { PRODUCT_NAME, logoFor } from "./lib/brand";
  import { type CategoryId, FILE_CATEGORIES, inCategory } from "./lib/categories";
  import CategoryTree from "./lib/CategoryTree.svelte";
  import DeleteDialog from "./lib/DeleteDialog.svelte";
  import DownloadTable from "./lib/DownloadTable.svelte";
  import Icon from "./lib/Icon.svelte";
  import Menu from "./lib/Menu.svelte";
  import MenuBar from "./lib/MenuBar.svelte";
  import { type MenuItem, type ToolButton, type TopMenu, sep, soon } from "./lib/menu";
  import OptionsDialog from "./lib/OptionsDialog.svelte";
  import { loadPref, savePref } from "./lib/prefs";
  import PropertiesDialog from "./lib/PropertiesDialog.svelte";
  import { BREAKPOINTS, SIDE_WIDTH, type SideState, bandOf, clampSideWidth, menuBarCompact, nextZoom, resolveSide, sanitizeSide, sanitizeZoom, sideOnResize, sideToggle } from "./lib/responsive";
  import { type ActionId, actionFor } from "./lib/shortcuts";
  import ShortcutsDialog from "./lib/ShortcutsDialog.svelte";
  import StatusBar from "./lib/StatusBar.svelte";
  import { COLUMNS, type TableLayout, defaultLayout, sanitizeLayout, sortJobs, toggleHidden } from "./lib/table";
  import Toolbar from "./lib/Toolbar.svelte";

  type Theme = "dark" | "light";
  type Dir = "ltr" | "rtl";
  const SOON = "Coming soon";

  // ---------- state ----------
  let jobs = $state<Job[]>([]);
  let settings = $state<SettingsDto | null>(null);
  let info = $state<AppInfo | null>(null);
  let engineError = $state<string | null>(null);

  let theme = $state<Theme>(loadPref("theme", "dark", (v): v is Theme => v === "dark" || v === "light"));
  let dir = $state<Dir>(loadPref("dir", "ltr", (v): v is Dir => v === "ltr" || v === "rtl"));
  let category = $state<CategoryId>(loadPref("category", "all", (v): v is CategoryId => typeof v === "string" && v !== "queues"));
  let layout = $state<TableLayout>(sanitizeLayout(loadPref<unknown>("columns", null, (v): v is unknown => true)));
  let show = $state(loadPref("show", { toolbar: true, tree: true, status: true }, (v): v is { toolbar: boolean; tree: boolean; status: boolean } => !!v && typeof v === "object"));

  // Window-size driven layout (CSS px of the app box; zoom changes them too).
  let appEl = $state<HTMLDivElement | null>(null);
  let appW = $state(window.innerWidth);
  let appH = $state(window.innerHeight);
  let menuNeeded = $state(0);
  let sideState = $state<SideState>(sanitizeSide(loadPref<unknown>("sidebar", null, (v): v is unknown => true)));
  let sideWidth = $state(loadPref("sideWidth", SIDE_WIDTH.default, (v): v is number => typeof v === "number"));
  let zoom = $state(sanitizeZoom(loadPref<unknown>("zoom", 1, (v): v is unknown => true)));

  let selected = $state<number[]>([]);
  let cursor = $state<number | null>(null);

  type Dialog =
    | { kind: "add"; initial: string }
    | { kind: "fileinfo"; url: string; auth: Auth | null; probe: ProbeInfo | null; probeError: string | null; folder: string }
    | { kind: "scheduler" }
    | { kind: "progress"; id: number }
    | { kind: "delete"; ids: number[] }
    | { kind: "props"; id: number }
    | { kind: "options" }
    | { kind: "about" }
    | { kind: "keys" };
  let dialog = $state<Dialog | null>(null);
  let ctx = $state<{ x: number; y: number } | null>(null);
  let toasts = $state<{ id: number; kind: "ok" | "err" | "info"; text: string }[]>([]);
  let menubar = $state<MenuBar | null>(null);
  let addBusy = $state(false);
  let addError = $state("");
  let showComplete = $state(loadPref("showCompleteDialog", true, (v): v is boolean => typeof v === "boolean"));
  let completeQueue = $state<number[]>([]);
  let completion = $state<Record<number, CompletionOptions>>({});
  let progressOnStart = $state(loadPref("progressOnStart", false, (v): v is boolean => typeof v === "boolean"));
  let tray = $state({ available: false, close_to_tray: false });

  // ---------- persistence and document attributes ----------
  $effect(() => {
    document.documentElement.dataset.theme = theme;
    savePref("theme", theme);
  });
  $effect(() => {
    document.documentElement.dir = dir;
    savePref("dir", dir);
  });
  $effect(() => savePref("category", category));
  $effect(() => savePref("columns", layout));
  $effect(() => savePref("show", show));
  $effect(() => savePref("showCompleteDialog", showComplete));
  $effect(() => savePref("progressOnStart", progressOnStart));
  $effect(() => savePref("sidebar", sideState));
  $effect(() => savePref("sideWidth", sideWidth));
  $effect(() => {
    savePref("zoom", zoom);
    getCurrentWebview().setZoom(zoom).catch(() => {});
  });
  $effect(() => {
    if (!appEl) return;
    const el = appEl;
    const ro = new ResizeObserver(() => {
      appW = el.clientWidth;
      appH = el.clientHeight;
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  // ---------- responsive layout ----------
  const band = $derived(bandOf(appW));
  // Crossing the breakpoint ends the user's explicit sidebar choice (it then follows the window again).
  $effect(() => {
    const next = sideOnResize(sideState, band);
    if (next !== sideState) sideState = next;
  });
  const sideMode = $derived(resolveSide(sideState, band));
  /** Full tree requested while the window is narrow: shown as a drawer over the list. */
  const drawer = $derived(show.tree && band === "narrow" && sideMode === "full");
  const sideW = $derived(clampSideWidth(sideWidth, appW));
  const compact = $derived(appW < BREAKPOINTS.compact);
  const short = $derived(appH < BREAKPOINTS.shortHeight);
  const menuCompact = $derived(menuBarCompact(appW, menuNeeded));
  const toggleSidebar = () => {
    if (!show.tree) show = { ...show, tree: true };
    else sideState = sideToggle(sideState, band);
  };
  const closeDrawer = () => drawer && (sideState = sideToggle(sideState, band));

  function splitterDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    const h = e.currentTarget as HTMLElement;
    h.setPointerCapture(e.pointerId);
    const [x0, w0] = [e.clientX, sideW];
    const rtl = document.documentElement.dir === "rtl";
    const move = (ev: PointerEvent) => (sideWidth = clampSideWidth(w0 + (ev.clientX - x0) * (rtl ? -1 : 1), appW));
    const up = () => {
      h.removeEventListener("pointermove", move);
      h.removeEventListener("pointerup", up);
    };
    h.addEventListener("pointermove", move);
    h.addEventListener("pointerup", up);
  }
  function splitterKey(e: KeyboardEvent) {
    const rtl = document.documentElement.dir === "rtl";
    const grow = rtl ? "ArrowLeft" : "ArrowRight";
    const shrink = rtl ? "ArrowRight" : "ArrowLeft";
    if (e.key === grow || e.key === shrink) sideWidth = clampSideWidth(sideW + (e.key === grow ? 16 : -16), appW);
    else if (e.key === "Home") sideWidth = SIDE_WIDTH.min;
    else if (e.key === "End") sideWidth = clampSideWidth(SIDE_WIDTH.max, appW);
    else if (e.key === "Enter") sideWidth = SIDE_WIDTH.default;
    else return;
    e.preventDefault();
  }

  // ---------- derived ----------
  const logo = $derived(logoFor(theme));
  const rows = $derived(sortJobs(jobs.filter((j) => inCategory(j, category)), layout.sort));
  const byId = $derived(new Map(jobs.map((j) => [j.id, j])));
  const sel = $derived(selected.map((id) => byId.get(id)).filter((j): j is Job => !!j));
  const one = $derived(sel.length === 1 ? sel[0] : null);
  const counts = $derived.by(() => {
    const c = { all: jobs.length, unfinished: 0, finished: 0, queues: 0 } as Record<CategoryId, number>;
    for (const fc of FILE_CATEGORIES) c[fc.id] = 0;
    for (const j of jobs) for (const id of Object.keys(c) as CategoryId[]) if (id !== "all" && inCategory(j, id)) c[id]++;
    return c;
  });
  const active = $derived(jobs.filter((j) => j.state === "active"));
  const waiting = $derived(jobs.filter((j) => j.state === "queued"));
  const speed = $derived(active.reduce((s, j) => s + j.speed, 0));

  const canResume = $derived(sel.some((j) => j.state === "paused" || j.state === "failed"));
  const canStop = $derived(sel.some((j) => j.state === "active" || j.state === "queued"));
  const canStopAll = $derived(jobs.some((j) => j.state === "active" || j.state === "queued"));
  const hasCompleted = $derived(jobs.some((j) => j.state === "done"));

  // Drop selections of rows that disappeared.
  $effect(() => {
    const ids = new Set(jobs.map((j) => j.id));
    if (selected.some((id) => !ids.has(id))) selected = selected.filter((id) => ids.has(id));
    if (cursor != null && !ids.has(cursor)) cursor = null;
  });

  // ---------- helpers ----------
  let toastSeq = 0;
  function toast(kind: "ok" | "err" | "info", text: string) {
    const id = ++toastSeq;
    toasts.push({ id, kind, text });
    setTimeout(() => (toasts = toasts.filter((t) => t.id !== id)), kind === "err" ? 7000 : 4000);
  }
  async function guarded(fn: () => Promise<unknown>) {
    try {
      await fn();
    } catch (e) {
      toast("err", String(e));
    }
  }
  async function refreshSettings() {
    settings = await api.getSettings();
  }

  // ---------- actions ----------
  const authOpts = (a: Auth | null): AddOptions => (a ? { login: a.login, password: a.password } : {});

  async function addUrls(urls: string[], o: AddOptions): Promise<number[] | null> {
    try {
      const ids = await api.addDownloads(urls.join("\n"), o);
      const what = ids.length === 1 ? "Download" : `${ids.length} downloads`;
      toast("info", o.paused ? `${what} added, not started (Resume starts it)` : `${what} added`);
      if (category === "finished") category = "all";
      return ids;
    } catch (e) {
      toast("err", String(e));
      return null;
    }
  }

  // Add downloads -> Continue: one address is checked with the server first and continues to "file details".
  async function addOk(urls: string[], auth: Auth | null) {
    addError = "";
    if (urls.length > 1) {
      if (await addUrls(urls, authOpts(auth))) dialog = null;
      return;
    }
    addBusy = true;
    let probe: ProbeInfo | null = null;
    let probeError: string | null = null;
    try {
      probe = await api.probeUrl(urls[0], auth?.login, auth?.password);
    } catch (e) {
      probeError = String(e);
    }
    // The folder the engine would use for this file (category folder or default folder).
    let folder = settings?.default_dir ?? "";
    if (probe) folder = await api.suggestFolder(probe.file_name).catch(() => folder);
    addBusy = false;
    if (dialog?.kind === "add") dialog = { kind: "fileinfo", url: urls[0], auth, probe, probeError, folder };
  }

  async function addLater(urls: string[], auth: Auth | null) {
    if (await addUrls(urls, { ...authOpts(auth), paused: true })) dialog = null;
  }

  async function fileInfoDone(d: Extract<Dialog, { kind: "fileinfo" }>, target: { dir?: string; output?: string }, paused: boolean) {
    const ids = await addUrls([d.url], { ...target, ...authOpts(d.auth), paused });
    if (!ids) return;
    dialog = !paused && progressOnStart && ids.length === 1 ? { kind: "progress", id: ids[0] } : null;
  }

  const resumeSel = () => guarded(async () => { for (const j of sel) if (j.state === "paused" || j.state === "failed") await api.resume(j.id); });
  const stopSel = () => guarded(async () => { for (const j of sel) if (j.state === "active" || j.state === "queued") await api.pause(j.id); });
  const stopAll = () => guarded(async () => { for (const j of jobs) if (j.state === "active" || j.state === "queued") await api.pause(j.id); });
  const deleteSel = () => sel.length && (dialog = { kind: "delete", ids: sel.map((j) => j.id) });
  const clearCompleted = () =>
    guarded(async () => {
      const n = await api.clearFinished();
      toast("info", `Cleared ${n} completed download${n === 1 ? "" : "s"} from the list (files kept)`);
    });
  function openJob(j: Job | null) {
    if (!j) return;
    if (j.state === "done") guarded(() => api.openFile(j.id));
    else dialog = { kind: "progress", id: j.id };
  }

  // A download finished: per-download completion choices, then the "download completed" dialog (if enabled).
  function finished(id: number) {
    const c = completion[id];
    if (c?.openFolder) guarded(() => api.openFolder(id));
    if (c?.exitApp) {
      quit();
      return;
    }
    if (showComplete && !completeQueue.includes(id)) completeQueue = [...completeQueue, id];
  }
  const openFolder = () => one && guarded(() => api.openFolder(one.id));
  const properties = () => one && (dialog = { kind: "props", id: one.id });
  const options = () => (dialog = { kind: "options" });
  const toggleTheme = () => (theme = theme === "dark" ? "light" : "dark");
  // Exit really quits, also when the close button only hides the window to the tray.
  const quit = () => guarded(() => api.quitApp());
  function openContextAtCursor() {
    const row = document.querySelector<HTMLElement>(`[data-row="${cursor ?? selected[0]}"]`);
    if (!row || !sel.length) return;
    const r = row.getBoundingClientRect();
    ctx = { x: r.left + 40, y: r.bottom };
  }

  function run(a: ActionId) {
    switch (a) {
      case "add": dialog = { kind: "add", initial: "" }; break;
      case "resume": if (canResume) resumeSel(); break;
      case "stop": if (canStop) stopSel(); break;
      case "stopAll": if (canStopAll) stopAll(); break;
      case "delete": deleteSel(); break;
      case "clearCompleted": if (hasCompleted) clearCompleted(); break;
      case "open": openJob(one); break;
      case "openFolder": openFolder(); break;
      case "properties": properties(); break;
      case "selectAll": selected = rows.map((j) => j.id); break;
      case "options": options(); break;
      case "toggleTheme": toggleTheme(); break;
      case "shortcuts": dialog = { kind: "keys" }; break;
      case "about": dialog = { kind: "about" }; break;
      case "quit": quit(); break;
      case "focusMenu": menubar?.focusBar(); break;
      case "contextMenu": openContextAtCursor(); break;
      case "nextPane": cyclePane(1); break;
      case "prevPane": cyclePane(-1); break;
      case "toggleSidebar": toggleSidebar(); break;
      case "zoomIn": zoom = nextZoom(zoom, 1); break;
      case "zoomOut": zoom = nextZoom(zoom, -1); break;
      case "zoomReset": zoom = nextZoom(zoom, 0); break;
    }
  }

  // F6 / Shift+F6: categories -> column headers (or the compact sort control) -> download list.
  function cyclePane(d: number) {
    const panes = [
      document.querySelector<HTMLElement>("[role=tree] [tabindex='0']"),
      document.querySelector<HTMLElement>("[role=columnheader][tabindex='0'], .sortbar select"),
      document.querySelector<HTMLElement>("[role=grid]"),
    ].filter((p): p is HTMLElement => !!p);
    const i = panes.findIndex((p) => p === document.activeElement || p.contains(document.activeElement));
    const a = document.activeElement;
    const at = a?.closest("[role=tree]") ? 0 : a?.closest("[role=columnheader], .sortbar") ? panes.findIndex((p) => p.closest(".grid-head, .sortbar")) : i;
    panes[((at < 0 ? -1 : at) + d + panes.length) % panes.length]?.focus();
  }

  // ---------- menus ----------
  const item = (id: string, label: string, runFn: () => void, o: Partial<Extract<MenuItem, { kind: "item" }>> = {}): MenuItem => ({ kind: "item", id, label, run: runFn, ...o });

  const menus: TopMenu[] = [
    {
      id: "file", label: "File", mnemonic: "F",
      items: () => [
        item("add", "Add download…", () => run("add"), { shortcut: "Ctrl+N", icon: "addUrl" }),
        soon("bulkAdd", "Bulk add…"),
        soon("pageScanner", "Scan a page for links…", "pageScanner"),
        sep,
        soon("export", "Export list…"),
        soon("import", "Import list…"),
        sep,
        item("quit", "Quit", quit, { shortcut: "Ctrl+Q" }),
      ],
    },
    {
      id: "selection", label: "Selection", mnemonic: "S",
      items: () => [
        item("open", "Open", () => openJob(one), { shortcut: "Enter", disabled: !(one?.state === "done"), reason: "Select one completed download" }),
        soon("openWith", "Open with…"),
        item("folder", "Show in folder", openFolder, { shortcut: "Ctrl+E", icon: "folderOpen", disabled: !one, reason: "Select one download" }),
        soon("move", "Rename or move…"),
        sep,
        item("delete", "Delete", deleteSel, { shortcut: "Delete", icon: "delete", disabled: !sel.length, reason: "Select downloads" }),
        item("clearCompleted", "Clear completed", clearCompleted, { shortcut: "Ctrl+Shift+Delete", icon: "clearCompleted", disabled: !hasCompleted, reason: "No completed downloads" }),
        sep,
        item("props", "Properties", properties, { shortcut: "Alt+Enter", disabled: !one, reason: "Select one download" }),
      ],
    },
    {
      id: "downloads", label: "Downloads", mnemonic: "D",
      items: () => [
        item("resume", "Resume", resumeSel, { shortcut: "Ctrl+R", icon: "resume", disabled: !canResume, reason: "Select stopped or failed downloads" }),
        item("stop", "Stop", stopSel, { shortcut: "Ctrl+S", icon: "stop", disabled: !canStop, reason: "Select running or waiting downloads" }),
        item("stopAll", "Stop all", stopAll, { shortcut: "Ctrl+Shift+S", icon: "stopAll", disabled: !canStopAll, reason: "Nothing is running" }),
        soon("downloadAgain", "Download again"),
        sep,
        soon("runQueue", "Run queue", "runQueue"),
        soon("pauseQueue", "Pause queue", "pauseQueue"),
        item("scheduler", "Scheduler…", () => (dialog = { kind: "scheduler" }), { icon: "scheduler" }),
        sep,
        item("speedLimit", "Speed limit… (in Options)", options),
        item("options", "Options…", options, { shortcut: "Ctrl+,", icon: "options" }),
      ],
    },
    {
      id: "view", label: "View", mnemonic: "V",
      items: () => [
        item("toolbar", "Toolbar", () => (show = { ...show, toolbar: !show.toolbar }), { role: "check", checked: show.toolbar }),
        item("tree", "Categories", () => (show = { ...show, tree: !show.tree }), { role: "check", checked: show.tree }),
        item("collapse", "Collapse categories", toggleSidebar, { role: "check", checked: show.tree && sideMode === "rail", shortcut: "Ctrl+B", disabled: !show.tree, reason: "Categories are hidden" }),
        item("status", "Status bar", () => (show = { ...show, status: !show.status }), { role: "check", checked: show.status }),
        sep,
        { kind: "group", label: "Columns" },
        ...COLUMNS.filter((c) => c.id !== "name").map((c) =>
          item(`col-${c.id}`, c.label, () => (layout = toggleHidden(layout, c.id)), { role: "check", checked: !layout.hidden.includes(c.id) }),
        ),
        item("resetCols", "Reset columns", () => (layout = defaultLayout())),
        sep,
        item("zoomIn", "Zoom in", () => run("zoomIn"), { shortcut: "Ctrl+=" }),
        item("zoomOut", "Zoom out", () => run("zoomOut"), { shortcut: "Ctrl+-" }),
        item("zoomReset", `Actual size (now ${Math.round(zoom * 100)}%)`, () => run("zoomReset"), { shortcut: "Ctrl+0" }),
        sep,
        item("dark", "Dark theme", () => (theme = "dark"), { role: "radio", checked: theme === "dark" }),
        item("light", "Light theme", () => (theme = "light"), { role: "radio", checked: theme === "light", shortcut: "Ctrl+Shift+L" }),
        sep,
        item("ltr", "Left-to-right layout", () => (dir = "ltr"), { role: "radio", checked: dir === "ltr" }),
        item("rtl", "Right-to-left layout", () => (dir = "rtl"), { role: "radio", checked: dir === "rtl" }),
      ],
    },
    {
      id: "help", label: "Help", mnemonic: "H",
      items: () => [
        item("keys", "Keyboard shortcuts", () => run("shortcuts"), { shortcut: "F1", icon: "keyboard" }),
        sep,
        item("about", `About ${PRODUCT_NAME}`, () => run("about"), { icon: "info" }),
      ],
    },
  ];

  const toolbar = $derived<ToolButton[]>([
    { id: "add", label: "Add link", icon: "addUrl", run: () => run("add"), title: "Add download (Ctrl+N)" },
    { id: "resume", label: "Resume", icon: "resume", run: resumeSel, disabled: !canResume, reason: "select stopped or failed downloads" },
    { id: "stop", label: "Stop", icon: "stop", run: stopSel, disabled: !canStop, reason: "select running or waiting downloads" },
    { id: "stopAll", label: "Stop all", icon: "stopAll", run: stopAll, disabled: !canStopAll, reason: "nothing is running" },
    { id: "delete", label: "Delete", icon: "delete", run: deleteSel, disabled: !sel.length, reason: "select downloads" },
    { id: "clearCompleted", label: "Clear completed", icon: "clearCompleted", run: clearCompleted, disabled: !hasCompleted, reason: "no completed downloads", groupEnd: true },
    { id: "options", label: "Options", icon: "options", run: options, title: "Options (Ctrl+,)" },
    { id: "scheduler", label: "Scheduler", icon: "scheduler", run: () => (dialog = { kind: "scheduler" }), title: "Start times and the daily window" },
    { id: "runQueue", label: "Run queue", icon: "runQueue", disabled: true, reason: SOON },
    { id: "pauseQueue", label: "Pause queue", icon: "pauseQueue", disabled: true, reason: SOON, groupEnd: true },
    { id: "pageScanner", label: "Page scanner", icon: "pageScanner", disabled: true, reason: SOON },
  ]);

  const rowMenu = (): MenuItem[] => [
    one && one.state !== "done"
      ? item("progress", "Show progress", () => openJob(one), { shortcut: "Enter" })
      : item("open", "Open", () => openJob(one), { shortcut: "Enter", disabled: !(one?.state === "done"), reason: "Select one completed download" }),
    soon("openWith", "Open with…"),
    item("folder", "Show in folder", openFolder, { icon: "folderOpen", disabled: !one, reason: "Select one download" }),
    soon("move", "Rename or move…"),
    soon("downloadAgain", "Download again"),
    sep,
    item("resume", "Resume", resumeSel, { icon: "resume", disabled: !canResume, reason: "Nothing to resume" }),
    item("stop", "Stop", stopSel, { icon: "stop", disabled: !canStop, reason: "Nothing to stop" }),
    item("delete", "Delete", deleteSel, { icon: "delete", shortcut: "Delete" }),
    soon("queue", "Add to queue"),
    sep,
    item("props", "Properties", properties, { shortcut: "Alt+Enter", disabled: !one, reason: "Select one download" }),
  ];

  // ---------- global keyboard ----------
  function keydown(e: KeyboardEvent) {
    if (dialog || ctx) return;
    const target = e.target as HTMLElement;
    const typing = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";
    if (e.altKey && !e.ctrlKey && /^[a-z]$/i.test(e.key) && menubar?.openByMnemonic(e.key)) {
      e.preventDefault();
      return;
    }
    const a = actionFor(e);
    if (!a) return;
    if (typing && (a === "delete" || a === "open" || a === "selectAll")) return;
    // Enter and Delete act on rows only while the table (or the page) has focus, not on focused buttons.
    if ((a === "open" || a === "delete") && target.closest("button, [role=menu], [role=tree], [role=toolbar], [role=columnheader]")) return;
    if (a === "nextPane" || a === "prevPane") { e.preventDefault(); run(a); return; }
    e.preventDefault();
    run(a);
  }

  function linksIn(text: string): string {
    return text.split(/\s+/).filter((s) => /^https?:\/\/\S+/i.test(s)).join("\n");
  }

  onMount(() => {
    // Like a desktop download manager, the list has the keyboard focus at start.
    requestAnimationFrame(() => document.querySelector<HTMLElement>("[role=grid]")?.focus());
    const unlisten: (() => void)[] = [];
    (async () => {
      jobs = await api.listJobs();
      await refreshSettings();
      info = await api.appInfo();
      engineError = info.engine_error;
      tray = await api.trayState().catch(() => tray);
      if (info.migrated_from) toast("info", `Your download queue was copied to the new location: ${info.db_path}`);
      unlisten.push(await onJobs((j) => (jobs = j)));
      unlisten.push(await onEngineError((m) => (engineError = m)));
      unlisten.push(await onTrayAdd(() => { if (!dialog) dialog = { kind: "add", initial: "" }; }));
      unlisten.push(
        await onEngineEvent((e) => {
          if (e.type === "done") {
            toast("ok", `Download completed: ${e.path.split("/").pop() ?? e.path}`);
            finished(e.id);
          }
          else toast("err", `${jobs.find((j) => j.id === e.id)?.name ?? `#${e.id}`}: ${e.error}`);
        }),
      );
    })().catch((e) => toast("err", String(e)));

    const onPaste = (e: ClipboardEvent) => {
      const el = document.activeElement as HTMLElement | null;
      if (dialog || el?.tagName === "INPUT" || el?.tagName === "TEXTAREA") return;
      const links = linksIn(e.clipboardData?.getData("text") ?? "");
      if (links) {
        e.preventDefault();
        dialog = { kind: "add", initial: links };
      }
    };
    window.addEventListener("paste", onPaste);
    return () => {
      window.removeEventListener("paste", onPaste);
      unlisten.forEach((u) => u());
    };
  });

  const dialogJob = $derived(dialog?.kind === "props" || dialog?.kind === "progress" ? byId.get(dialog.id) ?? null : null);
  const completeJob = $derived(completeQueue.length ? byId.get(completeQueue[0]) ?? null : null);
  const deleteJobs = $derived(dialog?.kind === "delete" ? dialog.ids.map((id) => byId.get(id)).filter((j): j is Job => !!j) : []);
</script>

<svelte:window
  onkeydown={keydown}
  ondragover={(e) => e.preventDefault()}
  ondrop={(e) => {
    e.preventDefault();
    const links = linksIn(e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "");
    if (links && !dialog) dialog = { kind: "add", initial: links };
  }}
/>

<div class="app" bind:this={appEl}>
  {#if menuCompact}
    <!-- Compact: the folded menu and the toolbar share one row. -->
    <div class="topbar">
      <MenuBar bind:this={menubar} bind:needed={menuNeeded} compact {menus} {logo} name={PRODUCT_NAME} />
      {#if show.toolbar}<Toolbar buttons={toolbar} {short} />{:else}<span class="topbar-name"><img src={logo} alt="" width="18" height="18" />{PRODUCT_NAME}</span>{/if}
    </div>
  {:else}
    <MenuBar bind:this={menubar} bind:needed={menuNeeded} {menus} {logo} name={PRODUCT_NAME} />
    {#if show.toolbar}<Toolbar buttons={toolbar} {short} />{/if}
  {/if}

  <div class="workspace" class:no-tree={!show.tree} class:rail={show.tree && (sideMode === "rail" || drawer)} style="--side-w:{sideW}px">
    {#if show.tree}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <aside class="side" class:rail={sideMode === "rail"} class:drawer aria-label="Categories panel" onkeydown={(e) => { if (drawer && e.key === "Escape") { e.preventDefault(); closeDrawer(); document.querySelector<HTMLElement>(".side-toggle")?.focus(); } }}>
        <div class="side-head">
          <button
            class="icon-btn side-toggle"
            aria-expanded={sideMode === "full"}
            aria-label={sideMode === "full" ? "Collapse categories" : "Expand categories"}
            title={`${sideMode === "full" ? "Collapse" : "Expand"} categories (Ctrl+B)`}
            onclick={toggleSidebar}
          ><Icon name="sidebar" size={16} /></button>
          {#if sideMode === "full"}<span class="side-title">Categories</span>{/if}
        </div>
        <CategoryTree rail={sideMode === "rail"} selected={category} {counts} onselect={(c) => { category = c; selected = []; closeDrawer(); }} />
        {#if sideMode === "full" && !drawer}
          <!-- WAI-ARIA window splitter: a focusable separator is a widget (keyboard resizes it). -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <div
            class="splitter"
            role="separator"
            aria-orientation="vertical"
            aria-label="Width of the categories (Left/Right, Home, End, Enter resets)"
            aria-valuemin={SIDE_WIDTH.min}
            aria-valuemax={clampSideWidth(SIDE_WIDTH.max, appW)}
            aria-valuenow={sideW}
            tabindex="0"
            onpointerdown={splitterDown}
            ondblclick={() => (sideWidth = SIDE_WIDTH.default)}
            onkeydown={splitterKey}
          ></div>
        {/if}
      </aside>
      {#if drawer}<div class="drawer-scrim" role="presentation" onmousedown={closeDrawer}></div>{/if}
    {/if}
    <main class="content">
      <DownloadTable
        {rows}
        bind:layout
        bind:selected
        bind:cursor
        {compact}
        onopen={openJob}
        oncontext={(x, y) => (ctx = { x, y })}
      >
        {#snippet empty()}
          <div class="empty">
            <img src={logo} alt="" width="72" height="72" />
            <p>{jobs.length ? "No downloads in this category." : "No downloads yet."}</p>
            <p class="hint">Add a link with <kbd>Ctrl+N</kbd>, paste one with <kbd>Ctrl+V</kbd>, or drop it on this window.</p>
          </div>
        {/snippet}
      </DownloadTable>
    </main>
  </div>

  {#if show.status}
    <StatusBar total={jobs.length} active={active.length} queued={waiting.length} selected={sel.length} {speed} limit={settings?.values.max_speed ?? 0} {engineError} />
  {/if}
</div>

{#if ctx}
  <div class="menu-scrim" role="presentation" onmousedown={() => (ctx = null)}></div>
  <Menu items={rowMenu()} x={ctx.x} y={ctx.y} label="Download" onclose={(refocus) => { ctx = null; if (refocus) document.querySelector<HTMLElement>("[role=grid]")?.focus(); }} />
{/if}

{#if dialog?.kind === "add"}
  <AddUrlDialog initial={dialog.initial} busy={addBusy} error={addError} onok={addOk} onlater={addLater} onclose={() => (dialog = null)} />
{:else if dialog?.kind === "fileinfo"}
  {@const d = dialog}
  <FileInfoDialog
    url={d.url}
    probe={d.probe}
    probeError={d.probeError}
    defaultDir={d.folder}
    onstart={(t) => fileInfoDone(d, t, false)}
    onlater={(t) => fileInfoDone(d, t, true)}
    onclose={() => (dialog = null)}
  />
{:else if dialog?.kind === "progress" && dialogJob}
  {@const j = dialogJob}
  <ProgressDialog
    job={j}
    globalLimit={settings?.values.max_speed ?? 0}
    showCompleteDialog={showComplete}
    completion={completion[j.id] ?? { openFolder: false, exitApp: false }}
    onstop={() => { const id = j.id; guarded(() => api.pause(id)); }}
    onresume={() => { const id = j.id; guarded(() => api.resume(id)); }}
    onoptions={() => (dialog = { kind: "options" })}
    onshowcomplete={(v) => (showComplete = v)}
    oncompletion={(o) => (completion = { ...completion, [j.id]: o })}
    onclose={() => (dialog = null)}
  />
{:else if dialog?.kind === "delete" && deleteJobs.length}
  <DeleteDialog
    jobs={deleteJobs}
    onclose={() => (dialog = null)}
    onconfirm={(files) => {
      const list = deleteJobs;
      dialog = null;
      guarded(async () => { for (const j of list) await api.remove(j.id, files && j.state === "done"); });
    }}
  />
{:else if dialog?.kind === "props" && dialogJob}
  <PropertiesDialog job={dialogJob} onclose={() => (dialog = null)} />
{:else if dialog?.kind === "options" && settings}
  <OptionsDialog
    {settings}
    {info}
    {theme}
    {dir}
    {showComplete}
    {progressOnStart}
    {tray}
    onchange={refreshSettings}
    ontheme={(t) => (theme = t)}
    ondir={(d) => (dir = d)}
    onshowcomplete={(v) => (showComplete = v)}
    onprogressonstart={(v) => (progressOnStart = v)}
    onclosetotray={(v) => guarded(async () => { await api.setCloseToTray(v); tray = { ...tray, close_to_tray: v }; })}
    onerror={(m) => toast("err", m)}
    onclose={() => (dialog = null)}
  />
{:else if dialog?.kind === "scheduler" && settings}
  <SchedulerDialog selected={sel} {jobs} {settings} onchange={refreshSettings} onerror={(m) => toast("err", m)} onclose={() => (dialog = null)} />
{:else if dialog?.kind === "about"}
  <AboutDialog {logo} dbPath={info?.db_path ?? ""} migratedFrom={info?.migrated_from ?? null} migrationDeferred={info?.migration_deferred ?? null} onclose={() => (dialog = null)} />
{:else if dialog?.kind === "keys"}
  <ShortcutsDialog onclose={() => (dialog = null)} />
{:else if !dialog && completeJob}
  {@const c = completeJob}
  <CompleteDialog
    job={c}
    onopen={() => { const id = c.id; completeQueue = completeQueue.slice(1); guarded(() => api.openFile(id)); }}
    onfolder={() => { const id = c.id; completeQueue = completeQueue.slice(1); guarded(() => api.openFolder(id)); }}
    ondontshow={(v) => (showComplete = !v)}
    onclose={() => (completeQueue = completeQueue.slice(1))}
  />
{/if}

<div class="toasts" aria-live="polite">
  {#each toasts as t (t.id)}<div class="toast {t.kind}" role="status">{t.text}</div>{/each}
</div>
