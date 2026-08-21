<script lang="ts">
  import { DocsApp } from 'svedocs/theme';
  import components from 'virtual:svedocs/components';
  import layouts from 'virtual:svedocs/layouts';
  import themeComponents from 'virtual:svedocs/theme-components';
  import loadSearch from 'virtual:svedocs/search-loader';
  import LandingBackground from '$lib/LandingBackground.svelte';

  export let data;
</script>

<div class="docs-shell">
  <LandingBackground variant="calm" />
  <DocsApp page={data.page} pages={data.pages} tree={data.tree} search={data.search} config={data.config} {components} {layouts} {themeComponents} {loadSearch} />
</div>

<style>
  .docs-shell {
    position: relative;
    min-height: 100vh;
  }

  /* Keep the svedocs tree above the ambient background layer. */
  .docs-shell > :global(.sd-root) {
    z-index: 1;
  }
</style>
