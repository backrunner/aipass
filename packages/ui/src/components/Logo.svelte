<script lang="ts">
  export let size = 24;
  export let alt = "AIPass";
  export let rounded = true;
</script>

<img
  src="/aipass-logo.png"
  width={size}
  height={size}
  {alt}
  class:rounded
  draggable="false"
/>

<style lang="scss">
  img {
    display: block;
    user-select: none;
    -webkit-user-drag: none;
    flex-shrink: 0;
  }

  img.rounded {
    border-radius: 22%;
  }
</style>
