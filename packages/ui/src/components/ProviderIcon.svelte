<script lang="ts">
  import type { ProviderKind } from "@aipass/schemas";

  import { initials, providerKindTone } from "../helpers";

  export let title: string;
  export let kind: ProviderKind = "unknown";
  export let faviconUrl: string | undefined = undefined;
  export let size: "sm" | "md" | "lg" = "md";

  let faviconBroken = false;
  let lastFaviconUrl: string | undefined = faviconUrl;
  $: tone = providerKindTone[kind];
  $: if (faviconUrl !== lastFaviconUrl) {
    lastFaviconUrl = faviconUrl;
    faviconBroken = false;
  }
  $: showFavicon = Boolean(faviconUrl) && !faviconBroken;
</script>

<span class={`provider-icon tone-${tone} size-${size}`} aria-hidden="true">
  {#if showFavicon}
    <img src={faviconUrl} alt="" on:error={() => (faviconBroken = true)} />
  {:else}
    <span class="initials">{initials(title || "?")}</span>
  {/if}
</span>

<style lang="scss">
  .provider-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    overflow: hidden;
    flex-shrink: 0;

    img {
      width: 60%;
      height: 60%;
      object-fit: contain;
    }
  }

  .size-sm {
    width: 24px;
    height: 24px;
    font-size: 10px;
  }

  .size-md {
    width: 32px;
    height: 32px;
    font-size: 11px;
  }

  .size-lg {
    width: 48px;
    height: 48px;
    font-size: 16px;
    border-radius: 10px;
  }

  .initials {
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .tone-official {
    background: var(--kind-official-soft);
    color: var(--kind-official);
  }

  .tone-third {
    background: var(--kind-third-soft);
    color: var(--kind-third);
  }

  .tone-self {
    background: var(--kind-self-soft);
    color: var(--kind-self);
  }

  .tone-custom {
    background: var(--kind-custom-soft);
    color: var(--kind-custom);
  }
</style>
