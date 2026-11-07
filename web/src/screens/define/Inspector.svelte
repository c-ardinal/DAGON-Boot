<script lang="ts">
  import { tick } from 'svelte';
  import { INSPECT } from '../../app/copy.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import {
    boardMissing,
    connect,
    freeAddrs,
    definedPins,
    hopAvailable,
    LINK_KINDS,
    removeLink,
    setBoard,
    setBus,
    setLink,
    setLinkKind,
    setName,
    setNode,
    setUplink,
    template,
    uart0Pairs,
    uartBauds,
    i2cHzs,
    busNodes,
    type LinkKind,
    type TLink,
    type UplinkHost,
  } from '../../app/define/model.ts';
  import { problemChip, type Problem } from '../../app/define/problems.ts';
  import Icon from '../Icon.svelte';

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
  /** The bus speeds every node on the selected bus names in its MCU definition: the only ones it may be set to. */
  const hzs = $derived(bus ? i2cHzs(busNodes(t, bus.id)) : []);
  /** The 親機's own pins: what a bus or a link from it may be set to (the check refuses any other). */
  const hubPins = $derived(definedPins(hub));
  const MAX_ATTEMPTS = [1, 2, 3, 4, 5];
  const nodeLink = $derived(node ? (t.links ?? []).findIndex((l) => l.to === node.id) : -1);
  const name = (id: string) => d.doc.names[id] ?? id;
  const byId = (id: string) => t.nodes.find((n) => n.id === id);
  /** Kinds a 親機's uplink may be (not I2C or CAN towards a PC). */
  const UPLINK_KINDS: LinkKind[] = ['usb-cdc', 'uart', 'ble', 'wifi-tcp', 'ethernet'];
  const HOSTS: UplinkHost[] = ['pc-cli', 'browser', 'embedded'];
  const kindLabel = (k: string) => INSPECT.kinds[k] ?? INSPECT.kindsMore[k] ?? k;
  const kindOption = (k: LinkKind, ok: boolean) => (ok ? kindLabel(k) : INSPECT.notYetOption(kindLabel(k)));
  const hex = (a: number) => `0x${a.toString(16).padStart(2, '0')}`;

  /** Problems on the selected item at `field` (or below it). */
  function issues(field: string): Problem[] {
    return d.problems.filter((p) => p.select === d.selected && (p.field === field || p.field.startsWith(`${field}.`)));
  }

  /** Who else uses hub GPIO `n` (S20's in-use note). */
  function usedBy(n: number, self: string): string | null {
    for (const l of t.links ?? []) {
      if (`${l.to}.nrst` !== self && l.control?.nrst === n) return `${name(l.to)}の${INSPECT.reset}`;
      if (`${l.to}.boot` !== self && l.control?.boot === n) return `${name(l.to)}の${INSPECT.boot}`;
      if (l.pins && `${l.to}.tx` !== self && `${l.to}.rx` !== self && (l.pins.tx === n || l.pins.rx === n)) return name(l.to);
    }
    for (const b of t.buses ?? []) if (self !== 'bus' && (b.sda === n || b.scl === n)) return INSPECT.bus;
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
    {@const c = problemChip(p)}
    <!-- B-01 is said once, beside the ボード heading. -->
    {#if p.rule !== 'B-01'}<span class={c.kind} id="bad-{p.rule}-{field}">{c.mark} {p.short}</span>{/if}
  {/each}
{/snippet}

<!-- A pin chosen from its own end's definition: the only pins the check accepts; a value from a file that is not defined stays, flagged; another user of the pin is named. -->
{#snippet pinSelect(field: string, label: string, value: number | undefined, self: string, pins: number[], onpick: (n: number | undefined) => void)}
  <label
    >{label}<select data-field={field} value={value ?? ''} aria-invalid={issues(field).length > 0} onchange={(e) => onpick(num(e.currentTarget.value))}>
      <option value="">{INSPECT.unset}</option>
      {#if value !== undefined && !pins.includes(value)}<option value={value} translate="no">GPIO{value}{INSPECT.undefinedPin}</option>{/if}
      {#each pins as n (n)}
        {@const other = usedBy(n, self)}
        <option value={n} translate="no">GPIO{n}{other ? INSPECT.inUse(other) : ''}</option>
      {/each}
    </select>{@render bad(field)}</label
  >
{/snippet}

<section class="inspector" bind:this={root} aria-labelledby="inspector-h">
  {#if node}
    <div class="head">
      <h2 id="inspector-h" tabindex="-1">{name(node.id)}</h2>
      <!-- Destructive: red, top right; a modal confirms before anything goes. -->
      <button class="button-icon trash" aria-label={INSPECT.removeNode} title={INSPECT.removeNode} onclick={() => d.requestRemove(`node:${node.id}`)}><span class="layer"><Icon name="trash-2" /></span></button>
    </div>
    <div class="fields">
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
      <div class="field"><span class="label">{INSPECT.product}</span><span class="value" translate="no" title="{d.config.nodes.find((n) => n.id === node.id)?.product}・{d.config.nodes.find((n) => n.id === node.id)?.mcu}">{d.config.nodes.find((n) => n.id === node.id)?.product}・{d.config.nodes.find((n) => n.id === node.id)?.mcu}</span></div>
      <label
        >{INSPECT.role}<select data-field="role" value={node.role} onchange={(e) => d.apply(setNode(d.doc, node.id, 'role', e.currentTarget.value))}>
          <option value="coordinator">{INSPECT.roles.coordinator}</option>
          <option value="node">{INSPECT.roles.node}</option>
        </select>{@render bad('role')}</label
      >
      <label
        >{INSPECT.connectTo}<select
          data-field="connect"
          value={nodeLink >= 0 ? (t.links ?? [])[nodeLink].from : ''}
          onchange={(e) => {
            const parent = e.currentTarget.value;
            // なし removes the link: that is a deletion, so it asks first like every other.
            if (!parent) return void (nodeLink >= 0 && d.requestRemove(`link:${node.id}`));
            const without = nodeLink >= 0 ? removeLink(d.doc, nodeLink) : d.doc;
            d.apply(connect(without, node.id, parent));
          }}
        >
          <option value="">{INSPECT.none}</option>
          {#each t.nodes.filter((x) => x.id !== node.id) as p (p.id)}<option value={p.id}>{name(p.id)}</option>{/each}
        </select></label
      >
      <!-- A 親機 linked from another node has that node above it (つなぐ先); one nobody links to has its own uplink. -->
      {#if node.role === 'coordinator' && nodeLink < 0}
        {@const up = node.uplink ?? { kind: 'usb-cdc' as LinkKind, host: 'pc-cli' as UplinkHost }}
        <label
          >{INSPECT.uplink}<select data-field="uplink" value={up.kind} onchange={(e) => d.apply(setUplink(d.doc, node.id, { kind: e.currentTarget.value as LinkKind, host: up.host ?? 'pc-cli' }))}>
            {#each UPLINK_KINDS as k (k)}<option value={k}>{kindOption(k, hopAvailable(k, undefined, node))}</option>{/each}
          </select>{@render bad('uplink')}</label
        >
        <label
          >{INSPECT.uplinkHost}<select value={up.host ?? 'pc-cli'} onchange={(e) => d.apply(setUplink(d.doc, node.id, { kind: up.kind, host: e.currentTarget.value as UplinkHost }))}>
            {#each HOSTS as h (h)}<option value={h}>{INSPECT.hosts[h]}</option>{/each}
          </select></label
        >
      {/if}
      {#if !node.board}
        <h3>{INSPECT.board}{#if boardMissing(d.doc, node.id)}&ensp;<span class="warn">{INSPECT.boardUnset}</span>{/if}</h3>
        <label
          >{INSPECT.flashSize}<select
            data-field="board.flash"
            value={d.doc.boards?.[node.id]?.flashBytes ?? ''}
            aria-invalid={issues('board.flash').some((p) => p.severity === 'error')}
            aria-describedby="flash-help"
            onchange={(e) => d.apply(setBoard(d.doc, node.id, { flashBytes: e.currentTarget.value ? Number(e.currentTarget.value) : undefined }))}
          >
            <option value="">{INSPECT.unset}</option>
            {#each [18, 19, 20, 21, 22, 23, 24, 25] as p (p)}<option value={2 ** p}>{2 ** p >= 2 ** 20 ? `${2 ** (p - 20)} MiB` : `${2 ** (p - 10)} KiB`}</option>{/each}
          </select><span id="flash-help" class="help">{INSPECT.flashHelp}</span>{@render bad('board.flash')}</label
        >
      {/if}
      {#if nodeLink >= 0}
        {@render controlPins((t.links ?? [])[nodeLink], nodeLink)}
      {/if}
    </div>
    <details>
      <summary>{INSPECT.advanced}</summary>
      <div class="fields" translate="no">
        <label
          >{INSPECT.maxAttempts}<select data-field="max_attempts" value={node.max_attempts ?? ''} onchange={(e) => d.apply(setNode(d.doc, node.id, 'max_attempts', num(e.currentTarget.value)))}>
            <option value="">{INSPECT.unset}</option>
            {#each MAX_ATTEMPTS as n (n)}<option value={n}>{n}</option>{/each}
          </select></label
        >
        <label>{INSPECT.listenWindow}<input type="number" min="0" step="100" value={node.listen_window_ms ?? ''} onchange={(e) => d.apply(setNode(d.doc, node.id, 'listen_window_ms', num(e.currentTarget.value)))} /></label>
        <label>{INSPECT.flashMap}<input data-field="flash_map" value={node.flash_map ?? ''} onchange={(e) => d.apply(setNode(d.doc, node.id, 'flash_map', e.currentTarget.value))} />{@render bad('flash_map')}</label>
        <label>{INSPECT.tag}<input data-field="tag" value={node.tag} onchange={(e) => d.apply(setNode(d.doc, node.id, 'tag', e.currentTarget.value))} />{@render bad('tag')}</label>
      </div>
    </details>
  {:else if link}
    {@const from = byId(link.from)}
    {@const to = byId(link.to)}
    <div class="head">
      <h2 id="inspector-h" tabindex="-1" title="{name(link.from)} → {name(link.to)}">{name(link.from)} → {name(link.to)}</h2>
      <button class="button-icon trash" aria-label={INSPECT.removeLink} title={INSPECT.removeLink} onclick={() => d.requestRemove(`link:${link.to}`)}><span class="layer"><Icon name="trash-2" /></span></button>
    </div>
    <div class="fields">
      <label
        >{INSPECT.kind}<select data-field="kind" value={link.kind} aria-invalid={issues('kind').length > 0} onchange={(e) => d.apply(setLinkKind(d.doc, linkIndex, e.currentTarget.value as TLink['kind']))}>
          {#each LINK_KINDS as k (k)}<option value={k}>{kindOption(k, hopAvailable(k, from, to))}</option>{/each}
        </select>{@render bad('kind')}</label
      >
      {#if link.kind === 'i2c'}
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
            {#each template(to?.board ?? '')?.i2cPairs ?? [] as p (p.sda)}
              <option value="{p.sda}/{p.scl}">{p.sda}・{p.scl}</option>
            {/each}
          </select>{@render bad('node_pins')}</label
        >
      {:else if link.kind === 'uart'}
        {@const bauds = uartBauds(from, to)}
        {@const fromPins = link.iface === 'hw-uart0' ? uart0Pairs(from).flatMap((p) => [p.tx, p.rx]) : definedPins(from)}
        {@render pinSelect('pins', INSPECT.uartTx(name(link.from)), link.pins?.tx, `${link.to}.tx`, link.iface === 'hw-uart0' ? uart0Pairs(from).map((p) => p.tx) : fromPins, (n) => d.apply(setLink(d.doc, linkIndex, 'pins.tx', n)))}
        {@render pinSelect('pins.rx', INSPECT.uartRx(name(link.from)), link.pins?.rx, `${link.to}.rx`, link.iface === 'hw-uart0' ? uart0Pairs(from).map((p) => p.rx) : fromPins, (n) => d.apply(setLink(d.doc, linkIndex, 'pins.rx', n)))}
        <label
          >{INSPECT.baud}<select data-field="baud" value={link.baud ?? ''} aria-invalid={issues('baud').length > 0} title={bauds.length === 0 ? INSPECT.noBaud : undefined} onchange={(e) => d.apply(setLink(d.doc, linkIndex, 'baud', num(e.currentTarget.value)))}>
            <option value="">{bauds.length === 0 ? INSPECT.noBaud : INSPECT.unset}</option>
            {#if link.baud !== undefined && !bauds.includes(link.baud)}<option value={link.baud}>{link.baud}{INSPECT.undefinedPin}</option>{/if}
            {#each bauds as b (b)}<option value={b}>{b}</option>{/each}
          </select>{@render bad('baud')}</label
        >
      {/if}
      {@render controlPins(link, linkIndex)}
      {#if link.kind === 'i2c'}
        {@const b = (t.buses ?? []).findIndex((x) => x.id === link.bus)}
        {#if b >= 0}<button class="secondary" onclick={() => (d.selected = `bus:${b}`)}>{INSPECT.bus}</button>{/if}
      {/if}
    </div>
  {:else if bus}
    <div class="head"><h2 id="inspector-h" tabindex="-1">{INSPECT.bus}</h2></div>
    <div class="fields">
      {@render pinSelect('sda', INSPECT.busSda, bus.sda, 'bus', hubPins, (n) => d.apply(setBus(d.doc, busIndex, 'sda', n)))}
      {@render pinSelect('scl', INSPECT.busScl, bus.scl, 'bus', hubPins, (n) => d.apply(setBus(d.doc, busIndex, 'scl', n)))}
      <!-- Only the speeds every node's definition names; a speed set otherwise is shown as such until changed. -->
      <label
        >{INSPECT.speed}<select data-field="speed_hz" value={bus.speed_hz ?? ''} aria-invalid={issues('speed_hz').length > 0} title={hzs.length === 0 ? INSPECT.noSpeed : undefined} onchange={(e) => d.apply(setBus(d.doc, busIndex, 'speed_hz', num(e.currentTarget.value)))}>
          <option value="">{hzs.length === 0 ? INSPECT.noSpeed : INSPECT.unset}</option>
          {#if bus.speed_hz !== undefined && !hzs.includes(bus.speed_hz)}<option value={bus.speed_hz}>{INSPECT.hz(bus.speed_hz)}{INSPECT.undefinedPin}</option>{/if}
          {#each hzs as hz (hz)}<option value={hz}>{INSPECT.hz(hz)}</option>{/each}
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
    {@const owner = definedPins(byId(l.from))}
    {@render pinSelect('control.nrst', INSPECT.reset, l.control.nrst, `${l.to}.nrst`, owner, (n) => d.apply(setLink(d.doc, i, 'control.nrst', n)))}
    {@render pinSelect('control.boot', INSPECT.boot, l.control.boot, `${l.to}.boot`, owner, (n) => d.apply(setLink(d.doc, i, 'control.boot', n)))}
  {/if}
{/snippet}

<style>
  .inspector {
    padding: var(--s-5);
  }
  /* The sheet's head: its name, and the one destructive action at the top right. */
  /* The head stays at the top while the fields scroll under it. */
  .head {
    position: sticky;
    z-index: 1;
    top: calc(-1 * var(--s-5));
    display: flex;
    align-items: flex-start;
    gap: var(--s-2);
    margin: calc(-1 * var(--s-5)) var(--s-6) var(--s-4) 0;
    padding: var(--s-5) 0 var(--s-2);
    background: var(--c-layer-1);
  }
  h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font: 700 var(--t-heading) / 28px var(--f-ja);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  h2:focus {
    outline: none;
  }
  .trash {
    margin: -8px -8px -8px 0;
    color: var(--c-signal);
  }
  /* One column: every field gets the sheet's whole width, so no value is cut short. */
  .fields {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--s-4);
    margin-bottom: var(--s-4);
  }
  label,
  .field {
    display: grid;
    min-width: 0;
    gap: var(--s-2);
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  /* A read-only value in the same label style as every field; it stays on one line. */
  .value {
    min-height: var(--h-secondary);
    padding: 0 var(--s-3);
    overflow: hidden;
    border: 1px solid transparent;
    color: var(--c-paper);
    font: 400 var(--t-mono) / 28px var(--f-mono);
    text-overflow: ellipsis;
    white-space: nowrap;
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
    font: 400 var(--t-mono) / 20px var(--f-mono);
    transition: border-color var(--dur-quick) var(--ease-out);
  }
  /* The dropdown's own look: the ▾ sits 12 px from the edge, on the 4 px scale. */
  select {
    appearance: none;
    padding-right: var(--s-7);
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23f1efe8' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right var(--s-3) center;
    background-size: var(--i-s) var(--i-s);
    cursor: pointer;
  }
  select[aria-disabled='true'] {
    color: var(--c-muted);
  }
  input:focus-visible,
  select:focus-visible {
    outline: none;
    border-color: var(--c-paper);
    box-shadow: inset 0 0 0 1px var(--c-paper);
  }
  [aria-invalid='true'] {
    border-color: var(--c-signal);
  }
  h3 {
    margin: var(--s-2) 0 0;
    font: 700 var(--t-small) / 20px var(--f-ja);
  }
  .warn {
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  .help {
    color: var(--c-muted);
    font-weight: 400;
  }
  .error {
    color: var(--c-signal);
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  .warning,
  .unsupported {
    color: var(--c-paper);
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  details {
    margin: var(--s-4) 0;
  }
  summary {
    cursor: pointer;
    font: 700 var(--t-small) / 20px var(--f-ja);
    margin-bottom: var(--s-3);
  }
  summary:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
    border-radius: var(--r-s);
  }
</style>
