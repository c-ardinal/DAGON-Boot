<script lang="ts">
  import { ICONS, type IconName } from './icons.ts';

  interface Props {
    name: IconName;
    /** Edge length; a size token (`var(--i-m)`) by default. */
    size?: string;
  }

  let { name, size = 'var(--i-m)' }: Props = $props();
</script>

<svg class="icon" viewBox="0 0 24 24" style:width={size} style:height={size} aria-hidden="true" focusable="false">
  {#each ICONS[name] as d (d)}<path {d} />{/each}
</svg>

<style>
  .icon {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
