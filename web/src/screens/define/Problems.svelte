<script lang="ts">
  import { DEFINE } from '../../app/copy.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import { problemChip } from '../../app/define/problems.ts';

  interface Props {
    d: Define;
  }

  let { d }: Props = $props();
  const count = (kind: string) => d.problems.filter((p) => problemChip(p).kind === kind).length;
  const unsupported = $derived(count('unsupported'));
  const errors = $derived(count('error'));
  const cautions = $derived(count('warning'));
</script>

<section class="problems" aria-labelledby="problems-h">
  {#if d.problems.length === 0}
    <p class="lead">{DEFINE.nothingSelected}</p>
    {#if d.doc.topo.nodes.length}<h2 id="problems-h">{DEFINE.problemsNone}</h2>{/if}
  {:else}
    <h2 id="problems-h" tabindex="-1">{DEFINE.problems(errors)}{#if cautions}&ensp;{DEFINE.cautions(cautions)}{/if}{#if unsupported}&ensp;{DEFINE.unsupported(unsupported)}{/if}</h2>
    <ul>
      {#each d.problems as p, i (i)}
        {@const c = problemChip(p)}
        <li class:warn={c.kind !== 'error'} aria-label={DEFINE.problemName(p.short, p.rule, p.sentence)}>
          <p class="head"><span>{c.mark} {p.short}</span><span class="rule" translate="no">{p.rule}</span></p>
          <p class="fix-s">{p.sentence}</p>
          <!-- A part not updatable yet (info) is valid: nothing to fix. -->
          {#if p.select && p.severity !== 'info'}<button class="secondary" onclick={() => d.fix(p)}>{DEFINE.fix}</button>{/if}
        </li>
      {/each}
    </ul>
    <details>
      <summary>{DEFINE.details}</summary>
      <table>
        <thead><tr><th scope="col">{DEFINE.ruleId}</th><th scope="col">{DEFINE.yamlPath}</th></tr></thead>
        <tbody translate="no">
          {#each d.problems as p, i (i)}<tr><td>{p.rule}</td><td>{p.path}</td></tr>{/each}
        </tbody>
      </table>
    </details>
  {/if}
</section>

<style>
  .problems {
    padding: var(--s-5);
  }
  h2 {
    margin: 0 0 var(--s-4);
    font: 700 var(--t-title-size) / var(--t-title-lh) var(--f-ja);
  }
  h2:focus {
    outline: none;
  }
  .lead {
    margin: 0 0 var(--s-4);
    color: var(--c-muted);
  }
  ul {
    display: grid;
    gap: var(--s-4);
    margin: 0 0 var(--s-4);
    padding: 0;
    list-style: none;
  }
  li {
    padding: var(--s-4);
    border: 2px solid var(--c-signal);
    border-radius: var(--r-m);
  }
  li.warn {
    border-style: dashed;
    border-color: var(--c-line);
  }
  .head {
    display: flex;
    justify-content: space-between;
    margin: 0 0 var(--s-2);
    font: 700 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .rule {
    font: 400 var(--t-mono-size) / var(--t-body-lh) var(--f-mono);
  }
  .fix-s {
    margin: 0 0 var(--s-3);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  summary {
    cursor: pointer;
    font: 700 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  table {
    margin-top: var(--s-3);
    border-collapse: collapse;
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
  }
  th {
    color: var(--c-muted);
    text-align: left;
    font-family: var(--f-ja);
  }
  th,
  td {
    padding: var(--s-1) var(--s-4) var(--s-1) 0;
  }
</style>
