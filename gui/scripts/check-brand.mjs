// Brand consistency check (run by `npm run check`):
//  - the product name has one source: productName in src-tauri/tauri.conf.json;
//  - code never writes the name as a literal: the UI imports it from src/lib/brand.ts, the Tauri shell reads it from
//    the config, and the engine uses its one `product_name!` macro, which must equal productName;
//  - old product names do not appear in any user-visible file or file name, except on lines that name a legacy
//    location or environment variable (the deliberate migration and alias code and its documentation);
//  - the desktop entry and the brand SVGs agree with the config.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const gui = new URL("..", import.meta.url).pathname;
const repo = join(gui, "..");
const conf = JSON.parse(readFileSync(join(gui, "src-tauri/tauri.conf.json"), "utf8"));
const name = conf.productName;
const problems = [];
const rel = (p) => relative(repo, p);

const SKIP_DIRS = new Set([".git", "node_modules", "target", "dist", "gen"]);
function walk(dir, keep, out = []) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) {
      if (!SKIP_DIRS.has(f)) walk(p, keep, out);
    } else if (keep(p)) out.push(p);
  }
  return out;
}

// Rust sources: only the code before a trailing `#[cfg(test)]` module is shipped.
const shipped = (p, text) => (p.endsWith(".rs") ? text.split(/^#\[cfg\(test\)\]/m)[0] : text);

// 1. No literal product name in code.
const macro = join(repo, "crates/cawget-core/src/lib.rs");
const macroName = /macro_rules! product_name \{\s*\(\) => \{\s*"([^"]*)"/.exec(readFileSync(macro, "utf8"))?.[1];
if (macroName !== name) problems.push(`product_name! in ${rel(macro)} is "${macroName}", expected "${name}"`);
const code = [
  ...walk(join(gui, "src"), (p) => /\.(svelte|ts|css|html)$/.test(p)),
  join(gui, "index.html"),
  ...walk(join(gui, "src-tauri/src"), (p) => p.endsWith(".rs")),
  ...walk(join(repo, "crates"), (p) => p.endsWith(".rs") && p.includes("/src/")),
];
for (const f of code) {
  if (f.endsWith("brand.ts")) continue;
  shipped(f, readFileSync(f, "utf8"))
    .split("\n")
    .forEach((line, i) => {
      if (!line.includes(name)) return;
      if (f === macro && line.trim() === `"${name}"`) return; // the macro body itself
      problems.push(`literal product name at ${rel(f)}:${i + 1} (use PRODUCT_NAME, the config or product_name!)`);
    });
}
if (!readFileSync(join(gui, "index.html"), "utf8").includes("%PRODUCT_NAME%")) problems.push("index.html title must be %PRODUCT_NAME%");

// 2. No old names in user-visible files, except legacy locations and variables.
const OLD = /jackdaw|segget/i;
const LEGACY = /(JACKDAW|SEGGET)_DB|(jackdaw|segget)\/(jackdaw|segget)\.db|\("(jackdaw|segget)", "(jackdaw|segget)\.db"\)/;
const visible = walk(repo, (p) => {
  const r = rel(p);
  if (r === "gui/scripts/check-brand.mjs" || r === "LICENSE") return false;
  if (/^(crates\/[^/]+\/tests|gui\/tests)\//.test(r)) return false; // test code is not user-visible
  return /\.(rs|ts|svelte|css|html|json|toml|lock|md|svg|desktop|1|mjs|js)$|^(NOTICE|\.gitignore)$/.test(r.split("/").pop());
});
for (const f of walk(repo, () => true)) if (OLD.test(rel(f))) problems.push(`old name in file name ${rel(f)}`);
for (const f of visible) {
  shipped(f, readFileSync(f, "utf8"))
    .split("\n")
    .forEach((line, i) => {
      if (OLD.test(line) && !LEGACY.test(line)) problems.push(`old name at ${rel(f)}:${i + 1}: ${line.trim().slice(0, 100)}`);
    });
}

// 3. Desktop entry and brand assets agree with the config.
const desktopFile = join(repo, `packaging/${conf.mainBinaryName}.desktop`);
const desktop = readFileSync(desktopFile, "utf8");
if (!desktop.includes(`\nName=${name}\n`)) problems.push(`${rel(desktopFile)} Name= must be ${name}`);
if (!desktop.includes(`\nExec=${conf.mainBinaryName}\n`)) problems.push(`${rel(desktopFile)} Exec= must be ${conf.mainBinaryName}`);
for (const f of walk(join(gui, "assets/brand"), (p) => p.endsWith(".svg"))) {
  const svg = readFileSync(f, "utf8");
  if (!svg.includes(`<title>${name}</title>`) || !svg.includes(`aria-label="${name}"`)) {
    problems.push(`${rel(f)} must have <title>${name}</title> and aria-label="${name}"`);
  }
}

// 4. Optional local phrase list (scripts/brand-denylist.local.txt, untracked): one phrase per line, matched with exact
//    case as whole words in every text file of the repository. Blank lines and lines starting with # are ignored.
const listFile = join(gui, "scripts/brand-denylist.local.txt");
if (existsSync(listFile)) {
  const esc = (t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const phrases = readFileSync(listFile, "utf8").split("\n").map((l) => l.trim()).filter((l) => l && !l.startsWith("#"));
  const rx = phrases.map((t) => [t, new RegExp(`(?<![\\p{L}\\p{N}_])${esc(t)}(?![\\p{L}\\p{N}_])`, "u")]);
  const text = walk(repo, (p) => {
    const r = rel(p);
    return r !== "LICENSE" && !r.endsWith(".local.txt") && /\.(rs|ts|svelte|css|html|json|toml|lock|md|svg|desktop|1|mjs|js|txt)$|^(NOTICE|\.gitignore)$/.test(r.split("/").pop());
  });
  for (const f of text) {
    readFileSync(f, "utf8")
      .split("\n")
      .forEach((line, i) => {
        for (const [t, r] of rx) if (r.test(line)) problems.push(`listed phrase "${t}" at ${rel(f)}:${i + 1}`);
      });
  }
  console.log(`checked ${phrases.length} listed phrases in ${text.length} files`);
}

if (problems.length) {
  console.error("brand check failed:\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log(`brand check ok: "${name}" (binary ${conf.mainBinaryName}, id ${conf.identifier})`);
