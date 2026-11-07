<script lang="ts" module>
  /** The big dot number: its label, value, optional "/N" and full spoken name. */
  export interface Dot {
    label: string;
    value: number;
    of?: number;
    /** Unit after the number ("%", "% 停止"). */
    unit?: string;
    /** A label-value pair under the number ("リンク 2"). */
    sub?: readonly [string, string | number];
    /** Pairs after `sub` on the same line ("残り 約 2 分"). */
    more?: readonly (readonly [string, string])[];
    /** The number is a failure count (問題 n > 0): red. */
    alarm?: boolean;
    name: string;
  }
</script>

<script lang="ts">
  import { fade } from '../app/motion.ts';

  interface Props {
    dot: Dot;
    /** Many ノード: the number gives the canvas more room. */
    small?: boolean;
  }

  let { dot, small = false }: Props = $props();
</script>

<!-- The focal number: the one figure of the moment, last in the left column's reading order. -->
<div transition:fade class="focal" class:small data-avoid="" role="img" aria-label={dot.name}>
  <span class="lbl">{dot.label}</span>
  <span class="row"><span class="num" class:err={dot.alarm && dot.value > 0}>{dot.value}</span>{#if dot.of !== undefined}<span class="of">/{dot.of}</span>{/if}{#if dot.unit}<span class="of">{dot.unit}</span>{/if}</span>
  {#if dot.sub}<span class="sub"
      ><span class="k">{dot.sub[0]}</span>{dot.sub[1]}{#each dot.more ?? [] as [label, value] (label)}&emsp;<span class="k">{label}</span>{value}{/each}</span
    >{/if}
</div>

<style>
  .focal {
    display: grid;
    flex: none;
    gap: var(--s-1);
    pointer-events: none;
  }
  .lbl {
    color: var(--c-muted);
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
  }
  .num {
    color: var(--c-paper);
    font: 700 var(--t-display) / 1 var(--f-dot);
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
  }
  .num.err {
    color: var(--c-signal);
  }
  .of {
    color: var(--c-muted);
    font: 700 var(--t-title) / 1 var(--f-ja);
  }
  .small .num {
    font-size: var(--t-title);
    line-height: 32px;
  }
  .small .of {
    font-size: var(--t-small);
  }
  .sub {
    color: var(--c-paper);
    font: 400 var(--t-small) / 20px var(--f-ja);
    white-space: nowrap;
  }
  .sub .k {
    margin-right: var(--s-1);
    color: var(--c-muted);
  }
</style>
