import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// The product name has ONE source: `productName` in src-tauri/tauri.conf.json.
// It is injected into the frontend (`__PRODUCT_NAME__`) and the HTML title from there.
const tauriConf = JSON.parse(readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8"));
const PRODUCT_NAME: string = tauriConf.productName;
const VERSION: string = tauriConf.version;

export default defineConfig({
  plugins: [
    svelte(),
    { name: "product-name", transformIndexHtml: (html) => html.replaceAll("%PRODUCT_NAME%", PRODUCT_NAME) },
  ],
  define: { __PRODUCT_NAME__: JSON.stringify(PRODUCT_NAME), __APP_VERSION__: JSON.stringify(VERSION) },
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1", fs: { allow: [".", "../shared"] } },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
  test: { include: ["tests/**/*.test.ts"], environment: "node" },
});
