<script lang="ts">
  // Ambient background for the landing page, adapted from the desktop
  // unlock screen (apps/desktop HeroBackground): drifting accent orbs
  // plus a soft wave anchored to the bottom of the hero. Purely
  // decorative, single-hue (--ap-primary), all motion is slow and
  // disabled under prefers-reduced-motion.
  //
  // variant "calm" is used on docs pages: same drifting orbs at lower
  // intensity, no waves, so body text stays perfectly legible.
  export let variant: 'full' | 'calm' = 'full';
</script>

<div class="page-bg" class:calm={variant === 'calm'} aria-hidden="true">
  <div class="orb orb-1"></div>
  <div class="orb orb-2"></div>
  <div class="orb orb-3"></div>
  <svg class="waves" viewBox="0 0 1440 320" preserveAspectRatio="none">
    <defs>
      <linearGradient id="ap-wave-1" x1="0" y1="0" x2="1" y2="0.6">
        <stop offset="0%" stop-color="var(--ap-primary)" stop-opacity="0.28" />
        <stop offset="50%" stop-color="var(--ap-primary)" stop-opacity="0.14" />
        <stop offset="100%" stop-color="var(--ap-primary)" stop-opacity="0.05" />
      </linearGradient>
      <linearGradient id="ap-wave-2" x1="0" y1="0" x2="1" y2="0.7">
        <stop offset="0%" stop-color="var(--ap-primary)" stop-opacity="0.12" />
        <stop offset="100%" stop-color="var(--ap-primary)" stop-opacity="0.26" />
      </linearGradient>
    </defs>
    <path
      class="wave wave-back"
      fill="url(#ap-wave-1)"
      d="M0,160 C240,90 360,210 720,170 C1080,130 1200,240 1440,180 L1440,320 L0,320 Z"
    />
    <path
      class="wave wave-front"
      fill="url(#ap-wave-2)"
      d="M0,210 C240,150 480,260 720,220 C960,180 1200,260 1440,220 L1440,320 L0,320 Z"
    />
  </svg>
</div>

<style>
  .page-bg {
    position: absolute;
    inset: 0 0 auto;
    height: 190svh;
    overflow: hidden;
    pointer-events: none;
    -webkit-mask-image: linear-gradient(to bottom, black 45%, transparent 92%);
    mask-image: linear-gradient(to bottom, black 45%, transparent 92%);
  }

  .orb {
    position: absolute;
    border-radius: 50%;
    filter: blur(90px);
    will-change: transform;
  }

  .orb-1 {
    width: 54vmax;
    height: 54vmax;
    left: -12vmax;
    top: -14vmax;
    background: radial-gradient(closest-side, var(--ap-primary), transparent 70%);
    opacity: .16;
    animation: orb-drift-a 26s ease-in-out infinite alternate;
  }

  .orb-2 {
    width: 44vmax;
    height: 44vmax;
    right: -14vmax;
    top: 6vmax;
    background: radial-gradient(closest-side, var(--ap-primary), transparent 70%);
    opacity: .12;
    animation: orb-drift-b 32s ease-in-out infinite alternate;
  }

  .orb-3 {
    width: 34vmax;
    height: 34vmax;
    left: 30vmax;
    top: 42vmax;
    background: radial-gradient(closest-side, var(--ap-primary), transparent 70%);
    opacity: .09;
    animation: orb-drift-c 38s ease-in-out infinite alternate;
  }

  .waves {
    position: absolute;
    left: -20%;
    top: 58svh;
    width: 140%;
    height: 42svh;
  }

  .wave {
    transform-origin: 50% 100%;
  }

  .wave-back {
    animation: wave-drift 11s linear infinite alternate;
  }

  .wave-front {
    animation: wave-drift 8.5s linear infinite alternate-reverse;
  }

  :global(:root[data-theme="dark"]) .orb-1 { opacity: .3; }
  :global(:root[data-theme="dark"]) .orb-2 { opacity: .22; }
  :global(:root[data-theme="dark"]) .orb-3 { opacity: .16; }

  .calm .waves { display: none; }
  .calm .orb-1 { opacity: .11; }
  .calm .orb-2 { opacity: .09; }
  .calm .orb-3 { opacity: .07; }
  :global(:root[data-theme="dark"]) .calm .orb-1 { opacity: .22; }
  :global(:root[data-theme="dark"]) .calm .orb-2 { opacity: .16; }
  :global(:root[data-theme="dark"]) .calm .orb-3 { opacity: .11; }

  @keyframes orb-drift-a {
    from { transform: translate3d(0, 0, 0) scale(1); }
    to { transform: translate3d(7vmax, 4vmax, 0) scale(1.07); }
  }

  @keyframes orb-drift-b {
    from { transform: translate3d(0, 0, 0) scale(1); }
    to { transform: translate3d(-6vmax, -3vmax, 0) scale(1.05); }
  }

  @keyframes orb-drift-c {
    from { transform: translate3d(0, 0, 0) scale(1); }
    to { transform: translate3d(4vmax, -5vmax, 0) scale(1.09); }
  }

  @keyframes wave-drift {
    from { transform: translateX(0); }
    to { transform: translateX(-10%); }
  }

  @media (max-width: 920px) {
    .page-bg { height: 150svh; }
    .waves { top: 66svh; height: 30svh; }
  }

  @media (prefers-reduced-motion: reduce) {
    .orb,
    .wave {
      animation: none !important;
    }
  }
</style>
