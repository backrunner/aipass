<script lang="ts">
  import type { ToolConfigTarget } from "../../types";

  export let tool: ToolConfigTarget;
  export let size = 17;

  const icons: Record<ToolConfigTarget, string> = {
    codex: "/tool-icons/codex.svg",
    "claude-code": "/tool-icons/claude-code.svg",
    "gemini-cli": "/tool-icons/gemini-cli.svg",
    opencode: "/tool-icons/opencode.svg",
    grok: "/tool-icons/grok.svg",
    pi: "/tool-icons/pi.svg",
    cursor: "/tool-icons/cursor.svg"
  };

  $: icon = icons[tool];
</script>

<span
  class="brand-icon"
  style={`--brand-icon: url("${icon}"); --brand-icon-size: ${size}px`}
  aria-hidden="true"
></span>

<style>
  .brand-icon {
    display: block;
    width: var(--brand-icon-size);
    height: var(--brand-icon-size);
    background: currentColor;
    -webkit-mask: var(--brand-icon) center / contain no-repeat;
    mask: var(--brand-icon) center / contain no-repeat;
  }
</style>
