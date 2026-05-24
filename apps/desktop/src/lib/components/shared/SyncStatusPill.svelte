<script lang="ts">
  import type { SyncReport } from "../../types";

  export let state: SyncReport["status"] = "idle";
  export let onClick: () => void = () => {};

  $: tone = state === "conflict" || state === "auth_failed" || state === "server_error"
    ? "danger"
    : state === "offline"
      ? "neutral"
      : state === "syncing"
        ? "info"
        : "success";

  $: label =
    state === "syncing"
      ? "Syncing…"
      : state === "conflict"
        ? "Conflicts"
        : state === "offline"
          ? "Offline"
          : state === "auth_failed"
            ? "Auth failed"
            : state === "server_error"
              ? "Server error"
              : "Synced";
</script>

<button type="button" class={`pill tone-${tone}`} on:click={onClick} aria-label="Sync status">
  <span class="dot"></span>
  <span class="label">{label}</span>
</button>

<style lang="scss">
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: background-color 80ms ease, color 120ms ease;

    &:hover {
      color: var(--text);
      background: var(--surface-2);
    }
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 999px;
    background: var(--text-tertiary);
  }

  .tone-success .dot {
    background: var(--success);
  }

  .tone-info .dot {
    background: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }

  .tone-danger {
    color: var(--danger);

    .dot {
      background: var(--danger);
    }
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.45; }
  }
</style>
