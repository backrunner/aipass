<script lang="ts">
  export let active = true;
  export let fillProgress = 0;
</script>

<div class="hero" class:active style:--fill={fillProgress}>
  <div class="layer layer-1"></div>
  <div class="layer layer-2"></div>
  <div class="layer layer-3"></div>
  <svg class="waves" viewBox="0 0 1440 320" preserveAspectRatio="none" aria-hidden="true">
    <defs>
      <linearGradient id="hero-wave-1" x1="0" y1="0" x2="1" y2="0.6">
        <stop offset="0%" stop-color="var(--accent)" stop-opacity="0.32" />
        <stop offset="50%" stop-color="var(--accent)" stop-opacity="0.18" />
        <stop offset="100%" stop-color="var(--accent)" stop-opacity="0.08" />
      </linearGradient>
      <linearGradient id="hero-wave-2" x1="0" y1="0" x2="1" y2="0.7">
        <stop offset="0%" stop-color="var(--accent)" stop-opacity="0.18" />
        <stop offset="100%" stop-color="var(--accent)" stop-opacity="0.32" />
      </linearGradient>
    </defs>
    <path
      class="wave wave-back"
      fill="url(#hero-wave-1)"
      d="M0,160 C240,90 360,210 720,170 C1080,130 1200,240 1440,180 L1440,320 L0,320 Z"
    />
    <path
      class="wave wave-front"
      fill="url(#hero-wave-2)"
      d="M0,210 C240,150 480,260 720,220 C960,180 1200,260 1440,220 L1440,320 L0,320 Z"
    />
  </svg>
</div>

<style lang="scss">
  .hero {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: linear-gradient(135deg, var(--bg) 0%, var(--surface-2) 100%);
    isolation: isolate;
    pointer-events: none;
  }

  .layer {
    position: absolute;
    border-radius: 50%;
    filter: blur(80px);
    opacity: 0.55;
    will-change: transform;
  }

  .layer-1 {
    width: 56vmax;
    height: 56vmax;
    left: -10vmax;
    top: -10vmax;
    background: radial-gradient(closest-side, var(--accent), transparent 70%);
    opacity: 0.35;
    animation: drift-a 18s ease-in-out infinite alternate;
  }

  .layer-2 {
    width: 48vmax;
    height: 48vmax;
    right: -12vmax;
    top: 18vmax;
    background: radial-gradient(closest-side, var(--accent-hover, var(--accent)), transparent 70%);
    opacity: 0.28;
    animation: drift-b 22s ease-in-out infinite alternate;
  }

  .layer-3 {
    width: 38vmax;
    height: 38vmax;
    left: 22vmax;
    bottom: -10vmax;
    background: radial-gradient(closest-side, var(--accent), transparent 70%);
    opacity: 0.22;
    animation: drift-c 26s ease-in-out infinite alternate;
  }

  .waves {
    position: absolute;
    left: -20%;
    right: -20%;
    bottom: -10%;
    width: 140%;
    height: 60%;
    transform: translateY(calc(var(--fill, 0) * -100%));
    transition: transform 700ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  .wave {
    transform-origin: 50% 100%;
    animation: wave-drift 5s linear infinite;
  }

  .wave-back {
    animation-duration: 7s;
    animation-direction: alternate;
  }

  .wave-front {
    animation-duration: 4.5s;
    animation-direction: alternate-reverse;
  }

  @keyframes drift-a {
    from {
      transform: translate3d(0, 0, 0) scale(1);
    }
    to {
      transform: translate3d(8vmax, 4vmax, 0) scale(1.08);
    }
  }

  @keyframes drift-b {
    from {
      transform: translate3d(0, 0, 0) scale(1);
    }
    to {
      transform: translate3d(-6vmax, -3vmax, 0) scale(1.05);
    }
  }

  @keyframes drift-c {
    from {
      transform: translate3d(0, 0, 0) scale(1);
    }
    to {
      transform: translate3d(4vmax, -6vmax, 0) scale(1.1);
    }
  }

  @keyframes wave-drift {
    from {
      transform: translateX(0);
    }
    to {
      transform: translateX(-12%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .layer,
    .wave {
      animation: none !important;
    }

    .waves {
      transition: none;
    }
  }
</style>
