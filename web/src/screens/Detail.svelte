<script lang="ts">
  import { DETAIL, NODE, NODE_ACTION, SLOT, TRIAL_CHOICE, WIRING } from '../app/copy.ts';
  import type { Live } from '../app/operate.svelte.ts';
  import type { Config, Link, Node } from '../app/sample.ts';
  import { previousVersion } from '../app/slots.ts';
  import { versionText, type RecoverResult, type StatusResp } from '../wasm/dagon.ts';
  import type { HelloResp } from '../serial/hello.ts';

  interface Props {
    config: Config | null;
    live: Record<string, Live>;
    selected: string | null;
    connected: boolean;
    /** Node actions are offered (watching, nothing running). */
    act?: boolean;
    /** M38 "このバージョンで確定する". */
    onkeep?: (id: string) => void;
    /** "前のバージョンに戻す". */
    onback?: (id: string) => void;
    /** The last recovery, for its figures under 詳しく見る. */
    recovery?: { id: string; result: RecoverResult } | null;
  }

  let { config, live, selected, connected, act = false, onkeep, onback, recovery = null }: Props = $props();

  function recoveryRows(id: string): [string, string][] {
    if (recovery?.id !== id) return [];
    const r = recovery.result;
    const rows: [string, string][] = [[NODE_ACTION.path, NODE_ACTION.paths[r.path]]];
    if (r.rom) rows.push([NODE_ACTION.romRtt, DETAIL.ms(r.rom.maxRttMs)], [NODE_ACTION.romAttempts, String(r.rom.attempts)]);
    return rows;
  }

  const node = $derived(selected?.startsWith('node:') ? config?.nodes.find((n) => `node:${n.id}` === selected) : undefined);
  const link = $derived(selected?.startsWith('link:') ? config?.links.find((l) => `link:${l.to}` === selected) : undefined);

  const SLOT_EMPTY = 0;
  const SLOT_TRYING = 3;
  const SLOT_CONFIRMED = 4;
  const FACTORY = 2;
  const MODE_FACTORY = 3;
  const SHORT_HEX = 4;

  const clock = new Intl.DateTimeFormat('ja-JP', { hour: '2-digit', minute: '2-digit' });
  const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');

  function slotWord(i: number, s: StatusResp, h: HelloResp | null): string {
    const slot = s.slots[i];
    const running = i === FACTORY ? h?.mode === MODE_FACTORY : i === s.active;
    if (running) {
      const settled = slot.state === SLOT_CONFIRMED || slot.state === SLOT_TRYING;
      return settled ? `${SLOT.inUse}・${SLOT.states[slot.state]}` : SLOT.inUse;
    }
    if (i === FACTORY) return slot.state === SLOT_EMPTY ? SLOT.states[SLOT_EMPTY] : SLOT.present;
    if (slot.state === SLOT_CONFIRMED) return SLOT.previous;
    return SLOT.states[slot.state] ?? '';
  }

  function slotVersion(i: number, s: StatusResp): string {
    const slot = s.slots[i];
    return slot.state === SLOT_EMPTY || i === FACTORY ? '' : versionText(slot.version);
  }

  function reason(code: number): string {
    return code === 0 ? DETAIL.none : (DETAIL.reasons[code] ?? String(code));
  }

  function health(l: Live | undefined): [string, string][] {
    if (!l) return [];
    const rows: [string, string][] = [];
    if (l.rttMs !== null) rows.push([DETAIL.rtt, DETAIL.ms(l.rttMs)]);
    if (l.lastSeen !== null) rows.push([DETAIL.lastSeen, clock.format(l.lastSeen)]);
    if (l.status) {
      rows.push([DETAIL.attempts, String(l.status.attempts)]);
      rows.push([DETAIL.lastReason, reason(l.status.lastReason)]);
    }
    return rows;
  }

  function raw(l: Live | undefined): [string, string][] {
    const h = l?.hello;
    const s = l?.status;
    const rows: [string, string][] = [];
    if (h) {
      rows.push(
        ['proto_ver', String(h.protoVer)],
        ['role', String(h.role)],
        ['port_kind', String(h.portKind)],
        ['mcu_id', `0x${h.mcuId.toString(16).padStart(4, '0')}`],
        ['mode', String(h.mode)],
        ['active_version', versionText(h.activeVersion)],
        ['trial', String(h.trial ? 1 : 0)],
        ['key_id', hex(h.keyId)],
        ['max_payload', String(h.maxPayload)],
      );
    }
    if (s) {
      s.slots.forEach((x, i) =>
        rows.push([`slot[${i}]`, `state ${x.state} · ${versionText(x.version)} · ${hex(x.digest16.subarray(0, SHORT_HEX))}…`]),
      );
      rows.push(
        ['active', String(s.active)],
        ['confirmed', String(s.confirmed)],
        ['attempts', String(s.attempts)],
        ['last_reason', String(s.lastReason)],
        ['log_generation', String(s.logGeneration)],
      );
    }
    return rows;
  }

  function wiringText(l: Link): string {
    const lines = [WIRING.cols.join('\t'), ...l.wires.map((w) => [w.signal, w.hubPin, w.nodePin, w.shared ? WIRING.shared : WIRING.own].join('\t'))];
    return lines.join('\n');
  }

  async function copyWiring(l: Link) {
    await navigator.clipboard?.writeText(wiringText(l)).catch(() => undefined);
  }
</script>

<div class="detail">
  {#if !config}
    <section aria-labelledby="intro-h">
      <h2 id="intro-h" class="title">{DETAIL.introHeading}</h2>
      <ol class="intro">
        {#each DETAIL.introSteps as s, i (s)}
          <li><span class="n">{String(i + 1).padStart(2, '0')}</span>{s}</li>
        {/each}
      </ol>
      <p class="note">{DETAIL.introNote}</p>
    </section>
  {:else if node}
    {@render nodeDetail(node, live[node.id])}
  {:else if link}
    {@render wiring(link)}
  {:else}
    <p class="lead">{DETAIL.nothingSelected}</p>
  {/if}
</div>

{#snippet nodeDetail(n: Node, l: Live | undefined)}
  <section aria-labelledby="detail-h">
    <h2 id="detail-h" class="title" tabindex="-1">{n.name}</h2>
    {#if n.notYet}
      <!-- Updating does not reach it yet (V-23): nothing is watched or written there. -->
      <p class="choice">{NODE.notYet}</p>
      <p>{NODE.notYetWhy[n.notYet] ?? ''}</p>
    {:else if connected}
      {@const rows = health(l)}
      {@const r = raw(l)}
      {#if rows.length}
        <dl class="pairs">
          {#each rows as [k, v] (k)}
            <dt>{k}</dt>
            <dd>{v}</dd>
          {/each}
        </dl>
      {/if}
      {#if l?.status || l?.responding !== false}
        <h3>{SLOT.heading}</h3>
      {/if}
      {#if l?.status}
        <table class="slots" class:stale={l.responding === false}>
          <tbody>
            {#each SLOT.rows as row, i (row)}
              <tr>
                <th scope="row">{row}</th>
                <td class="v" translate="no">{slotVersion(i, l.status)}</td>
                <td>{slotWord(i, l.status, l.hello)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else if l?.responding !== false}
        <div class="slots-sk" aria-hidden="true">
          {#each SLOT.rows as row (row)}<span class="skeleton"></span>{/each}
        </div>
      {/if}
      {@const back = previousVersion(l?.status)}
      {#if act && l?.responding && l.hello?.trial}
        <p class="choice">{TRIAL_CHOICE.sentence}</p>
        <div class="actions">
          <button class="secondary" onclick={() => onkeep?.(n.id)}>{TRIAL_CHOICE.keep}</button>
          {#if back}<button class="secondary" onclick={() => onback?.(n.id)}>{TRIAL_CHOICE.back}</button>{/if}
        </div>
      {:else if act && l?.responding && back}
        <div class="actions">
          <button class="secondary" onclick={() => onback?.(n.id)}>{NODE_ACTION.back(back)}</button>
        </div>
      {/if}
      {@const rec = recoveryRows(n.id)}
      {#if r.length || rec.length}
        <details class="more">
          <summary>{DETAIL.details}</summary>
          {#if rec.length}
            <dl class="raw">
              {#each rec as [k, v] (k)}
                <dt>{k}</dt>
                <dd>{v}</dd>
              {/each}
            </dl>
          {/if}
          <dl class="raw" translate="no">
            {#each r as [k, v] (k)}
              <dt>{k}</dt>
              <dd>{v}</dd>
            {/each}
          </dl>
        </details>
      {/if}
    {/if}
  </section>
{/snippet}

{#snippet wiring(l: Link)}
  <section class="wiring" data-print aria-labelledby="wiring-h">
    <h2 id="wiring-h" class="title">{WIRING.heading}</h2>
    <div class="scroll">
    <table class="wires">
      <thead>
        <tr>{#each WIRING.cols as c (c)}<th scope="col">{c}</th>{/each}</tr>
      </thead>
      <tbody translate="no">
        {#each l.wires as w (w.signal)}
          <tr>
            <th scope="row">{w.signal}</th>
            <td>{w.hubPin}</td>
            <td>{w.nodePin}</td>
            <td>{#if w.shared}{WIRING.shared}{:else}<span aria-label={WIRING.ownName}>{WIRING.own}</span>{/if}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    </div>
    <h3>{WIRING.legendHeading}</h3>
    <dl class="pairs legend">
      {#each WIRING.legend as [k, v] (k)}
        <dt>{k}</dt>
        <dd>{v}</dd>
      {/each}
    </dl>
    <div class="actions">
      <button class="secondary" onclick={() => copyWiring(l)}>{WIRING.copy}</button>
      <button class="secondary" onclick={() => window.print()}>{WIRING.print}</button>
    </div>
  </section>
{/snippet}

<style>
  .detail {
    padding: var(--s-5);
  }
  .title {
    margin: 0 0 var(--s-3);
    font: 700 var(--t-title-size) / var(--t-title-lh) var(--f-ja);
    letter-spacing: 0.02em;
    font-feature-settings: 'palt';
    text-wrap: balance;
  }
  .title:focus {
    outline: none;
  }
  .choice {
    margin: var(--s-4) 0 var(--s-3);
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  h3 {
    margin: var(--s-4) 0 var(--s-3);
    padding-top: var(--s-4);
    border-top: 1px solid var(--c-line);
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
    font-feature-settings: 'palt';
  }
  .lead {
    margin: 0;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .intro {
    display: grid;
    gap: var(--s-2);
    margin: 0;
    padding: 0;
    list-style: none;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    text-wrap: pretty;
  }
  .n {
    display: inline-block;
    width: var(--s-6);
    font: 400 var(--t-body-size) / 1 var(--f-version);
    font-variant-numeric: tabular-nums;
    color: var(--c-muted);
  }
  .note {
    margin: var(--s-4) 0 0;
    color: var(--c-muted);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .pairs {
    display: grid;
    grid-template-columns: minmax(6rem, max-content) 1fr;
    gap: var(--s-2) var(--s-4);
    align-items: baseline;
    margin: 0;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  dt {
    color: var(--c-muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  th,
  td {
    height: 2.5rem;
    padding: var(--s-2) var(--s-3);
    border-bottom: 1px solid var(--c-line);
    text-align: left;
    font-weight: 400;
  }
  thead th {
    color: var(--c-muted);
  }
  .v {
    font: 400 var(--t-version-small-size) / 1 var(--f-version);
    font-variant-numeric: tabular-nums;
  }
  .stale {
    color: var(--c-muted);
  }
  .slots-sk {
    display: grid;
    gap: var(--s-2);
  }
  .slots-sk .skeleton {
    height: var(--s-6);
  }
  .more {
    margin-top: var(--s-6);
  }
  summary {
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    cursor: pointer;
  }
  .raw {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--s-1) var(--s-4);
    margin: var(--s-2) 0 0;
    font: 500 var(--t-mono-size) / var(--t-mono-log-lh) var(--f-mono);
    overflow-wrap: anywhere;
  }
  .scroll {
    overflow-x: auto;
  }
  .wires {
    font-family: var(--f-mono);
    font-size: var(--t-mono-size);
  }
  .wires :is(th, td):last-child {
    white-space: nowrap;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-3);
    margin-top: var(--s-5);
  }
  @media print {
    .actions {
      display: none;
    }
  }
</style>
