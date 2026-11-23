<script lang="ts">
  // The band's buttons where a notice repeats them: the secondary ones, then the primary one.
  // A button whose procedure this page does not run shows as unavailable.
  import type { Button } from '../app/band.ts';
  import { CONTROL_OF, RUNS } from '../app/console/states.ts';
  import type { Action } from '../app/machine.ts';

  let { secondary, primary, onpress }: { secondary: Button[]; primary: Button | null; onpress: (a: Action) => void } = $props();
</script>

{#each secondary as b (b.label)}
  <button class="button-secondary" data-control={CONTROL_OF[b.action]} aria-disabled={!RUNS.has(b.action)} onclick={() => RUNS.has(b.action) && onpress(b.action)}>{b.label}</button>
{/each}
{#if primary}
  <button class="button-primary" data-control={CONTROL_OF[primary.action]} aria-disabled={!RUNS.has(primary.action)} onclick={() => RUNS.has(primary.action) && onpress(primary.action)}>{primary.label}</button>
{/if}
