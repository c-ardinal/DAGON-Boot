<script lang="ts">
  // The bottom drawer's open tab: 問題, ジョブ, 出力, タイムライン, DAGONとの会話 and 記録.
  import { AGENT, DRAWER, JOBS } from '../app/copy.ts';
  import type { Agent } from '../app/agent.svelte.ts';
  import type { Note } from '../app/console/feedback.svelte.ts';
  import type { Cli } from '../app/define/cli.svelte.ts';
  import type { Define } from '../app/define/define.svelte.ts';
  import CliPanel from './define/CliPanel.svelte';
  import Problems from './define/Problems.svelte';
  import Timeline, { type StageEvent } from './Timeline.svelte';

  /** The latest output lines shown. */
  const OUTPUT_KEPT = 300;

  let {
    tab,
    d,
    cli,
    agent,
    notes,
    timeline,
    onapply,
  }: {
    tab: string;
    d: Define;
    cli: Cli;
    agent: Agent;
    notes: Note[];
    timeline: { rows: { id: string; name: string }[]; events: StageEvent[]; now: number; label: (s: string) => string };
    /** 『反映する』 on the proposal of message `i`. */
    onapply: (i: number, at: Element) => void;
  } = $props();

  /** 出力: the CLI jobs' output lines, oldest job first. */
  const outputLines = $derived(
    [...cli.jobs]
      .reverse()
      .flatMap((j) => j.lines.map((line) => ({ kind: j.kind, line })))
      .slice(-OUTPUT_KEPT),
  );
</script>

{#if tab === 'problems'}
  <Problems {d} />
{:else if tab === 'talk'}
  {#if agent.messages.length === 0}
    <p class="log-none">{AGENT.talkNone}</p>
  {:else}
    <ol class="talk">
      {#each agent.messages as m, i (i)}
        <li class={m.role}>
          <span class="who">{m.role === 'dagon' ? AGENT.name : AGENT.you}</span>
          <p class:failed={m.failed}>{m.text}</p>
          {#if m.more}<p class="more">{m.more}</p>{/if}
          {#if m.proposal}
            <figure>
              <figcaption>{AGENT.diffCaption}</figcaption>
              <pre>{#each m.proposal.yaml_diff.split('\n') as line, k (k)}<span class:add={line.startsWith('+')} class:del={line.startsWith('-')}>{line}</span>{'\n'}{/each}</pre>
              {#if !m.applied}<button class="primary" data-control="apply" onclick={(e) => onapply(i, e.currentTarget)}>{AGENT.apply}</button>{/if}
            </figure>
          {/if}
        </li>
      {/each}
    </ol>
  {/if}
{:else if tab === 'output'}
  {#if outputLines.length === 0}
    <p class="log-none">{DRAWER.outputNone}</p>
  {:else}
    <ol class="log out">
      {#each outputLines as o, i (i)}<li><span class="k">{JOBS.kinds[o.kind] ?? o.kind}</span> {o.line}</li>{/each}
    </ol>
  {/if}
{:else if tab === 'timeline'}
  <Timeline rows={timeline.rows} events={timeline.events} now={timeline.now} label={timeline.label} />
{:else if tab === 'jobs'}
  <CliPanel {cli} panel="jobs" />
{:else if tab === 'log'}
  {#if notes.length === 0}
    <p class="log-none">{DRAWER.logNone}</p>
  {:else}
    <ol class="log">
      {#each notes as n, i (i)}<li><time>{n.at}</time> {n.text}</li>{/each}
    </ol>
  {/if}
{/if}

<style>
  .log-none,
  .log {
    margin: 0;
    padding: var(--s-3) var(--s-5);
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .log {
    list-style: none;
  }
  .log.out {
    font-family: var(--f-mono);
    white-space: pre-wrap;
  }
  .log .k {
    color: var(--c-muted);
  }
  .log time {
    color: var(--c-muted);
    font-family: var(--f-mono);
  }
  .talk {
    display: grid;
    gap: var(--s-3);
    margin: 0;
    padding: var(--s-3) var(--s-5);
    list-style: none;
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .talk p {
    margin: 0;
  }
  .talk .who {
    color: var(--c-muted);
    font-size: var(--t-caption);
  }
  .talk .user p {
    color: var(--c-muted);
  }
  .talk .failed {
    color: var(--c-signal);
  }
  .talk .more {
    white-space: pre-wrap;
  }
  .talk figure {
    margin: var(--s-2) 0 0;
  }
  .talk pre {
    margin: var(--s-1) 0 var(--s-2);
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    font: 400 var(--t-mono) / 20px var(--f-mono);
    overflow-x: auto;
  }
  /* A proposal's diff: added lines in blue, removed ones muted and struck (not red: nothing failed). */
  .talk .add {
    color: var(--c-accent);
  }
  .talk .del {
    color: var(--c-muted);
    text-decoration: line-through;
  }
</style>
