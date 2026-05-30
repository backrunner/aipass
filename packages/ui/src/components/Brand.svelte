<script lang="ts">
  import Logo from "./Logo.svelte";

  export let size: "sm" | "md" = "md";
  export let responsive = true;

  $: logoSize = size === "sm" ? 22 : 28;
</script>

<div class={`brand size-${size}`} class:responsive>
  <Logo size={logoSize} />
  <span class="wordmark">AIPass</span>
</div>

<style lang="scss">
  .brand {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--text);
    font-weight: 600;
    letter-spacing: -0.005em;
  }

  .size-sm {
    font-size: 13px;
  }

  .size-md {
    font-size: 15px;
  }

  .wordmark {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 920px) {
    .responsive .wordmark {
      display: none;
    }
  }
</style>
