import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { svedocs } from 'svedocs/vite';
import svedocsConfig from './svedocs.config';

export default defineConfig({
  plugins: [svedocs({ config: svedocsConfig }), tailwindcss(), sveltekit()],
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
