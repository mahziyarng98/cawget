// WCAG AA check of the text roles defined in src/theme.css, for both themes.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("../src/theme.css", import.meta.url), "utf8");

function block(selector: string): Record<string, string> {
  const start = css.indexOf(selector + " {");
  const body = css.slice(start, css.indexOf("}", start));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/--([\w-]+):\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}

function resolve(t: Record<string, string>, base: Record<string, string>, name: string): string {
  let v = t[name] ?? base[name];
  for (let i = 0; i < 5 && v?.startsWith("var("); i++) {
    const ref = v.slice(6, -1);
    v = t[ref] ?? base[ref];
  }
  return v;
}

function lum(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((x) => (x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}
function ratio(a: string, b: string): number {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}
function over(rgba: string, bg: string): string {
  const [r, g, b, a] = rgba.match(/[\d.]+/g)!.map(Number);
  const base = [1, 3, 5].map((i) => parseInt(bg.slice(i, i + 2), 16));
  return "#" + [r, g, b].map((v, i) => Math.round(a * v + (1 - a) * base[i]).toString(16).padStart(2, "0")).join("");
}

const dark = block(":root");
const light = block(':root[data-theme="light"]');

// [foreground role, background roles it is used on]
const PAIRS: [string, string[]][] = [
  ["text", ["bg", "surface", "surface-2"]],
  ["text-secondary", ["bg", "surface", "surface-2"]],
  ["danger-text", ["bg", "surface", "surface-2"]],
  ["accent-text", ["bg", "surface"]],
  ["warning", ["bg", "surface"]],
  ["success", ["surface"]],
];

for (const [name, theme, base] of [["dark", dark, dark], ["light", light, dark]] as const) {
  describe(`${name} theme contrast (WCAG AA, 4.5:1)`, () => {
    for (const [fg, bgs] of PAIRS) {
      for (const bg of bgs) {
        it(`${fg} on ${bg}`, () => {
          expect(ratio(resolve(theme, base, fg), resolve(theme, base, bg))).toBeGreaterThanOrEqual(4.5);
        });
      }
    }
    it("text on a selected row", () => {
      const sel = over(resolve(theme, base, "selection-row"), resolve(theme, base, "surface"));
      expect(ratio(resolve(theme, base, "text"), sel)).toBeGreaterThanOrEqual(4.5);
      expect(ratio(resolve(theme, base, "text-secondary"), sel)).toBeGreaterThanOrEqual(4.5);
    });
    it("text on filled accent buttons", () => {
      expect(ratio(resolve(theme, base, "on-accent"), resolve(theme, base, "accent-fill"))).toBeGreaterThanOrEqual(4.5);
    });
  });
}
