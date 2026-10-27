import adapterStatic from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { svedocsPreprocess, svedocsSvelteExtensions } from 'svedocs/svelte';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { svedocs } from 'svedocs/vite';
import svedocsConfig from './svedocs.config';

export default defineConfig({
  plugins: [
    svedocs({ config: svedocsConfig }),
    tailwindcss(),
    sveltekit({
      adapter: adapterStatic({ strict: false }),
      extensions: svedocsSvelteExtensions,
      preprocess: [vitePreprocess(), svedocsPreprocess()]
    })
  ],
  server: {
    proxy: {
      '/api/releases': {
        target: 'https://api.github.com',
        changeOrigin: true,
        rewrite: () => '/repos/backrunner/aipass/releases?per_page=20'
      }
    }
  }
});
