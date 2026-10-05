<script lang="ts">
  import type { ProviderKind } from "@aipass/schemas";
  import { getProviderIcon } from "@aipass/schemas";

  import { initials, providerKindTone } from "../helpers";
  import { builtInProviderIcons, monochromeProviderIcons } from "../provider-icons";

  export let title: string;
  export let kind: ProviderKind = "unknown";
  export let faviconUrl: string | undefined = undefined;
  export let providerId: string | undefined = undefined;
  export let domain: string | undefined = undefined;
  export let size: "sm" | "md" | "lg" = "md";

  let faviconBroken = false;
  let lastIconUrl: string | undefined;
  $: tone = providerKindTone[kind];
  $: if (iconUrl !== lastIconUrl) {
    lastIconUrl = iconUrl;
    faviconBroken = false;
  }

  // Try built-in icon first, then cached favicon, then fallback to initials
  $: builtInIcon = providerId || domain ? getProviderIcon(providerId || "", domain) : undefined;
  $: builtInIconUrl = builtInIcon ? builtInProviderIcons[builtInIcon] : undefined;
  $: monochrome = builtInIcon ? monochromeProviderIcons.has(builtInIcon) : false;
  $: cachedFaviconUrl = faviconUrl?.startsWith("data:image/") ? faviconUrl : undefined;
  $: iconUrl = builtInIconUrl || cachedFaviconUrl;
  $: showIcon = Boolean(iconUrl) && !faviconBroken;
</script>

<span class={`provider-icon tone-${tone} size-${size}`} aria-hidden="true">
  {#if showIcon}
    {#if monochrome}
      <span class="monochrome-icon" style:--provider-icon={`url("${iconUrl}")`}></span>
    {:else}
      <img src={iconUrl} alt="" on:error={() => (faviconBroken = true)} />
    {/if}
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

  .monochrome-icon {
    width: 60%;
    height: 60%;
    background: currentColor;
    mask: var(--provider-icon) center / contain no-repeat;
    -webkit-mask: var(--provider-icon) center / contain no-repeat;
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
