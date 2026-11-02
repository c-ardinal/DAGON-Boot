<script lang="ts">
  import { tick } from 'svelte';
  import { INSPECT } from '../../app/copy.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import { connect, freeAddrs, removeLink, removeNode, setBus, setLink, setLinkKind, setName, setNode, template, type TLink } from '../../app/define/model.ts';
  import type { Problem } from '../../app/define/problems.ts';

  interface Props {
    d: Define;
  }

  let { d }: Props = $props();

  const t = $derived(d.doc.topo);
  const nodeId = $derived(d.selected?.startsWith('node:') ? d.selected.slice(5) : null);
  const node = $derived(t.nodes.find((n) => n.id === nodeId));
  const linkIndex = $derived(d.selected?.startsWith('link:') ? (t.links ?? []).findIndex((l) => `link:${l.to}` === d.selected) : -1);
  const link = $derived(linkIndex >= 0 ? (t.links ?? [])[linkIndex] : undefined);
  const busIndex = $derived(d.selected?.startsWith('bus:') ? Number(d.selected.slice(4)) : -1);
  const bus = $derived(busIndex >= 0 ? t.buses?.[busIndex] : undefined);
  const hub = $derived(t.nodes.find((n) => n.role === 'coordinator'));
  const hubPins = $derived((hub?.board ? template(hub.board)?.header : null) ?? []);
  const nodeLink = $derived(node ? (t.links ?? []).findIndex((l) => l.to === node.id) : -1);
  const name = (id: string) => d.doc.names[id] ?? id;
  const hex = (a: number) => `0x${a.toString(16).padStart(2, '0')}`;
  const gpioNo = (pin: string) => Number(pin.replace(/^GPIO/, ''));

  /** Problems on the selected item at `field` (or below it). */
  function issues(field: string): Problem[] {
    return d.problems.filter((p) => p.select === d.selected && (p.field === field || p.field.startsWith(`${field}.`)));
  }

  /** Who else uses hub GPIO `n` (S20's in-use note). */
  function usedBy(n: number, self: string): string | null {
    for (const l of t.links ?? []) {
      if (`${l.to}.nrst` !== self && l.control?.nrst === n) return `${name(l.to)}の${INSPECT.reset}`;
      if (`${l.to}.boot` !== self && l.control?.boot === n) return `${name(l.to)}の${INSPECT.boot}`;
      if (l.pins && (l.pins.tx === n || l.pins.rx === n)) return name(l.to);
    }
    for (const b of t.buses ?? []) if (b.sda === n || b.scl === n) return INSPECT.bus;
    return null;
  }

  let root = $state<HTMLElement>();
  $effect(() => {
    if (d.focusRequest === 0) return;
    const f = d.focusField;
    void tick().then(() => (root?.querySelector<HTMLElement>(`[data-field="${f}"]`) ?? root?.querySelector<HTMLElement>('h2'))?.focus());
  });

  const num = (v: string) => (v === '' ? undefined : Number(v));
</script>

{#snippet bad(field: string)}
  {#each issues(field) as p (p.rule + p.path)}
    <span class="bad" id="bad-{p.rule}-{field}">! {p.short}</span>
  {/each}
{/snippet}

<section class="inspector" bind:this={root} aria-labelledby="inspector-h">
  {#if node}
    <h2 id="inspector-h" tabindex="-1">{name(node.id)}</h2>
    <div class="grid">
      <label>{INSPECT.name}<input value={d.doc.names[node.id] ?? ''} onchange={(e) => d.apply(setName(d.doc, node.id, e.currentTarget.value))} /></label>
      <label
        >{INSPECT.id}<input
          data-field="id"
          translate="no"
          value={node.id}
          aria-invalid={issues('id').length > 0}
          onchange={(e) => {
            const v = e.currentTarget.value.trim();
            d.apply(setNode(d.doc, node.id, 'id', v));
            d.selected = `node:${v}`;
          }}
        />{@render bad('id')}</label
      >
      <label
        >{INSPECT.role}<select data-field="role" value={node.role} onchange={(e) => d.apply(setNode(d.doc, node.id, 'role', e.currentTarget.value))}>
          <option value="coordinator">{INSPECT.roles.coordinator}</option>
          <option value="node">{INSPECT.roles.node}</option>
        </select>{@render bad('role')}</label
      >
      {#if node.role === 'node'}
        <label
          >{INSPECT.connectTo}<select
            data-field="connect"
            value={nodeLink >= 0 ? 'hub' : ''}
            onchange={(e) => d.apply(e.currentTarget.value ? connect(d.doc, node.id) : removeLink(d.doc, nodeLink))}
          >
            <option value="">{INSPECT.none}</option>
            {#if hub}<option value="hub">{name(hub.id)}</option>{/if}
          </select></label
        >
      {/if}
    </div>
    <dl class="pair"><dt>{INSPECT.product}</dt><dd translate="no">{d.config.nodes.find((n) => n.id === node.id)?.product} · {d.config.nodes.find((n) => n.id === node.id)?.mcu}</dd></dl>
    {#if nodeLink >= 0}
      {@render controlPins((t.links ?? [])[nodeLink], nodeLink)}
    {/if}
    <details>
      <summary>{INSPECT.advanced}</summary>
      <div class="grid" translate="no">
        <label>max_attempts<input data-field="max_attempts" type="number" min="1" max="5" value={node.max_attempts ?? ''} onchange={(e) => d.apply(setNode(d.doc, node.id, 'max_attempts', num(e.currentTarget.value)))} /></label>
        <label>listen_window_ms<input type="number" min="0" value={node.listen_window_ms ?? ''} onchange={(e) => d.apply(setNode(d.doc, node.id, 'listen_window_ms', num(e.currentTarget.value)))} /></label>
        <label>flash_map<input data-field="flash_map" value={node.flash_map ?? ''} onchange={(e) => d.apply(setNode(d.doc, node.id, 'flash_map', e.currentTarget.value))} />{@render bad('flash_map')}</label>
        <label>tag<input data-field="tag" value={node.tag} onchange={(e) => d.apply(setNode(d.doc, node.id, 'tag', e.currentTarget.value))} />{@render bad('tag')}</label>
      </div>
    </details>
    <button class="secondary" onclick={() => { d.apply(removeNode(d.doc, node.id)); d.selected = null; }}>{INSPECT.removeNode}</button>
  {:else if link}
    <h2 id="inspector-h" tabindex="-1">{name(link.from)} → {name(link.to)}</h2>
    <fieldset class="seg">
      <legend>{INSPECT.kind}</legend>
      {#each ['i2c', 'uart'] as k (k)}
        <label
          ><input type="radio" name="kind" value={k} checked={link.kind === k} onchange={() => d.apply(setLinkKind(d.doc, linkIndex, k as TLink['kind']))} />{INSPECT.kinds[k]}</label
        >
      {/each}
    </fieldset>
    {@render bad('kind')}
    {#if link.kind === 'i2c'}
      <div class="grid">
        <label
          >{INSPECT.addr}<select data-field="addr" value={link.addr} aria-invalid={issues('addr').length > 0} onchange={(e) => d.apply(setLink(d.doc, linkIndex, 'addr', Number(e.currentTarget.value)))}>
            {#each [...new Set([...(link.addr !== undefined ? [link.addr] : []), ...freeAddrs(t, linkIndex)])].sort((a, b) => a - b) as a (a)}
              <option value={a}>{hex(a)}</option>
            {/each}
          </select>{@render bad('addr')}</label
        >
        <label
          >{INSPECT.nodePins}<select
            data-field="node_pins"
            value={link.node_pins ? `${link.node_pins.sda}/${link.node_pins.scl}` : ''}
            onchange={(e) => {
              const [sda, scl] = e.currentTarget.value.split('/');
              d.apply(setLink(d.doc, linkIndex, 'node_pins', sda ? { sda, scl } : undefined));
            }}
          >
            <option value="">{INSPECT.unset}</option>
            {#each template(t.nodes.find((n) => n.id === link.to)?.board ?? '')?.i2cPairs ?? [] as p (p.sda)}
              <option value="{p.sda}/{p.scl}">{p.sda} · {p.scl}</option>
            {/each}
          </select>{@render bad('node_pins')}</label
        >
      </div>
    {:else}
      <div class="grid">
        <label>{INSPECT.uartPins} TX<input data-field="pins" type="number" value={link.pins?.tx ?? ''} onchange={(e) => d.apply(setLink(d.doc, linkIndex, 'pins.tx', num(e.currentTarget.value)))} />{@render bad('pins')}</label>
        <label>{INSPECT.uartPins} RX<input type="number" value={link.pins?.rx ?? ''} onchange={(e) => d.apply(setLink(d.doc, linkIndex, 'pins.rx', num(e.currentTarget.value)))} /></label>
        <label>{INSPECT.baud}<input data-field="baud" type="number" value={link.baud ?? ''} onchange={(e) => d.apply(setLink(d.doc, linkIndex, 'baud', num(e.currentTarget.value)))} /></label>
      </div>
    {/if}
    {@render controlPins(link, linkIndex)}
    <div class="actions">
      {#if link.kind === 'i2c'}
        {@const b = (t.buses ?? []).findIndex((x) => x.id === link.bus)}
        {#if b >= 0}<button class="secondary" onclick={() => (d.selected = `bus:${b}`)}>{INSPECT.bus}</button>{/if}
      {/if}
      <button class="secondary" onclick={() => { d.apply(removeLink(d.doc, linkIndex)); d.selected = null; }}>{INSPECT.removeLink}</button>
    </div>
  {:else if bus}
    <h2 id="inspector-h" tabindex="-1">{INSPECT.bus}</h2>
    <div class="grid">
      <label>{INSPECT.busPins} SDA<input data-field="sda" type="number" value={bus.sda} onchange={(e) => d.apply(setBus(d.doc, busIndex, 'sda', num(e.currentTarget.value)))} />{@render bad('sda')}</label>
      <label>{INSPECT.busPins} SCL<input type="number" value={bus.scl} onchange={(e) => d.apply(setBus(d.doc, busIndex, 'scl', num(e.currentTarget.value)))} /></label>
      <label
        >{INSPECT.speed}<select data-field="speed_hz" value={bus.speed_hz} onchange={(e) => d.apply(setBus(d.doc, busIndex, 'speed_hz', Number(e.currentTarget.value)))}>
          {#each Object.entries(INSPECT.speeds) as [hz, label] (hz)}<option value={Number(hz)}>{label}</option>{/each}
        </select>{@render bad('speed_hz')}</label
      >
      <label
        >{INSPECT.pullups}<select data-field="pullups" value={bus.pullups ?? ''} onchange={(e) => d.apply(setBus(d.doc, busIndex, 'pullups', e.currentTarget.value))}>
          <option value="">{INSPECT.unset}</option>
          {#each Object.entries(INSPECT.pullupKinds) as [k, label] (k)}<option value={k}>{label}</option>{/each}
        </select>{@render bad('pullups')}</label
      >
    </div>
  {/if}
</section>

{#snippet controlPins(l: TLink, i: number)}
  {#if l.control?.kind === 'full'}
    <div class="grid">
      {#each [['nrst', INSPECT.reset], ['boot', INSPECT.boot]] as [k, label] (k)}
        {@const v = l.control[k as 'nrst' | 'boot']}
        <label
          >{label}<select
            data-field="control.{k}"
            value={v ?? ''}
            aria-invalid={issues(`control.${k}`).length > 0}
            onchange={(e) => d.apply(setLink(d.doc, i, `control.${k}`, num(e.currentTarget.value)))}
          >
            <option value="">{INSPECT.unset}</option>
            {#each hubPins as pin (pin)}
              {@const n = gpioNo(pin)}
              {@const other = usedBy(n, `${l.to}.${k}`)}
              <option value={n} translate="no">{pin}{other ? INSPECT.inUse(other) : ''}</option>
            {/each}
          </select>{@render bad(`control.${k}`)}</label
        >
      {/each}
    </div>
  {/if}
{/snippet}

<style>
  .inspector {
    padding: var(--s-5);
  }
  h2 {
    margin: 0 0 var(--s-4);
    font: 700 var(--t-title-size) / var(--t-title-lh) var(--f-ja);
  }
  h2:focus {
    outline: none;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s-4);
    margin-bottom: var(--s-4);
  }
  label {
    display: grid;
    min-width: 0;
    gap: var(--s-2);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  input,
  select {
    width: 100%;
    box-sizing: border-box;
    min-height: var(--h-secondary);
    padding: 0 var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
  }
  [aria-invalid='true'] {
    border-color: var(--c-signal);
  }
  .bad {
    color: var(--c-signal);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .pair {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-4);
    margin: 0 0 var(--s-4);
  }
  .pair dt {
    color: var(--c-muted);
  }
  .pair dd {
    margin: 0;
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
  }
  details {
    margin: var(--s-4) 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-3);
  }
  summary {
    cursor: pointer;
    font: 700 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    margin-bottom: var(--s-3);
  }
  .seg {
    display: flex;
    gap: 0;
    margin: 0 0 var(--s-4);
    padding: 0;
    border: 0;
  }
  .seg legend {
    margin-bottom: var(--s-2);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .seg label {
    flex: 1;
    display: block;
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--c-line);
    font-weight: 400;
    cursor: pointer;
  }
  .seg label:has(input:checked) {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .seg input {
    position: absolute;
    opacity: 0;
    min-height: 0;
  }
  .seg label:focus-within {
    outline: 3px solid var(--c-paper);
    outline-offset: 2px;
  }
</style>
