<script lang="ts" module>
  /** One change of a node's stage in an update, at `t` (ms since the update began). */
  export interface StageEvent {
    t: number;
    id: string;
    stage: string;
  }
</script>

<script lang="ts">
  import { TIMELINE } from '../app/copy.ts';

  interface Props {
    /** Nodes in update order: id and the name shown. */
    rows: { id: string; name: string }[];
    events: StageEvent[];
    /** ms since the update began (the cursor). */
    now: number;
    /** Stage → its short word (the card chip's words). */
    label: (stage: string) => string;
  }

  let { rows, events, now, label }: Props = $props();
  const SEC = 1000;
  const total = $derived(Math.max(4 * SEC, now * 1.08));
  const pc = (t: number) => `${((t / total) * 100).toFixed(2)}%`;
  const ticks = $derived(Array.from({ length: Math.floor(total / SEC) + 1 }, (_, i) => i).filter((i) => i * SEC < total * 0.96));
  /** Blocks per node: each stage from its start to the next one (or now). */
  function blocks(id: string) {
    const own = events.filter((e) => e.id === id);
    return own.map((e, i) => ({ stage: e.stage, t0: e.t, t1: own[i + 1]?.t ?? now }));
  }
  const STAGES = ['writing', 'rebooting', 'done', 'waiting'];
</script>

<!-- DES-UX3-131: where the update spends its time, node by node, in time order. -->
{#if events.length === 0}
  <p class="none">{TIMELINE.none}</p>
{:else}
  <div class="tl" role="img" aria-label={TIMELINE.label}>
    <div class="row axis">
      <span class="name">{TIMELINE.time}</span>
      <div class="lane">
        {#each ticks as i (i)}<span class="tick" style:left={pc(i * SEC)}>{i} s</span>{/each}
        <span class="cursor" style:left={pc(now)}></span>
      </div>
    </div>
    {#each rows as r (r.id)}
      <div class="row">
        <span class="name">{r.name}</span>
        <div class="lane">
          {#each blocks(r.id) as b, i (i)}
            <span class="blk st-{b.stage}" style:left={pc(b.t0)} style:width="calc({pc(Math.max(80, b.t1 - b.t0))} - 2px)" title={label(b.stage)}>{label(b.stage)}</span>
          {/each}
        </div>
      </div>
    {/each}
    <div class="legend">{#each STAGES as s (s)}<span><i class="blk st-{s}"></i>{label(s)}</span>{/each}</div>
  </div>
{/if}

<style>
  .none {
    margin: 0;
    padding: var(--s-3) var(--s-5);
    color: var(--c-muted);
    font: 400 var(--t-12) / 1.5 var(--f-ja);
  }
  .tl {
    padding: 2px var(--s-3) 0.25rem;
    font: 400 var(--t-12) / 1 var(--f-ja);
  }
  .row {
    display: grid;
    grid-template-columns: 7rem 1fr;
    align-items: center;
    height: 1.25rem;
  }
  .name {
    overflow: hidden;
    color: var(--c-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lane {
    position: relative;
    height: 1rem;
    border-bottom: 1px solid var(--c-hair);
  }
  .axis .lane {
    border-bottom: 0;
  }
  .tick {
    position: absolute;
    top: 2px;
    height: 0.875rem;
    padding-left: 0.1875rem;
    border-left: 1px solid var(--c-hair);
    color: var(--c-muted);
    font: 500 var(--t-11) / 1 var(--f-mono);
  }
  .cursor {
    position: absolute;
    top: 0;
    bottom: -6rem;
    width: 1px;
    background: var(--c-accent);
  }
  .blk {
    position: absolute;
    top: 1px;
    height: 0.875rem;
    overflow: hidden;
    padding: 0 0.25rem;
    border-radius: 2px;
    color: var(--c-paper);
    font: 500 var(--t-11) / 0.875rem var(--f-ja);
    white-space: nowrap;
  }
  .st-writing {
    background: var(--c-layer-3);
  }
  .st-rebooting {
    background: repeating-linear-gradient(135deg, var(--c-layer-3) 0 0.25rem, transparent 0.25rem 0.5rem);
  }
  .st-done {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .st-waiting {
    border: 1px solid var(--c-line);
    line-height: 0.75rem;
  }
  .st-silent,
  .st-refused,
  .st-cannotFix {
    background: var(--c-signal);
    color: var(--c-ink);
  }
  .legend {
    display: flex;
    gap: 0.75rem;
    padding: 2px 0 0 7rem;
    color: var(--c-muted);
    font-size: var(--t-11);
  }
  .legend .blk {
    position: static;
    display: inline-block;
    width: 0.875rem;
    height: 0.625rem;
    margin-right: 0.25rem;
    padding: 0;
    vertical-align: -1px;
  }
</style>
