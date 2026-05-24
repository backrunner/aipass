<script lang="ts">
  export let size: "sm" | "md" = "md";
</script>

<div class={`brand size-${size}`}>
  <span class="mark" aria-hidden="true">
    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
      <path d="M12 3l8 4v5c0 4.4-3.4 8.3-8 9-4.6-.7-8-4.6-8-9V7l8-4z" />
      <path d="M9 12l2 2 4-4" />
    </svg>
  </span>
  <span class="wordmark">AIPass</span>
</div>

<style lang="scss">
  .brand {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
    font-weight: 600;
    letter-spacing: -0.005em;
  }

  .mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--accent);
    color: #fff;
    border-radius: var(--radius);
  }

  .size-sm {
    font-size: 13px;

    .mark {
      width: 22px;
      height: 22px;
    }
  }

  .size-md {
    font-size: 15px;

    .mark {
      width: 26px;
      height: 26px;
    }
  }

  .wordmark {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  @media (max-width: 920px) {
    .wordmark {
      display: none;
    }
  }
</style>
