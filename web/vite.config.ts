import { defineConfig } from "vite";

// The WASM bindings are generated into ./pkg by `npm run build:wasm`.
// Vite treats the emitted `.wasm` as an asset and rewrites the
// `new URL(..., import.meta.url)` reference in the wasm-pack glue code.
export default defineConfig({
  base: "./",
  server: {
    port: 5173,
    strictPort: false,
  },
  build: {
    target: "es2022",
    outDir: "dist",
    sourcemap: true,
  },
});
