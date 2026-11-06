<script lang="ts">
  import { CLI_LINK, CLI_NAV, JOBS } from '../../app/copy.ts';
  import type { Cli, Job } from '../../app/define/cli.svelte.ts';
  import { KeyDraft, type Agent } from '../../app/agent.svelte.ts';

  interface Props {
    cli: Cli;
    panel: 'link' | 'jobs';
    /** The agent link: the API key of each way that takes one is registered here (CLI 連携). */
    agent?: Agent;
  }

  let { cli, panel, agent }: Props = $props();

  /** Every way that takes an API key, as GET /v1/agent lists them. */
  const keyWays = $derived((agent?.status?.agents ?? []).flatMap((a) => a.ways.filter((w) => w.key != null).map((w) => ({ a, w }))));
  /** One field per way, kept across status refreshes (it holds no key once sent). */
  const drafts = new Map<string, KeyDraft>();
  function draftOf(a: string, w: string): KeyDraft {
    const id = `${a}/${w}`;
    let draft = drafts.get(id);
    if (!draft && agent) drafts.set(id, (draft = new KeyDraft(agent, { agent: a, way: w })));
    return draft!;
  }

  const ready = $derived(cli.state === 'ready');
  let selectedJob = $state<number | null>(null);
  const job = $derived(cli.jobs.find((j) => j.id === selectedJob) ?? cli.jobs[0]);
  const elapsed = (j: Job) => {
    const s = Math.round(((j.ended ?? Date.now()) - j.started) / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  };
</script>

<section class="panel" aria-labelledby="panel-h">
  {#if panel === 'link' || !ready}
    <h2 id="panel-h" tabindex="-1">{CLI_NAV.items.link[1]}</h2>
    {#if panel !== 'link' && !ready}<p>{CLI_LINK.needCli}</p>{/if}
    {#if cli.state === 'ready' && cli.hello}
      <dl class="pairs">
        <dt>{CLI_LINK.workspace}</dt><dd translate="no">{cli.hello.workspace ?? ''}</dd>
        <dt>{CLI_LINK.key}</dt><dd>{cli.hello.key ? `${CLI_LINK.yes}（${(cli.hello.key_id ?? '').slice(0, 8)}…）` : CLI_LINK.no}</dd>
      </dl>
      {#if keyWays.length > 0}
        <h3>{CLI_LINK.keys.heading}</h3>
        <p class="note">{CLI_LINK.keys.detail}</p>
        <ul class="rows">
          {#each keyWays as { a, w } (`${a.id}/${w.id}`)}
            {@const draft = draftOf(a.id, w.id)}
            <li>
              <span class="who"><strong>{a.name}</strong><span>{w.name}</span></span>
              <span class="chip">{w.key ? CLI_LINK.keys.set : CLI_LINK.keys.unset}</span>
              <form
                onsubmit={(e) => {
                  e.preventDefault();
                  void draft.save();
                }}
              >
                <label>{CLI_LINK.keys.input}<input type="password" autocomplete="off" spellcheck="false" translate="no" bind:value={draft.value} /></label>
                <div class="actions">
                  <button type="submit" class="secondary" aria-disabled={!draft.value.trim()} title={draft.value.trim() ? undefined : CLI_LINK.keys.saveWhy}>{CLI_LINK.keys.save}</button>
                  {#if w.key}<button type="button" class="secondary destructive" title={CLI_LINK.keys.removeWhy} onclick={() => void draft.remove()}>{CLI_LINK.keys.remove}</button>{/if}
                </div>
              </form>
              {#if draft.problem}<p class="bad">{draft.problem.label ?? ''} {draft.problem.text}</p>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      <h3>{CLI_LINK.features}</h3>
      <dl class="pairs">
        {#each Object.entries(CLI_LINK.featureNames) as [f, label] (f)}
          <dt>{label}</dt><dd>{cli.has(f) ? '✓' : CLI_LINK.unsupported}</dd>
        {/each}
      </dl>
    {:else if cli.state === 'denied'}
      <p class="cause">{CLI_LINK.deniedChip}</p>
      <p>{CLI_LINK.deniedSentence}</p>
    {:else if cli.state === 'stopped'}
      <p>{CLI_LINK.stoppedSentence}</p>
      <p class="pair"><span class="label">{CLI_LINK.withoutCli[0]}</span> {CLI_LINK.withoutCli[1]}</p>
      <button class="secondary" onclick={() => void cli.connect()}>{CLI_LINK.reconnect}</button>
    {:else}
      <ol class="steps">
        {#each CLI_LINK.steps as s, i (s)}<li><span class="n">{String(i + 1).padStart(2, '0')}</span>{s}</li>{/each}
      </ol>
      <p class="pair"><span class="label">{CLI_LINK.withoutCli[0]}</span> {CLI_LINK.withoutCli[1]}</p>
    {/if}
  {:else}
    <h2 id="panel-h" tabindex="-1">{JOBS.heading}</h2>
    {#if cli.jobs.length === 0}
      <p>{JOBS.none}</p>
    {:else}
      <table>
        <thead><tr>{#each JOBS.cols as c (c)}<th scope="col">{c}</th>{/each}</tr></thead>
        <tbody>
          {#each cli.jobs as j (j.id)}
            <tr aria-selected={job?.id === j.id} onclick={() => (selectedJob = j.id)}>
              <td><button class="link" onclick={() => (selectedJob = j.id)}>{JOBS.kinds[j.kind] ?? j.kind}</button></td>
              <td translate="no">{j.target}</td>
              <td>{JOBS.states[j.state] ?? j.state}</td>
              <td class="num">{elapsed(j)}</td>
              <td class="num">{j.percent ?? ''}{j.percent !== null ? '%' : ''}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if job}
        <dl class="pairs"><dt>{JOBS.started}</dt><dd>{new Date(job.started).toLocaleTimeString('ja-JP')}</dd></dl>
        <div class="actions">
          {#if job.state === 'running' || job.state === 'queued'}<button class="secondary" onclick={() => void cli.cancel(job)}>{JOBS.cancel}</button>{/if}
          <button class="secondary" onclick={() => void navigator.clipboard?.writeText(job.lines.join('\n'))}>{JOBS.copyLog}</button>
        </div>
        <pre translate="no">{job.lines.join('\n')}</pre>
        {#if job.state === 'failed' && job.kind === 'build'}<p>{JOBS.failSentence}</p>{/if}
      {/if}
    {/if}
  {/if}
</section>

<style>
  .panel {
    padding: var(--s-5);
  }
  h2 {
    margin: 0 0 var(--s-4);
    padding-right: var(--s-6);
    font: 700 var(--t-heading) / 28px var(--f-ja);
  }
  h2:focus {
    outline: none;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    margin: var(--s-4) 0 var(--s-3);
    font: 700 var(--t-small) / 20px var(--f-ja);
  }
  p {
    margin: 0 0 var(--s-3);
  }
  .n {
    font: 400 var(--t-small) / 1 var(--f-mono);
    color: var(--c-muted);
  }
  .pairs {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2) var(--s-4);
    margin: 0 0 var(--s-4);
  }
  .pairs dt,
  .label {
    color: var(--c-muted);
  }
  /* A value stays on one line: it scrolls sideways rather than breaking mid-word. */
  .pairs dd {
    margin: 0;
    min-width: 0;
    overflow-x: auto;
    white-space: nowrap;
  }
  .steps,
  .rows {
    display: grid;
    gap: var(--s-3);
    margin: 0 0 var(--s-4);
    padding: 0;
    list-style: none;
  }
  .steps li {
    display: flex;
    gap: var(--s-3);
  }
  .rows li {
    display: grid;
    gap: var(--s-2);
    padding: var(--s-4);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
  }
  .who {
    display: grid;
  }
  .chip {
    justify-self: start;
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  .bad,
  .cause {
    color: var(--c-signal);
    font-weight: 700;
  }
  .num {
    font: 400 var(--t-mono) / 20px var(--f-mono);
  }
  label {
    display: grid;
    gap: var(--s-1);
    min-width: 0;
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  input {
    min-height: var(--h-secondary);
    padding: 0 var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    background: var(--c-ink);
    color: var(--c-paper);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-3);
  }
  .note {
    color: var(--c-muted);
  }
  form {
    display: grid;
    gap: var(--s-2);
  }
  /* Destructive: the secondary shape in red (red only for errors and destroying actions). */
  .destructive {
    border-color: var(--c-signal);
    color: var(--c-signal);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: var(--s-4);
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  th {
    color: var(--c-muted);
    text-align: left;
  }
  th,
  td {
    padding: var(--s-1) var(--s-2);
    border-bottom: 1px solid var(--c-line);
  }
  tr[aria-selected='true'] {
    background: var(--c-layer-2);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--c-paper);
    text-decoration: underline;
    cursor: pointer;
  }
  pre {
    max-height: 20rem;
    overflow: auto;
    padding: var(--s-3);
    border: 1px solid var(--c-line);
    font: 400 var(--t-mono) / 20px var(--f-mono);
    white-space: pre-wrap;
  }
</style>
