<script lang="ts">
  import { CLI_LINK, CLI_NAV, FLASH, JOBS, PROJECT, RELEASE_STEPS } from '../../app/copy.ts';
  import type { Cli, Job } from '../../app/define/cli.svelte.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import { boardMissing, toYaml } from '../../app/define/model.ts';

  interface Props {
    cli: Cli;
    d: Define;
    panel: 'link' | 'jobs' | 'release' | 'project' | 'flash';
    /** "このリリースを運用で使う": hand the .dgb to the operate mode. */
    onuse: (name: string, bytes: Uint8Array) => void;
  }

  let { cli, d, panel, onuse }: Props = $props();

  const ready = $derived(cli.state === 'ready');
  const nodes = $derived(d.config.nodes);
  const ids = $derived(nodes.map((n) => n.id));
  /** Making and building the project: every check passes but V-23 (a shape not updatable yet still builds). */
  const clean = $derived(!d.problems.some((p) => (p.severity === 'error' && p.rule !== 'V-23') || p.rule === 'B-01'));
  const latest = (kind: string) => cli.jobs.find((j) => j.kind === kind);
  const ok = (kind: string) => latest(kind)?.state === 'succeeded';
  let selectedJob = $state<number | null>(null);
  const job = $derived(cli.jobs.find((j) => j.id === selectedJob) ?? cli.jobs[0]);
  let version = $state('1.0.0');
  let releaseName = $state('release');
  const versionOk = $derived(/^\d+\.\d+\.\d+$/.test(version));
  const elapsed = (j: Job) => {
    const s = Math.round(((j.ended ?? Date.now()) - j.started) / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  };
  const release = $derived(latest('release')?.result as { release?: string; bytes?: number; topology_sha256?: string } | null);
  const generated = (id: string) => latest('gen')?.state === 'succeeded' && (latest('gen')!.target.split(' · ').includes(id) || latest('gen')!.target === '');
  /** The hub first (it confirms the nodes behind it). */
  const flashOrder = $derived([...nodes].sort((a, b) => (a.role === 'hub' ? -1 : b.role === 'hub' ? 1 : 0)));
  const flashJob = (id: string) => cli.jobs.find((j) => j.kind === 'flash' && j.target === id);
  const mcuOf = (id: string) => d.doc.topo.nodes.find((n) => n.id === id)?.mcu ?? '';

  async function makeProject() {
    await cli.saveTopology(toYaml(d.doc.topo));
    await cli.gen(ids);
  }

  async function use() {
    if (!release?.release) return;
    onuse(`${release.release}.dgb`, await cli.releaseBytes(release.release));
  }
</script>

{#snippet stateChip(j: Job | undefined)}
  {#if j}<span class="chip" class:bad={j.state === 'failed'}>{JOBS.states[j.state] ?? j.state}</span>{/if}
{/snippet}

<section class="panel" aria-labelledby="panel-h">
  {#if panel === 'link' || !ready}
    <h2 id="panel-h" tabindex="-1">{CLI_NAV.items.link[1]}</h2>
    {#if panel !== 'link' && !ready}<p>{CLI_LINK.needCli}</p>{/if}
    {#if cli.state === 'ready' && cli.hello}
      <dl class="pairs">
        <dt>{CLI_LINK.workspace}</dt><dd translate="no">{cli.hello.workspace ?? ''}</dd>
        <dt>{CLI_LINK.apiKey}</dt><dd>{cli.hello.api_key ? CLI_LINK.yes : CLI_LINK.no}</dd>
        <dt>{CLI_LINK.key}</dt><dd>{cli.hello.key ? `${CLI_LINK.yes}（${(cli.hello.key_id ?? '').slice(0, 8)}…）` : CLI_LINK.no}</dd>
      </dl>
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
  {:else if panel === 'project'}
    <h2 id="panel-h" tabindex="-1">{PROJECT.heading}</h2>
    <ul class="rows">
      {#each nodes as n (n.id)}
        <li>
          <span class="who"><strong>{n.name}</strong><span translate="no">{n.product} · {n.mcu}</span></span>
          <span class="chip">{boardMissing(d.doc, n.id) ? PROJECT.boardUnset : generated(n.id) ? PROJECT.made : PROJECT.notMade}</span>
          <span class="here"><span class="label">{PROJECT.writeHere}</span> <code translate="no">nodes/{n.id}/src/app.rs</code></span>
        </li>
      {/each}
    </ul>
    {#if !clean}<p>{PROJECT.fixFirst}</p>{/if}
    <button class="primary" aria-disabled={!clean || !cli.has('gen')} onclick={() => clean && cli.has('gen') && void makeProject()}>{PROJECT.make}</button>
    {@render stateChip(latest('gen'))}
  {:else if panel === 'flash'}
    <h2 id="panel-h" tabindex="-1">{FLASH.heading}</h2>
    <p>{FLASH.hubFirst}</p>
    <ul class="rows">
      {#each flashOrder as n (n.id)}
        {@const fam = FLASH.families[mcuOf(n.id)]}
        {@const fj = flashJob(n.id)}
        <li>
          <span class="who"><strong>{n.name}</strong><span translate="no">{n.product} · {n.mcu}</span></span>
          {#if fam}<span class="pair"><span class="label">{FLASH.via}</span> {fam[0]}</span><p>{fam[1]}</p>{/if}
          {#if fj?.question}
            <p>{fj.question.text}</p>
            <button class="secondary" onclick={() => void cli.answer(fj, JOBS.answer)}>{JOBS.answer}</button>
          {/if}
          <span class="chip">{fj?.state === 'succeeded' ? FLASH.done : fj?.state === 'running' || fj?.state === 'question' ? FLASH.running : fj?.state === 'failed' ? FLASH.notFound : ''}</span>
          <button class="primary" aria-disabled={!cli.has('flash')} onclick={() => cli.has('flash') && void cli.flash(n.id)}>{FLASH.write}</button>
        </li>
      {/each}
    </ul>
  {:else if panel === 'release'}
    <h2 id="panel-h" tabindex="-1">{RELEASE_STEPS.heading}</h2>
    <ol class="stages">
      <li>
        <h3><span class="n">01</span>{RELEASE_STEPS.steps[0]} {@render stateChip(latest('build'))}</h3>
        <button class="secondary" aria-disabled={!cli.has('build')} onclick={() => cli.has('build') && void cli.build(ids)}>{RELEASE_STEPS.build}</button>
      </li>
      <li>
        <h3><span class="n">02</span>{RELEASE_STEPS.steps[1]} {@render stateChip(latest('sign'))}</h3>
        {#if !cli.hello?.key}<p>{RELEASE_STEPS.noKey}</p>{:else}<p class="pair"><span class="label">{RELEASE_STEPS.keyLabel}</span> <span translate="no">{(cli.hello.key_id ?? '').slice(0, 8)}…</span></p>{/if}
        {#if !ok('build')}<p>{RELEASE_STEPS.buildFirst}</p>{/if}
        <button class="secondary" aria-disabled={!ok('build') || !cli.hello?.key} onclick={() => ok('build') && cli.hello?.key && void cli.sign(ids)}>{RELEASE_STEPS.sign}</button>
      </li>
      <li>
        <h3><span class="n">03</span>{RELEASE_STEPS.steps[2]} {@render stateChip(latest('release'))}</h3>
        {#if !ok('sign')}<p>{RELEASE_STEPS.signFirst}</p>{/if}
        {#if d.errors}<p>{RELEASE_STEPS.fixFirst}</p>{/if}
        <div class="grid">
          <label>{RELEASE_STEPS.version}<input bind:value={version} aria-invalid={!versionOk} aria-describedby="version-help" translate="no" /></label>
          <label>{RELEASE_STEPS.name}<input bind:value={releaseName} translate="no" /></label>
        </div>
        {#if !versionOk}<p id="version-help" class="bad">{RELEASE_STEPS.versionHelp}</p>{/if}
        <button class="secondary" aria-disabled={!ok('sign') || !!d.errors || !versionOk} onclick={() => ok('sign') && !d.errors && versionOk && void cli.release(version, releaseName)}>{RELEASE_STEPS.makeRelease}</button>
        {#if release?.release}
          <p class="pair" translate="no"><span class="label">{release.release}.dgb</span> {Math.ceil((release.bytes ?? 0) / 1024)} KB</p>
        {/if}
      </li>
      <li>
        <h3><span class="n">04</span>{RELEASE_STEPS.steps[3]}</h3>
        <div class="actions">
          <button class="primary" aria-disabled={!release?.release} onclick={() => void use()}>{RELEASE_STEPS.useInOperate}</button>
        </div>
      </li>
    </ol>
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
    font: 700 var(--t-title-size) / var(--t-title-lh) var(--f-ja);
  }
  h2:focus {
    outline: none;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    margin: var(--s-4) 0 var(--s-3);
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
  }
  p {
    margin: 0 0 var(--s-3);
  }
  .n {
    font: 400 var(--t-body-size) / 1 var(--f-version);
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
  .pairs dd {
    margin: 0;
  }
  .steps,
  .stages,
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
  .rows li,
  .stages li {
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
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .bad,
  .cause {
    color: var(--c-signal);
    font-weight: 700;
  }
  code,
  .num {
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s-3);
  }
  label {
    display: grid;
    gap: var(--s-1);
    min-width: 0;
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  input {
    min-height: var(--h-secondary);
    padding: 0 var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    background: var(--c-ink);
    color: var(--c-paper);
  }
  [aria-invalid='true'] {
    border-color: var(--c-signal);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-3);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: var(--s-4);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
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
    font: 400 var(--t-mono-size) / var(--t-mono-log-lh) var(--f-mono);
    white-space: pre-wrap;
  }
</style>
