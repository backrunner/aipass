import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { createPublicKey, generateKeyPairSync } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { build, type Plugin } from "vite";
import { defineConfig } from "vitest/config";

const classicContentScripts = [
  {
    entry: resolve(__dirname, "src/content/detector.ts"),
    fileName: "content.js",
    name: "AIPassContentScript"
  },
  {
    entry: resolve(__dirname, "src/content/clipboard-bridge.ts"),
    fileName: "clipboardBridge.js",
    name: "AIPassClipboardBridge"
  }
];

function classicContentScriptBuild(): Plugin {
  return {
    name: "aipass-classic-content-script-build",
    apply: "build",
    buildStart() {
      for (const script of classicContentScripts) {
        this.addWatchFile(script.entry);
      }
      this.addWatchFile(resolve(__dirname, "src/content/secret-scanner.ts"));
      this.addWatchFile(resolve(__dirname, "src/content/page-capture-policy.ts"));
      this.addWatchFile(resolve(__dirname, "src/content/standalone-secret.ts"));
      this.addWatchFile(resolve(__dirname, "../../packages/schemas/src/index.ts"));
    },
    async closeBundle() {
      for (const script of classicContentScripts) {
        await build({
          configFile: false,
          root: __dirname,
          publicDir: false,
          logLevel: "warn",
          resolve: {
            dedupe: ["svelte", "bits-ui"]
          },
          build: {
            outDir: "dist",
            emptyOutDir: false,
            copyPublicDir: false,
            lib: {
              entry: script.entry,
              formats: ["iife"],
              name: script.name,
              fileName: () => script.fileName
            },
            rollupOptions: {
              output: {
                extend: true
              }
            }
          }
        });
      }
    }
  };
}

function extensionManifestKey(): Plugin {
  return {
    name: "aipass-extension-manifest-key",
    apply: "build",
    async writeBundle() {
      const manifestPath = resolve(__dirname, "dist", "manifest.json");
      const manifest = JSON.parse(await readFile(manifestPath, "utf8")) as Record<string, unknown>;
      const key = await extensionPublicKey();
      if (manifest.key === key) return;
      await writeFile(manifestPath, `${JSON.stringify({ ...manifest, key }, null, 2)}\n`);
    }
  };
}

async function extensionPublicKey() {
  const privateKey = await extensionPrivateKey();
  return createPublicKey(privateKey)
    .export({
      type: "spki",
      format: "der"
    })
    .toString("base64");
}

async function extensionPrivateKey() {
  if (process.env.AIPASS_EXTENSION_PRIVATE_KEY) {
    return process.env.AIPASS_EXTENSION_PRIVATE_KEY.replaceAll(String.raw`\n`, "\n");
  }

  const keyPath = resolve(
    process.env.AIPASS_EXTENSION_KEY_PATH ?? resolve(__dirname, "chrome-extension.pem")
  );
  try {
    return await readFile(keyPath, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    const { privateKey } = generateKeyPairSync("rsa", {
      modulusLength: 2048,
      publicExponent: 0x10001
    });
    const privateKeyPem = privateKey.export({
      type: "pkcs8",
      format: "pem"
    });
    await writeFile(keyPath, privateKeyPem, { mode: 0o600 });
    return privateKeyPem;
  }
}

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() }), classicContentScriptBuild(), extensionManifestKey()],
  resolve: {
    dedupe: ["svelte", "bits-ui"]
  },
  optimizeDeps: {
    exclude: ["@aipass/ui"]
  },
  test: {
    environment: "happy-dom"
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        popup: resolve(__dirname, "popup.html"),
        serviceWorker: resolve(__dirname, "src/service-worker.ts")
      },
      output: {
        entryFileNames: "[name].js",
        chunkFileNames: "chunks/[name].js",
        assetFileNames: "assets/[name][extname]"
      }
    }
  }
});
