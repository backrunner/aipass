<script lang="ts">
  import { onMount, createEventDispatcher } from "svelte";

  export let direction: "up" | "down" = "up";

  const dispatch = createEventDispatcher<{ covered: void; done: void }>();

  onMount(() => {
    const reduced =
      typeof window !== "undefined" &&
      window.matchMedia &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    if (reduced) {
      requestAnimationFrame(() => {
        dispatch("covered");
        dispatch("done");
      });
      return;
    }

    const coverTimer = setTimeout(() => dispatch("covered"), 600);
    const doneTimer = setTimeout(() => dispatch("done"), 1200);

    return () => {
      clearTimeout(coverTimer);
      clearTimeout(doneTimer);
    };
  });
</script>

<div class="veil" class:veil-down={direction === "down"} aria-hidden="true">
  <div class="block">
    <svg class="wave" viewBox="0 0 1440 120" preserveAspectRatio="none">
      <path
        class="wave-back"
        d="M0,70 C220,18 460,98 720,46 C980,-6 1220,86 1440,32 L1440,120 L0,120 Z"
      />
      <path
        class="wave-front"
        d="M0,86 C220,42 460,108 720,62 C980,18 1220,98 1440,52 L1440,120 L0,120 Z"
      />
    </svg>
    <div class="fill"></div>
  </div>
</div>

<style lang="scss">
  .veil {
    position: fixed;
    inset: 0;
    z-index: 90;
    overflow: hidden;
    pointer-events: none;
  }

  .block {
    position: absolute;
    left: -2%;
    right: -2%;
    top: 0;
    width: 104%;
    height: 100vh;
    transform: translateY(110%);
    animation: rise 1200ms cubic-bezier(0.5, 0, 0.5, 1) forwards;
    will-change: transform;
  }

  .veil-down .block {
    animation-name: descend;
  }

  .wave {
    position: absolute;
    bottom: 100%;
    left: 0;
    right: 0;
    width: 100%;
    height: 12vh;
    display: block;
    margin-bottom: -1px;
  }

  .veil-down .wave {
    bottom: auto;
    top: 100%;
    margin-bottom: 0;
    margin-top: -1px;
    transform: scaleY(-1);
  }

  .wave-back {
    fill: var(--accent);
    opacity: 0.55;
  }

  .wave-front {
    fill: var(--accent);
  }

  .fill {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      180deg,
      var(--accent) 0%,
      color-mix(in oklab, var(--accent) 78%, var(--bg)) 100%
    );
  }

  .veil-down .fill {
    background: linear-gradient(
      0deg,
      var(--accent) 0%,
      color-mix(in oklab, var(--accent) 78%, var(--bg)) 100%
    );
  }

  @keyframes rise {
    0% {
      transform: translateY(110%);
    }
    48% {
      transform: translateY(0);
    }
    52% {
      transform: translateY(0);
    }
    100% {
      transform: translateY(-100%);
    }
  }

  @keyframes descend {
    0% {
      transform: translateY(-110%);
    }
    48% {
      transform: translateY(0);
    }
    52% {
      transform: translateY(0);
    }
    100% {
      transform: translateY(100%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .block {
      animation: none;
      opacity: 0;
    }
  }
</style>
