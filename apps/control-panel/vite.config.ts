import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { relative } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const repositoryRoot = fileURLToPath(new URL("../../", import.meta.url));

export default defineConfig({
  plugins: [svelte({
    preprocess: vitePreprocess(),
    compilerOptions: {
      // Shared components live outside Vite's root. Svelte otherwise includes
      // their absolute checkout path in the committed embedded assets.
      cssHash: ({ filename, css, hash }) =>
        `svelte-${hash(`${relative(repositoryRoot, filename).replaceAll("\\", "/")}\n${css}`)}`,
    },
  })],
  build: {
    target: "es2022",
    outDir: "embedded",
    emptyOutDir: true,
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        entryFileNames: "panel.js",
        assetFileNames: "panel.[ext]",
        inlineDynamicImports: true,
      },
    },
  },
});
