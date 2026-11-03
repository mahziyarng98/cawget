<script lang="ts" module>
  // Original icon set: simple strokes on a 24x24 grid. No third-party artwork.
  export const ICONS = {
    addUrl: "M10 14a4 4 0 0 0 5.7 0l2.6-2.6a4 4 0 0 0-5.7-5.7l-.9.9M14 10a4 4 0 0 0-5.7 0l-2.6 2.6a4 4 0 0 0 5.7 5.7l.9-.9M18 15v6M15 18h6",
    resume: "M8 5.5v13l10.5-6.5z",
    stop: "M7 7h10v10H7z",
    stopAll: "M5 9h9v9H5zM10 6h9v9",
    delete: "M5 7h14M10 7V5h4v2m-7 0 1 12h8l1-12M10 11v5M14 11v5",
    clearCompleted: "M5 7h14M10 7V5h4v2m-7 0 .6 7M17 7l-.4 4.5M12 18.5l2 2 4.5-4.5",
    options: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 13.5l1.6 1.2-2 3.4-1.9-.8a7 7 0 0 1-2 1.2L14.8 21h-4l-.3-2.5a7 7 0 0 1-2-1.2l-1.9.8-2-3.4 1.6-1.2a7 7 0 0 1 0-2.6L4.6 9.7l2-3.4 1.9.8a7 7 0 0 1 2-1.2L10.8 3h4l.3 2.5a7 7 0 0 1 2 1.2l1.9-.8 2 3.4-1.6 1.2a7 7 0 0 1 0 2.6z",
    scheduler: "M12 7.5V12l3 2M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z",
    runQueue: "M4 6h9M4 11h9M4 16h5M14 14v7l5.5-3.5z",
    pauseQueue: "M4 6h9M4 11h9M4 16h5M14 15h6v6h-6z",
    pageScanner: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM3.5 9h17M3.5 15h17M12 3c-2.5 3-2.5 15 0 18M12 3c2.5 3 2.5 15 0 18",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    folderOpen: "M3 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v1M3 7v10a2 2 0 0 0 2 2h12l3-8H7l-3 8",
    compressed: "M5 4h14v16H5zM12 4v2m0 2v2m0 2v2m-2 0h4v3h-4z",
    documents: "M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 15h6M9 18h4",
    music: "M9 18V6l10-2v12M9 18a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0zm10-2a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z",
    programs: "M4 5h16v14H4zM4 9h16M7 7h.01M9.5 7h.01M8 13l2 2-2 2M12 17h4",
    video: "M4 6h12v12H4zM16 10l4-2.5v9L16 14",
    other: "M6 3h8l4 4v14H6zM14 3v4h4",
    unfinished: "M12 3a9 9 0 1 0 9 9M12 3a9 9 0 0 1 9 9M12 3v9h9",
    finished: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM8 12.5l2.5 2.5L16 9.5",
    queues: "M5 6h14M5 10h14M5 14h14M5 18h8",
    all: "M12 4v10m0 0-4-4m4 4 4-4M5 19h14",
    check: "M5 12.5l4.5 4.5L19 7.5",
    dot: "M12 13.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z",
    close: "M6 6l12 12M18 6 6 18",
    chevron: "M9 6l6 6-6 6",
    chevronDown: "M6 9l6 6 6-6",
    sortUp: "M8 14l4-4 4 4",
    sortDown: "M8 10l4 4 4-4",
    info: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5m0-8v.01",
    alert: "M12 8v5m0 3.5v.01M10.3 4.3 3 17a2 2 0 0 0 1.7 3h14.6A2 2 0 0 0 21 17L13.7 4.3a2 2 0 0 0-3.4 0z",
    keyboard: "M3 7h18v10H3zM6 10h.01M9 10h.01M12 10h.01M15 10h.01M18 10h.01M7 14h10",
    theme: "M12 3a9 9 0 1 0 9 9 7 7 0 0 1-9-9z",
    more: "M6 12h.01M12 12h.01M18 12h.01",
    menu: "M4 7h16M4 12h16M4 17h16",
    sidebar: "M4 5h16v14H4zM9 5v14M6 9h1M6 12h1",
    sortAsc: "M7 18V6m0 0-3 3m3-3 3 3M13 7h7M13 12h5M13 17h3",
  } as const;
  export type IconName = keyof typeof ICONS;
</script>

<script lang="ts">
  let { name, size = 16, label }: { name: IconName; size?: number; label?: string } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={name === "resume" || name === "dot" ? "currentColor" : "none"}
  stroke="currentColor"
  stroke-width={name === "more" ? 3 : 1.7}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}><path d={ICONS[name]} /></svg>
