<script lang="ts">
  import { INSPECT, TABLE, WIRING } from '../../app/copy.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import { connect } from '../../app/define/model.ts';

  interface Props {
    d: Define;
    /** "ノードを加える": the node list places nodes. */
    onaddnode: () => void;
    /** The open tab (the menu opens 配線). */
    tab?: number;
  }

  let { d, onaddnode, tab = $bindable(0) }: Props = $props();
  const c = $derived(d.config);
  const unlinked = $derived(d.doc.topo.nodes.find((n) => n.role === 'node' && !(d.doc.topo.links ?? []).some((l) => l.to === n.id)));
  const nameOf = (id: string) => c.nodes.find((n) => n.id === id)?.name ?? id;

  function keys(e: KeyboardEvent) {
    const step = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    tab = (tab + step + TABLE.tabs.length) % TABLE.tabs.length;
    (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[role="tab"]')[tab]?.focus();
  }

  function edit(key: string) {
    d.selected = key;
    d.focusField = '';
    d.focusRequest++;
  }
</script>

<div class="table-view">
  <div class="tools">
    <button class="secondary" onclick={onaddnode}>{TABLE.addNode}</button>
    <button class="secondary" aria-disabled={!unlinked} title={unlinked ? undefined : TABLE.addLinkWhy} onclick={() => unlinked && d.apply(connect(d.doc, unlinked.id))}>{TABLE.addLink}</button>
  </div>
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div role="tablist" tabindex="-1" onkeydown={keys}>
    {#each TABLE.tabs as name, i (name)}
      <button role="tab" id="tab-{i}" aria-selected={tab === i} aria-controls="panel-{i}" tabindex={tab === i ? 0 : -1} onclick={() => (tab = i)}>{name}</button>
    {/each}
  </div>
  <div class="scroll" role="tabpanel" id="panel-{tab}" aria-labelledby="tab-{tab}">
    {#if tab === 0}
      <table>
        <thead><tr>{#each TABLE.nodeCols as h (h)}<th scope="col">{h}</th>{/each}<th></th></tr></thead>
        <tbody>
          {#each c.nodes as n (n.id)}
            <tr>
              <td>{n.no}</td>
              <th scope="row">{n.name}</th>
              <td translate="no">{n.product} · {n.mcu}</td>
              <td>{INSPECT.roles[n.role === 'hub' ? 'coordinator' : 'node']}</td>
              <td translate="no">{n.via}</td>
              <td translate="no">{[n.reset, n.boot].filter(Boolean).join(' · ')}</td>
              <td class="ops">
                <button class="secondary" onclick={() => edit(`node:${n.id}`)}>{TABLE.edit}</button>
                <button class="secondary destructive" onclick={() => d.requestRemove(`node:${n.id}`)}>{TABLE.remove}</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else if tab === 1}
      <table>
        <thead><tr>{#each TABLE.linkCols as h (h)}<th scope="col">{h}</th>{/each}<th></th></tr></thead>
        <tbody>
          {#each c.links as l (l.to)}
            <tr>
              <td>{nameOf(l.from)}</td>
              <th scope="row">{nameOf(l.to)}</th>
              <td>{l.kind}</td>
              <td translate="no">{l.addr}</td>
              <td translate="no">{l.wires.map((w) => w.hubPin).filter((p) => p.startsWith('GPIO')).join(' · ')}</td>
              <td class="ops">
                <button class="secondary" onclick={() => edit(`link:${l.to}`)}>{TABLE.edit}</button>
                <button class="secondary destructive" onclick={() => d.requestRemove(`link:${l.to}`)}>{TABLE.remove}</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <table>
        <thead><tr><th scope="col">{TABLE.linkCols[1]}</th>{#each WIRING.cols as h (h)}<th scope="col">{h}</th>{/each}</tr></thead>
        <tbody translate="no">
          {#each c.links as l (l.to)}
            {#each l.wires as w (w.signal)}
              <tr><td>{nameOf(l.to)}</td><th scope="row">{w.signal}</th><td>{w.hubPin}</td><td>{w.nodePin}</td><td>{w.shared ? WIRING.shared : WIRING.own}</td></tr>
            {/each}
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>

<style>
  .table-view {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: var(--s-3);
    min-width: 0;
    min-height: 0;
    height: 100%;
    padding: var(--s-4) var(--s-5);
  }
  .tools {
    display: flex;
    gap: var(--s-3);
  }
  [role='tablist'] {
    display: flex;
    gap: var(--s-2);
    border-bottom: 1px solid var(--c-line);
  }
  [role='tab'] {
    padding: var(--s-2) var(--s-4);
    border: 0;
    border-bottom: 3px solid transparent;
    background: none;
    color: var(--c-muted);
    font: 700 var(--t-small) / 20px var(--f-ja);
    cursor: pointer;
  }
  [role='tab'][aria-selected='true'] {
    border-bottom-color: var(--c-paper);
    color: var(--c-paper);
  }
  .scroll {
    min-height: 0;
    overflow: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  thead th {
    position: sticky;
    top: 0;
    background: var(--c-layer-0);
    color: var(--c-muted);
    text-align: left;
  }
  th,
  td {
    padding: var(--s-2) var(--s-3);
    border-bottom: 1px solid var(--c-line);
    white-space: nowrap;
  }
  tbody th {
    text-align: left;
  }
  /* The actions end the row; a narrow window scrolls the table sideways, nothing hides under anything. */
  .ops {
    white-space: nowrap;
  }
  .ops button + button {
    margin-left: var(--s-2);
  }
  .destructive {
    border-color: var(--c-signal);
    color: var(--c-signal);
  }
</style>
