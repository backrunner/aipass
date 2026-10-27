<script lang="ts">
  import 'svedocs/theme/styles.css';
  import '#lib/theme.css';
  import type { LayoutProps } from './$types';

  let { children }: LayoutProps = $props();
</script>

{@render children()}
