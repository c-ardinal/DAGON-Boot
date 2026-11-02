<script lang="ts">
  import { NODE } from '../app/copy.ts';
  import {
    lengthTable,
    neighbour,
    pathD,
    pointAt,
    shifted,
    tAtLength,
    tangentAt,
    tieLengths,
    verticalLink,
    wireOffsets,
    type Pt,
  } from './geometry.ts';
  import { LINKS, NODES, bundleLabel, type SampleLink } from './sample.ts';

  const W = 800;
  const H = 800;
  const NODE_W = 240;
  const L1_TOP = 120;
  const L2_TOP = 440;
  const ARC_SAG = 8;
  const PC = { x: 280, y: 24, w: 240, h: 44 };

  let heights = $state<Record<string, number>>({});
  let selected = $state<string | null>(null);
  let focusKey = $state('node:hub');
  let canvas: HTMLDivElement;

  const byId = Object.fromEntries(NODES.map((n) => [n.id, n]));
  const childLinks = (id: string) => LINKS.filter((l) => l.from === id);

  function bottomPort(l: SampleLink): Pt {
    const n = byId[l.from];
    const siblings = childLinks(l.from);
    const slot = (siblings.indexOf(l) + 1) / (siblings.length + 1);
    return { x: n.x + NODE_W * slot, y: n.y + (heights[n.id] ?? 200) };
  }

  function topPort(id: string): Pt {
    const n = byId[id];
    return { x: n.x + NODE_W / 2, y: n.y };
  }

  const drawn = $derived(
    LINKS.map((l) => {
      const key = `link:${l.to}`;
      const open = selected === key;
      const base = verticalLink(bottomPort(l), topPort(l.to));
      const table = lengthTable(base);
      const total = table[table.length - 1];
      const gap = open ? 8 : 4;
      const offsets = wireOffsets(l.wires.length, gap);
      const width = (l.wires.length - 1) * gap;
      const ties = open
        ? []
        : tieLengths(total).map((s) => {
            const t = tAtLength(table, s);
            const p = pointAt(base, t);
            const d = tangentAt(base, t);
            return { p, deg: (Math.atan2(d.y, d.x) * 180) / Math.PI - 90, len: width + 4 };
          });
      const mid = pointAt(base, tAtLength(table, total / 2));
      return { l, key, open, base, offsets, ties, mid };
    }),
  );

  /** Focus stops in screen order: nodes, then link chips. */
  const stops = $derived([
    ...NODES.map((n) => ({ key: `node:${n.id}`, at: { x: n.x + NODE_W / 2, y: n.y + 40 } })),
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
    selected = selected === key ? null : key;
  }

  function layerPath(top: number, bottom: number): string {
    return `M0 ${top}Q${W / 2} ${top + 2 * ARC_SAG} ${W} ${top}V${bottom}H0Z`;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="canvas" bind:this={canvas} style:width="{W}px" style:height="{H}px" onkeydown={onKey}>
  <svg class="links" width={W} height={H} viewBox="0 0 {W} {H}" aria-hidden="true">
    <path d="M0 0H{W}V{H}H0Z" fill="var(--c-layer-0)" />
    <path d={layerPath(L1_TOP, H)} fill="var(--c-layer-1)" />
    <path d={layerPath(L2_TOP, H)} fill="var(--c-layer-2)" />

    <line x1={PC.x + PC.w / 2} y1={PC.y + PC.h} x2={byId.hub.x + NODE_W / 2} y2={byId.hub.y} class="pc-link" />

    {#each drawn as d (d.key)}
      <g class="bundle" class:open={d.open}>
        {#each d.l.wires as w, i (w.signal)}
          <path d={pathD(shifted(d.base, d.offsets[i]))} class="wire" class:shared={w.shared} />
        {/each}
        {#each d.ties as tie, i (i)}
          <rect
            class="tie"
            x={-tie.len / 2}
            y={-3}
            width={tie.len}
            height={6}
            rx="3"
            transform="translate({tie.p.x} {tie.p.y}) rotate({tie.deg})"
          />
        {/each}
      </g>
      <circle cx={d.base[0].x} cy={d.base[0].y} r="6" class="port" />
      {#if d.l.kind === 'I2C'}<circle cx={d.base[0].x} cy={d.base[0].y} r="3" class="port" />{/if}
      <circle cx={d.base[3].x} cy={d.base[3].y} r="6" class="port" />
      {#if d.l.kind === 'I2C'}<circle cx={d.base[3].x} cy={d.base[3].y} r="3" class="port" />{/if}
    {/each}
  </svg>

  <span class="layer-label" style:top="16px">{NODE.layerL0}</span>
  <span class="layer-label" style:top="{L1_TOP + 16}px">{NODE.layerL1}</span>
  <span class="layer-label" style:top="{L2_TOP + 16}px">{NODE.layerL2}</span>

  <div class="dot" style:top="{L1_TOP + 48}px" role="img" aria-label={NODE.dotRespondingName(NODES.length, NODES.length)}>
    <span class="dot-label">{NODE.dotResponding}</span>
    <span class="dot-num">{NODES.length}</span><span class="dot-of">/{NODES.length}</span>
  </div>

  <div class="pc" style:left="{PC.x}px" style:top="{PC.y}px">{NODE.pc}</div>

  {#each NODES as n (n.id)}
    {@const key = `node:${n.id}`}
    <button
      class="node"
      style:left="{n.x}px"
      style:top="{n.y}px"
      data-key={key}
      tabindex={focusKey === key ? 0 : -1}
      aria-pressed={selected === key}
      bind:offsetHeight={heights[n.id]}
      onclick={() => pick(key)}
    >
      <span class="node-band">
        <span>[{n.no}] {n.name}　{n.role}</span>
        <span class="version" translate="no">{n.version}</span>
      </span>
      <span class="node-body">
        <span class="product" translate="no">{n.product}</span>
        <span class="mono"><span class="label">MCU</span> <span translate="no">{n.mcu}</span></span>
        <span class="mono" translate="no">{n.via}</span>
        <span class="chip">{NODE.responding}</span>
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
      onclick={() => pick(d.key)}
    >
      {bundleLabel(d.l)}
    </button>
  {/each}
</div>

<style>
  .canvas {
    position: relative;
    overflow: hidden;
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
    font: 500 var(--t-caption-size) / var(--t-caption-lh) var(--f-ja);
    color: var(--c-muted);
  }
  .dot-num {
    font: 700 var(--t-display-size) / var(--t-display-lh) var(--f-dot);
    font-variant-numeric: tabular-nums;
  }
  .dot-of {
    font: 700 var(--t-version-size) / 1 var(--f-dot);
  }
  .pc {
    position: absolute;
    width: 240px;
    height: 44px;
    display: grid;
    place-items: center;
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    font: 400 var(--t-small-size) / 1 var(--f-ja);
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
  }
  .node[aria-pressed='true'] {
    background: var(--c-layer-2);
    border-color: var(--c-paper);
    box-shadow: inset 4px 0 0 var(--c-paper);
  }
  .node:focus-visible {
    border: 3px solid var(--c-paper);
    padding: 0;
    outline: 3px solid transparent;
    outline-offset: -3px;
  }
  @media (hover: hover) and (pointer: fine) {
    .node:not([aria-pressed='true']):hover {
      background: var(--c-hover);
    }
  }
  .node-band {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: var(--s-2) var(--s-3);
    border-bottom: 1px solid var(--c-line);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .version {
    font: 400 var(--t-version-small-size) / var(--t-version-small-lh) var(--f-version);
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
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .mono {
    font: 500 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
    letter-spacing: 0.02em;
  }
  .label {
    color: var(--c-muted);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: var(--h-chip);
    padding: 0 var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 700 var(--t-caption-size) / 1 var(--f-ja);
    letter-spacing: 0.04em;
    white-space: nowrap;
  }
  .link-chip {
    position: absolute;
    z-index: var(--z-chip);
    transform: translate(-50%, -50%);
    cursor: pointer;
    outline: 3px solid transparent;
  }
  .link-chip.selected {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .link-chip:focus-visible {
    outline-color: var(--c-paper);
  }
</style>
