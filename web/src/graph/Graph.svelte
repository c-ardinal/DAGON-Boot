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
    name: string;
  }

  /** A node chip set by the page (update progress) instead of the watch result. */
  export interface Chip {
    chip: string;
    spoken: string;
    silent: boolean;
    /** A label-value line under the chip ("目安 約 1 分", "自動の再接続 あと 30 s"). */
    note?: string;
  }
</script>

<script lang="ts">
  import { CANVAS, LINK, NODE } from '../app/copy.ts';
  import { KIND_LABEL, type LinkKind } from '../app/define/model.ts';
  import type { Live } from '../app/operate.svelte.ts';
  import type { Config, Link, Node } from '../app/sample.ts';
  import { versionText } from '../wasm/dagon.ts';
  import { lengthTable, neighbour, pathD, pointAt, shifted, tAtLength, tangentAt, tieLengths, verticalLink, wireOffsets, type Pt } from './geometry.ts';

  interface Props {
    config: Config | null;
    live: Record<string, Live>;
    /** The hub is connected: the PC link is solid and nodes carry live marks. */
    connected: boolean;
    /** The configuration is on its way to the hub. */
    sending: boolean;
    selected: string | null;
    onselect: (key: string) => void;
    dot: Dot | null;
    chips?: Record<string, Chip>;
    /** Drawing scale (define mode's zoom; scrolling pans). */
    zoom?: number;
    /** Nodes being written: how full their water is, 0–100 (SC-O5, owner 11/2 and 11/3). */
    fill?: Record<string, number>;
  }

  let { config, live, connected, sending, selected, onselect, dot, chips = {}, zoom = 1, fill = {} }: Props = $props();

  const NODE_W = 240;
  const MIN_W = 600;
  const PC = { y: 24, w: 240, h: 44 };
  const L1_TOP = 96;
  const NODE_TOP_GAP = 16;
  const LINK_ROOM = 96;
  const L2_GAP = 64;
  const BOTTOM = 24;
  const ARC_SAG = 8;
  const EMPTY_H = 208;

  // Content box, so a scrollbar appearing narrows the canvas instead of adding a sideways scroll.
  let box = $state<DOMRectReadOnly>();
  const boxW = $derived(box?.width ?? 800);
  const boxH = $derived(box?.height ?? 800);
  let heights = $state<Record<string, number>>({});
  let focusKey = $state('node:hub');
  let canvas: HTMLDivElement;

  const W = $derived(Math.max(MIN_W, boxW));
  const hubY = L1_TOP + NODE_TOP_GAP;
  /** Row of a node: 0 the 親機 (L1), 1 its 子機, 2 and on deeper (L2 and below). */
  const depthOf = (n: Node) => n.depth ?? (n.role === 'hub' ? 0 : 1);
  const rows = $derived.by(() => {
    const out: Node[][] = [[], []];
    for (const n of config?.nodes ?? []) (out[depthOf(n)] ??= []).push(n);
    return out.map((r) => r ?? []);
  });
  const children = $derived(rows[1]);
  const rowCount = (d: number) => (config ? rows[d]?.length ?? 0 : d === 0 ? 1 : 2);
  const rowH = (d: number) => (config ? Math.max(0, ...(rows[d] ?? []).map((n) => heights[n.id] ?? (d === 0 ? 200 : 240))) : EMPTY_H);
  const hubH = $derived(config ? Math.max(200, rowH(0)) : EMPTY_H);
  const L2_TOP = $derived(hubY + hubH + LINK_ROOM);
  const childY = $derived(L2_TOP + 64);
  const childH = $derived(rowH(1));
  /** Top of row `d` (rows 2 and on stack below the 子機, a link's room apart). */
  const rowY = (d: number) => (d === 0 ? hubY : childY + (d - 1) * (childH + LINK_ROOM));
  const H = $derived(Math.max(boxH, rowY(Math.max(1, rows.length - 1)) + rowH(Math.max(1, rows.length - 1)) + BOTTOM));
  const hubX = $derived(rowX(0, 0));

  function rowX(d: number, i: number): number {
    const count = rowCount(d);
    const total = count * NODE_W + (count - 1) * L2_GAP;
    return W / 2 - total / 2 + i * (NODE_W + L2_GAP);
  }

  function childX(i: number): number {
    return rowX(1, i);
  }

  const pos = $derived(
    Object.fromEntries((config?.nodes ?? []).map((n) => [n.id, { x: rowX(depthOf(n), rows[depthOf(n)].indexOf(n)), y: rowY(depthOf(n)) }])),
  );

  const nameOf = (id: string) => config?.nodes.find((n) => n.id === id)?.name ?? id;

  const drawn = $derived(
    (config?.links ?? []).map((l, i, all) => {
      const key = `link:${l.to}`;
      const open = selected === key;
      const from = pos[l.from];
      const to = pos[l.to];
      const slot = (i + 1) / (all.length + 1);
      const base = verticalLink({ x: from.x + NODE_W * slot, y: from.y + (heights[l.from] ?? 200) }, { x: to.x + NODE_W / 2, y: to.y });
      const table = lengthTable(base);
      const total = table[table.length - 1];
      const gap = open ? 8 : 4;
      const offsets = wireOffsets(l.wires.length, gap);
      const width = (l.wires.length - 1) * gap;
      const ties = open
        ? []
        : tieLengths(total).map((s) => {
            const t = tAtLength(table, s);
            const d = tangentAt(base, t);
            return { p: pointAt(base, t), deg: (Math.atan2(d.y, d.x) * 180) / Math.PI - 90, len: width + 4 };
          });
      const mid = pointAt(base, tAtLength(table, total / 2));
      const label = bundleLabel(l);
      return { l, key, open, base, offsets, ties, mid, label, name: LINK.name(label, nameOf(l.from), nameOf(l.to), l.addr) };
    }),
  );

  function bundleLabel(l: Link): string {
    return LINK.bundle(l.kind, l.wires.length, l.wires.filter((w) => w.shared).length);
  }

  /** Focus stops in screen order: nodes, then link chips. */
  const stops = $derived([
    ...(config?.nodes ?? []).map((n) => ({ key: `node:${n.id}`, at: { x: pos[n.id].x + NODE_W / 2, y: pos[n.id].y + 40 } })),
    ...drawn.map((d) => ({ key: d.key, at: d.mid })),
  ]);

  const DIRS = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' } as const;

  function onKey(e: KeyboardEvent) {
    const dir = DIRS[e.key as keyof typeof DIRS];
    if (!dir) return;
    const from = stops.find((s) => s.key === focusKey);
    if (!from) return;
    const i = neighbour(from.at, stops.map((s) => s.at), dir);
    if (i < 0) return;
    e.preventDefault();
    focusKey = stops[i].key;
    canvas.querySelector<HTMLElement>(`[data-key="${focusKey}"]`)?.focus();
  }

  function pick(key: string) {
    focusKey = key;
    onselect(key);
  }

  /** Four sine periods across twice the node's width: sliding it by half loops without a seam. */
  const WAVE = 'M0 7Q25 0 50 7T100 7T150 7T200 7T250 7T300 7T350 7T400 7V14H0Z';

  function layerPath(top: number, bottom: number): string {
    return `M0 ${top}Q${W / 2} ${top + 2 * ARC_SAG} ${W} ${top}V${bottom}H0Z`;
  }

  type Mark = Chip | 'skeleton' | null;

  function markOf(n: Node): Mark {
    if (n.notYet) {
      const why = NODE.notYetWhy[n.notYet] ?? '';
      return { chip: NODE.notYet, spoken: NODE.notYetName(why), silent: false, note: why };
    }
    if (chips[n.id]) return chips[n.id];
    if (!connected) return null;
    const l = live[n.id];
    if (!l || l.responding === null) return 'skeleton';
    if (!l.responding) return { chip: NODE.silent, spoken: NODE.silentName, silent: true };
    if (l.hello?.trial) return { chip: NODE.trial, spoken: NODE.trialName, silent: false };
    return { chip: NODE.responding, spoken: NODE.respondingName, silent: false };
  }

  function versionOf(n: Node): string {
    const h = connected ? live[n.id]?.hello : null;
    return h ? versionText(h.activeVersion) : '';
  }

  /** Product names wrap after hyphens and after the MCU's name in them ("Waveshare RP2040" / "-Touch-…"). */
  function breaks(product: string, mcu: string): string[] {
    const at = mcu ? product.indexOf(mcu) : -1;
    const cut = at < 0 ? [product] : [product.slice(0, at + mcu.length), product.slice(at + mcu.length)];
    return cut.filter(Boolean).flatMap((p) => p.split(/(?<=-)/));
  }

  /** A 親機's own connection line: its uplink as the 構成 writes it, or the link from the node above it. */
  function hubVia(n: Node): string {
    if (!n.uplink) return n.via || NODE.usb;
    return NODE.uplinkVia(KIND_LABEL[n.uplink.kind as LinkKind] ?? n.uplink.kind, n.uplink.host);
  }

  /** 親機 nobody links to, each with its L0 (what drives it) above it. */
  const tops = $derived((config?.nodes ?? []).filter((n) => n.role === 'hub' && depthOf(n) === 0));
  /** The 親機 this page is connected to: the first one the 構成 names (the one updating reaches). */
  const firstHub = $derived(config?.nodes.find((n) => n.role === 'hub')?.id);

  function l0Of(n: Node): string {
    if (connected && n.id === firstHub) return CANVAS.pc;
    return n.uplink ? (CANVAS.pcBy[n.uplink.host] ?? CANVAS.pc) : CANVAS.pc;
  }

  function spokenName(n: Node, m: Mark, version: string): string {
    return NODE.name({
      name: n.name,
      product: n.product,
      mcu: n.mcu,
      role: n.role === 'hub' ? NODE.roleHub : NODE.roleNode,
      via: n.role === 'hub' ? hubVia(n) : NODE.viaName(n.via),
      state: m && m !== 'skeleton' ? m.spoken : '',
      version,
    });
  }
</script>

<div class="box" bind:contentRect={box}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="canvas" bind:this={canvas} style:width="{W}px" style:height="{H}px" style:zoom={zoom === 1 ? undefined : zoom} onkeydown={onKey}>
    <svg class="links" width={W} height={H} viewBox="0 0 {W} {H}" aria-hidden="true">
      <path d="M0 0H{W}V{H}H0Z" fill="var(--c-layer-0)" />
      <path d={layerPath(L1_TOP, H)} fill="var(--c-layer-1)" />
      <path d={layerPath(L2_TOP, H)} fill="var(--c-layer-2)" />

      {#if config}
        {#each tops as h (h.id)}
          {@const x = pos[h.id].x + NODE_W / 2}
          <line x1={x} y1={PC.y + PC.h} x2={x} y2={hubY} class="pc-link" class:dashed={!connected || h.id !== firstHub} class:ink={sending && h.id === firstHub} />
          <circle cx={x} cy={PC.y + PC.h} r="5" class="port" />
          <circle cx={x} cy={hubY} r="5" class="port" />
        {/each}
      {:else}
        <circle cx={W / 2} cy={PC.y + PC.h} r="5" class="port" />
      {/if}

      {#each drawn as d (d.key)}
        <g class="bundle" class:open={d.open}>
          {#each d.l.wires as w, i (w.signal)}
            <path d={pathD(shifted(d.base, d.offsets[i]))} class="wire" class:shared={w.shared} />
          {/each}
          {#each d.ties as tie, i (i)}
            <rect class="tie" x={-tie.len / 2} y={-3} width={tie.len} height={6} rx="3" transform="translate({tie.p.x} {tie.p.y}) rotate({tie.deg})" />
          {/each}
        </g>
        {#each [d.base[0], d.base[3]] as p, i (i)}
          <circle cx={p.x} cy={p.y} r="6" class="port" />
          {#if d.l.kind === 'I2C'}<circle cx={p.x} cy={p.y} r="3" class="port" />{/if}
        {/each}
      {/each}
    </svg>

    <span class="layer-label" style:top="16px">{CANVAS.layerL0}</span>
    <span class="layer-label" style:top="{L1_TOP + 16}px">{CANVAS.layerL1}</span>
    <span class="layer-label" style:top="{L2_TOP + 16}px">{CANVAS.layerL2}</span>

    {#if dot}
      <div class="dot" style:top="{L1_TOP + 48}px" role="img" aria-label={dot.name}>
        <span class="dot-label">{dot.label}</span>
        <span class="dot-num">{dot.value}</span>{#if dot.of !== undefined}<span class="dot-of">/{dot.of}</span>{/if}{#if dot.unit}<span class="dot-of">{dot.unit}</span>{/if}
        {#if dot.sub}<span class="dot-sub"
            ><span class="dot-label">{dot.sub[0]}</span> {dot.sub[1]}{#each dot.more ?? [] as [label, value] (label)}&emsp;<span class="dot-label">{label}</span> {value}{/each}</span
          >{/if}
      </div>
    {/if}

    {#if config}
      {#each tops as h (h.id)}
        <div class="pc" style:left="{pos[h.id].x + NODE_W / 2 - PC.w / 2}px" style:top="{PC.y}px">{l0Of(h)}</div>
      {/each}
    {:else}
      <div class="pc" style:left="{W / 2 - PC.w / 2}px" style:top="{PC.y}px">{CANVAS.pc}</div>
    {/if}

    {#if !config}
      <div class="receptacle" style:left="{hubX}px" style:top="{hubY}px" style:height="{EMPTY_H}px">{CANVAS.hubSlot}</div>
      {#each [0, 1] as i (i)}
        <div class="receptacle" style:left="{childX(i)}px" style:top="{childY}px" style:height="{EMPTY_H}px">{CANVAS.nodeSlot}</div>
      {/each}
      <p class="empty" style:top="{L1_TOP + 96}px" style:width="{hubX - 48}px">{CANVAS.empty}</p>
    {/if}

    {#each config?.nodes ?? [] as n (n.id)}
      {@const key = `node:${n.id}`}
      {@const m = markOf(n)}
      {@const v = versionOf(n)}
      {@const level = fill[n.id]}
      <button
        class="node"
        style:left="{pos[n.id].x}px"
        style:top="{pos[n.id].y}px"
        data-key={key}
        tabindex={focusKey === key ? 0 : -1}
        aria-pressed={selected === key}
        aria-label={spokenName(n, m, v)}
        bind:offsetHeight={heights[n.id]}
        onclick={() => pick(key)}
      >
        {#if level !== undefined}
          <!-- The water rises with the node's progress; two wave layers drift at different speeds for depth. -->
          <span class="water" aria-hidden="true">
            <span class="surface" style:transform="translateY({100 - Math.min(100, Math.max(0, level))}%)">
              <svg class="wave back" viewBox="0 0 400 14" preserveAspectRatio="none"><path d={WAVE} /></svg>
              <svg class="wave front" viewBox="0 0 400 14" preserveAspectRatio="none"><path d={WAVE} /></svg>
            </span>
          </span>
        {/if}
        <span class="node-band">
          <span>[{n.no}] {n.name}</span>
          {#if v}<span class="version" translate="no">{v}</span>{:else if m === 'skeleton'}<span class="skeleton version-sk"></span>{/if}
        </span>
        <span class="node-body">
          <span class="product" translate="no">{#each breaks(n.product, n.mcu) as part, i (i)}{#if i}<wbr />{/if}{part}{/each}</span>
          <span class="mono"><span class="label">{NODE.mcu}</span> <span translate="no">{n.mcu}</span></span>
          {#if n.role === 'hub'}
            <span class="mono">{hubVia(n)}</span>
          {:else}
            <span class="mono" translate="no">{n.via}</span>
            <span class="mono"><span class="label">{NODE.reset}</span> <span translate="no">{n.reset ?? NODE.noWire}</span></span>
            <span class="mono"><span class="label">{NODE.boot}</span> <span translate="no">{n.boot ?? NODE.noWire}</span></span>
          {/if}
          {#if m === 'skeleton'}
            <span class="skeleton chip-sk"></span>
          {:else if m}
            <span class="chip" class:silent={m.silent}>{m.chip}</span>
            {#if m.note}<span class="note">{m.note}</span>{/if}
          {/if}
        </span>
      </button>
    {/each}

    {#each drawn as d (d.key)}
      <button
        class="chip link-chip"
        class:selected={d.open}
        style:left="{d.mid.x}px"
        style:top="{d.mid.y}px"
        data-key={d.key}
        tabindex={focusKey === d.key ? 0 : -1}
        aria-pressed={d.open}
        aria-label={d.name}
        onclick={() => pick(d.key)}
      >
        {d.label}
      </button>
    {/each}
  </div>
</div>

<style>
  .box {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: auto;
  }
  .canvas {
    position: relative;
    user-select: none;
  }
  .links {
    position: absolute;
    inset: 0;
  }
  .pc-link {
    stroke: var(--c-line);
    stroke-width: 2;
  }
  .pc-link.dashed {
    stroke-dasharray: 4 3;
  }
  .pc-link.ink {
    stroke: var(--c-signal);
  }
  .wire {
    fill: none;
    stroke: var(--c-line);
    stroke-width: 1;
  }
  .wire.shared {
    stroke-dasharray: 4 3;
  }
  .bundle.open .wire {
    stroke: var(--c-paper);
    stroke-width: 2;
  }
  .bundle.open .wire.shared {
    stroke-dasharray: 6 4;
  }
  .tie {
    fill: var(--c-line);
  }
  .port {
    fill: var(--c-ink);
    stroke: var(--c-line);
    stroke-width: 2;
  }
  .layer-label {
    position: absolute;
    left: var(--s-5);
    font: 500 var(--t-caption-size) / var(--t-caption-lh) var(--f-ja);
    letter-spacing: 0.04em;
    color: var(--c-muted);
  }
  .dot {
    position: absolute;
    left: var(--s-5);
    display: grid;
    grid-template-columns: auto auto;
    align-items: baseline;
    color: var(--c-paper);
  }
  .dot-label {
    grid-column: 1 / -1;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    color: var(--c-muted);
  }
  .dot-num {
    font: 700 var(--t-display-size) / var(--t-display-lh) var(--f-dot);
    font-variant-numeric: tabular-nums;
  }
  .dot-of {
    font: 700 var(--t-version-size) / 1 var(--f-dot);
  }
  .dot-sub {
    grid-column: 1 / -1;
    margin-top: var(--s-3);
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .dot-sub .dot-label {
    display: inline;
  }
  .pc {
    position: absolute;
    width: var(--w-node);
    height: var(--h-secondary);
    display: grid;
    place-items: center;
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    font: 400 var(--t-small-size) / 1 var(--f-ja);
  }
  .receptacle {
    position: absolute;
    width: var(--w-node);
    display: grid;
    place-items: center;
    border: 2px dashed var(--c-line);
    border-radius: var(--r-l);
    color: var(--c-muted);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .empty {
    position: absolute;
    left: var(--s-5);
    max-width: 40em;
    margin: 0;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    color: var(--c-paper);
    text-wrap: pretty;
  }
  .node {
    position: absolute;
    z-index: var(--z-node);
    width: var(--w-node);
    display: grid;
    padding: 2px; /* keeps 3 px of border room: 1 px line + 2 px */
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-paper);
    text-align: left;
    font: inherit;
    cursor: pointer;
    overflow: hidden;
    outline: 3px solid transparent;
    outline-offset: -3px;
  }
  .node[aria-pressed='true'] {
    background: var(--c-layer-2);
    border-color: var(--c-paper);
    box-shadow: inset 4px 0 0 var(--c-paper);
  }
  .node:focus-visible {
    border: 3px solid var(--c-paper);
    padding: 0;
  }
  @media (hover: hover) and (pointer: fine) {
    .node:not([aria-pressed='true']):hover {
      background: var(--c-hover);
    }
  }
  /* 32 px band; the one line of name and version sits centred, on one baseline. */
  .node-band,
  .node-body {
    position: relative;
  }
  /*
   * Water fill (SC-O5): one layer moved with transform only. Ink-red tints of
   * the one signal colour keep paper text near 12:1 and the muted labels above 4.5:1 (§9.1 never puts paper
   * on full signal red). The level follows each progress value in 250 ms
   * linear (§10 Interpolation); the waves drift on the compositor.
   */
  .water {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: inherit;
    pointer-events: none;
    --wave-front: color-mix(in srgb, var(--c-signal) 30%, var(--c-ink));
    --wave-back: color-mix(in srgb, var(--c-signal) 18%, var(--c-ink));
  }
  .surface {
    position: absolute;
    inset: 0;
    background: var(--wave-front);
    transition: transform 250ms linear;
  }
  .wave {
    position: absolute;
    left: 0;
    top: -14px;
    width: 200%;
    height: 14px;
  }
  .wave.back {
    top: -18px;
    height: 18px;
    fill: var(--wave-back);
    animation: drift 7s linear infinite reverse;
  }
  .wave.front {
    fill: var(--wave-front);
    animation: drift 4.2s linear infinite;
  }
  @keyframes drift {
    from {
      translate: 0 0;
    }
    to {
      translate: -50% 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    /* A static fill: no waves, the level jumps to each value. */
    .wave {
      display: none;
    }
    .surface {
      transition: none;
    }
  }
  .note {
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    font-variant-numeric: tabular-nums;
  }
  .node-band {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: baseline;
    align-content: center;
    height: 2rem;
    padding: 0 var(--s-3);
    border-bottom: 1px solid var(--c-line);
    font: 700 var(--t-small-size) / 1 var(--f-ja);
  }
  .version {
    font: 400 var(--t-version-small-size) / 1 var(--f-version);
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
  }
  .node-body {
    display: grid;
    gap: var(--s-2);
    justify-items: start;
    padding: var(--s-3) var(--s-4);
  }
  .product {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
  }
  .mono {
    font: 500 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
    letter-spacing: 0.02em;
  }
  .label {
    color: var(--c-muted);
  }
  .version-sk {
    width: var(--s-8);
    height: var(--s-4);
  }
  .chip-sk {
    width: var(--s-9);
    height: var(--h-chip);
    border-radius: var(--r-full);
  }
  .chip.silent {
    border-color: var(--c-signal);
    background: var(--c-signal);
    color: var(--c-ink);
  }
  .link-chip {
    position: absolute;
    z-index: var(--z-chip);
    transform: translate(-50%, -50%);
    cursor: pointer;
    outline: 3px solid transparent;
  }
  @media (hover: hover) and (pointer: fine) {
    .link-chip:not(.selected):hover {
      background: var(--c-hover);
    }
  }
  .link-chip.selected {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .link-chip:focus-visible {
    outline-color: var(--c-paper);
  }
</style>
