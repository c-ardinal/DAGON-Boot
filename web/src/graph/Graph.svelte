<script lang="ts" module>
  /** A node chip set by the page (update progress) instead of the watch result. */
  export interface Chip {
    chip: string;
    spoken: string;
    silent: boolean;
    /** A label-value line under the chip ("推定 約1分", "自動再接続 あと30秒"). */
    note?: string;
  }

  /** Where a part dropped on the canvas goes: under `parent`, or on its own (a new tree). */
  export interface DropTarget {
    parent: string | null;
  }

  /** What the canvas shows: only the user's zoom, pan and fit change it; nothing around the canvas does. */
  export interface View {
    zoom: number;
    x: number;
    y: number;
    /** The first view of this project has been made. */
    set: boolean;
  }

  export const newView = (): View => ({ zoom: 1, x: 0, y: 0, set: false });
  /** From this many ノード the cards are compact whatever the zoom (name, version and state only). */
  export const COMPACT_FROM = 9;
</script>

<script lang="ts">
  import { ANNOUNCE, CANVAS, LINK } from '../app/copy.ts';
  import { placeByClick, stepOf } from '../app/move.ts';
  import { easeOut, fade, draw, moves, tokenMs, tokenNumber } from '../app/motion.ts';
  import { lightsStill, nodeState, type NodeState } from '../app/nodeState.ts';
  import NodeCard from './NodeCard.svelte';
  import { LinkKind } from '../app/define/model.ts';
  import type { Live } from '../app/writeProgress.ts';
  import { hubIn, isHub, type Config, type Node } from '../app/sample.ts';
  import { bundleOf } from './bundles.ts';
  import { CableField, FrameLoop, restPoints } from './cable.ts';
  import { neighbour, pathD, verticalLink, type Bezier, type Pt } from './geometry.ts';
  import { boundsOf, clampZoom, fitView, SIDE, ZOOM_LIMITS } from './fit.ts';
  import { treeLayout } from './layout.ts';
  import { untrack } from 'svelte';

  interface Props {
    config: Config | null;
    live: Record<string, Live>;
    /** The coordinator is connected: the PC link is solid and nodes carry live marks. */
    connected: boolean;
    selected: string | null;
    onselect: (key: string) => void;
    chips?: Record<string, Chip>;
    /** The canvas's zoom and pan: the user's. */
    view?: View;
    /** Where the user put each card; the others stand in the tree layout. */
    positions?: Record<string, Pt>;
    /** A card was moved (a drag, Alt + arrow, or a click after `startMove`). */
    onmoved?: () => void;
    /** A sentence for screen readers: where a moved card went. */
    onannounce?: (text: string) => void;
    /** The free part of the window (client px) a fit puts the graph in; read only when fitting. */
    freeArea?: () => DOMRect | undefined;
    /** The whole area, tried when `freeArea` alone would make the fit compact. */
    wholeArea?: () => DOMRect | undefined;
    /** Nodes being written: how full their water is, 0–100. */
    fill?: Record<string, number>;
    /** Nodes being built now: their frame carries the edge light. */
    building?: readonly string[];
    /** Port drag (定義 only): why `from` cannot take `to` under it, or null when it can. */
    canLink?: (from: string, to: string) => string | null;
    /** A port drag ended on a node that can be linked. */
    onlink?: (from: string, to: string) => void;
    /** A refused port drag: the reason (a failure, shown in red by the page). */
    onrefuse?: (why: string, x: number, y: number) => void;
    /** The id of the node a dragged part would go under (highlighted). */
    dropOn?: string | null;
    /** 書込: each writable node's card check (true: it is written next); none in 準備. */
    checks?: Record<string, boolean>;
    /** A card check pressed. */
    oncheck?: (id: string) => void;
    /** The card checks stay as they are (a write runs). */
    checksLocked?: boolean;
    /** The node confirmed its new version: its water rises to the top as it goes. */
    confirmedOf?: (id: string) => boolean;
  }

  let {
    config,
    live,
    connected,
    selected,
    onselect,
    chips = {},
    view = $bindable(newView()),
    positions = $bindable({}),
    onmoved,
    onannounce,
    freeArea,
    wholeArea,
    fill = {},
    building = [],
    canLink,
    onlink,
    onrefuse,
    dropOn = null,
    checks = {},
    oncheck,
    checksLocked = false,
    confirmedOf = () => false,
  }: Props = $props();

  /** Names on the cards never get smaller than this on screen (px): past it the cards turn compact and hold the name at this size. */
  const NAME_MIN_PX = 13;
  /** A name's size on a card at 100 % (--t-small inside the canvas). */
  const NAME_PX = 14;
  /** Card heights before they are measured (px): what each class measures with the sample (a compact card keeps its header and its chip row). */
  const EST_H = { normal: 140, compact: 78 };
  /** Room above a top coordinator for its uplink card (px). */
  const UPLINK_ROOM = 84;
  /** A press moves this far before it is a drag, not a click (px). */
  const DRAG_PX = 4;

  let heights = $state<Record<string, number>>({});
  let focusKey = $state('node:hub');
  let canvas: HTMLDivElement;
  let boxEl = $state<HTMLDivElement>();

  /** Card geometry of each class (normal, compact): width and gaps, in canvas px; the zoom alone scales them. The rows stand far apart: the links' length is what lets a cable sweep and read. */
  const sizes = (compact: boolean) => (compact ? { w: 208, gapX: 20, gapY: 80 } : { w: 260, gapX: 80, gapY: 128 });
  const nodesOf = $derived((config?.nodes ?? []).map((n) => ({ id: n.id, hub: isHub(n) })));
  const compact = $derived(nodesOf.length >= COMPACT_FROM || view.zoom * NAME_PX < NAME_MIN_PX);
  const S = $derived(sizes(compact));
  const NODE_W = $derived(S.w);
  const hOf = (id: string) => heights[id] ?? (compact ? EST_H.compact : EST_H.normal);
  const tree = $derived(treeLayout(nodesOf, config?.links ?? [], { ...S, h: hOf }));
  /** Where each card belongs: the user's place, or the tree's, centred on x = 0 so a growing tree spreads both ways. */
  const pos = $derived(Object.fromEntries(Object.entries(tree.at).map(([id, p]): [string, Pt] => [id, positions[id] ?? { x: p.x - tree.width / 2, y: p.y }])));

  /** Where each card is drawn now: on its place at rest, on the way while the layout rearranges. */
  let shown = $state.raw<Record<string, Pt>>({});
  const at = (id: string): Pt => shown[id] ?? pos[id];
  let dragging = $state<string | null>(null);
  /** The graph takes its first measure after it appears: cards go straight to their places then. */
  let arriving = true;
  $effect(() => {
    const t = setTimeout(() => (arriving = false), tokenMs('--dur-line'));
    return () => clearTimeout(t);
  });

  /** Cards on the way to a new layout, and the cables behind them: one loop, running only while something moves. */
  let glide: { from: Record<string, Pt>; to: Record<string, Pt>; ms: number } | null = null;
  const cables = new CableField();
  let shapes = $state.raw<Record<string, Pt[]>>({});
  const loop = new FrameLoop((ms) => {
    if (glide) moveCards(ms);
    if (moves()) cables.step(ms);
    else cables.settle();
    shapes = cables.shapes();
    return glide !== null || !cables.atRest;
  });
  $effect(() => () => loop.stop());

  function moveCards(ms: number) {
    if (!glide) return;
    glide.ms += ms;
    const dur = tokenMs('--dur-line');
    const t = dur > 0 ? Math.min(1, glide.ms / dur) : 1;
    const e = easeOut()(t);
    const { from, to } = glide;
    shown = Object.fromEntries(Object.entries(to).map(([id, q]): [string, Pt] => {
      const p = from[id] ?? q;
      return [id, { x: p.x + (q.x - p.x) * e, y: p.y + (q.y - p.y) * e }];
    }));
    if (t === 1) glide = null;
  }

  /** A card an Alt + arrow step moved: there at once, as a dragged one is, so held keys do not lag. */
  let stepped: string | null = null;

  // A new layout: cards that have a place glide to the new one; a new, dragged or stepped card is there at once.
  $effect(() => {
    const to = pos;
    untrack(() => {
      const now = Object.fromEntries(Object.entries(to).map(([id, q]): [string, Pt] => [id, arriving || id === dragging || id === stepped || !shown[id] ? q : shown[id]]));
      stepped = null;
      shown = now;
      if (Object.entries(to).every(([id, q]) => now[id].x === q.x && now[id].y === q.y)) {
        glide = null;
        return;
      }
      glide = { from: now, to, ms: 0 };
      loop.kick();
    });
  });

  /** Each link's route from the cards as they are drawn now: where its cable rests. */
  const routes = $derived(
    Object.fromEntries(
      (config?.links ?? []).map((l): [string, Bezier] => {
        const from = at(l.from);
        const to = at(l.to);
        const siblings = (config?.links ?? []).filter((x) => x.from === l.from);
        const slot = (siblings.indexOf(l) + 1) / (siblings.length + 1);
        return [`link:${l.to}`, verticalLink({ x: from.x + NODE_W * slot, y: from.y + hOf(l.from) }, { x: to.x + NODE_W / 2, y: to.y })];
      }),
    ),
  );
  $effect(() => {
    const r = routes;
    untrack(() => {
      cables.route(r);
      if (!moves()) cables.settle();
      shapes = cables.shapes();
      if (!cables.atRest) loop.kick();
    });
  });

  /** Everything placed, in canvas px, as the cards stand (`c`: as if compact or not, from the tree's own layout for that class). */
  function boundsAt(c: boolean) {
    const s = sizes(c);
    const est = c ? EST_H.compact : EST_H.normal;
    const h = (id: string) => (c === compact ? hOf(id) : est);
    const t = treeLayout(nodesOf, config?.links ?? [], { ...s, h });
    return boundsOf(
      (config?.nodes ?? []).map((n) => {
        const p = positions[n.id] ?? { x: t.at[n.id].x - t.width / 2, y: t.at[n.id].y };
        return { ...p, w: s.w, h: h(n.id), top: tops.some((x) => x.id === n.id) ? UPLINK_ROOM : 0 };
      }),
    );
  }

  /** The canvas moves smoothly for a pressed fit or zoom; never for a drag or the wheel, which follow the hand. */
  let gliding = $state(false);

  /** The zoom under which the cards turn compact. */
  const COMPACT_ZOOM = NAME_MIN_PX / NAME_PX;

  /** The explicit fit: the whole graph in the free part of the window, at most `cap`; the same answer each time, whatever the cards were. */
  export function fit(cap: number = firstZoom(), smooth = true) {
    if (!boxEl || !config?.nodes.length) return;
    const box = boxEl.getBoundingClientRect();
    const fitted = fitView({
      normal: boundsAt(false),
      compact: () => boundsAt(true),
      box,
      free: freeArea?.() ?? box,
      whole: wholeArea?.(),
      cap,
      compactZoom: COMPACT_ZOOM,
      many: nodesOf.length >= COMPACT_FROM,
    });
    gliding = smooth;
    view = { ...fitted, set: true };
  }

  /** Zoom keeping the canvas point under (cx, cy) in place; by default the middle of the box. */
  export function zoomBy(factor: number, cx?: number, cy?: number, smooth = true) {
    if (!boxEl) return;
    const r = boxEl.getBoundingClientRect();
    const zoom = clampZoom(view.zoom * factor);
    const px = (cx ?? r.left + r.width / 2) - r.left;
    const py = (cy ?? r.top + r.height / 2) - r.top;
    gliding = smooth;
    view = { zoom, x: px - ((px - view.x) / view.zoom) * zoom, y: py - ((py - view.y) / view.zoom) * zoom, set: true };
  }

  /** The default zoom of the first view, the largest the graph then takes (--zoom-default). */
  export const firstZoom = () => tokenNumber('--zoom-default');

  // The first view of a project, once: afterwards only the user changes it.
  $effect(() => {
    if (view.set || !boxEl || !nodesOf.length || nodesOf.some((n) => !heights[n.id])) return;
    const t = setTimeout(() => untrack(() => !view.set && fit(firstZoom(), false)));
    return () => clearTimeout(t);
  });

  function panStart(e: PointerEvent) {
    if ((e.button !== 0 && e.button !== 1) || (e.target as Element).closest('button, .port')) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const start = { px: e.clientX, py: e.clientY, x: view.x, y: view.y };
    gliding = false;
    const move = (ev: PointerEvent) => (view = { ...view, x: start.x + ev.clientX - start.px, y: start.y + ev.clientY - start.py, set: true });
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }

  // Not passive, and not Svelte's delegated handler: the wheel must not reach the browser's own page zoom.
  $effect(() => {
    const el = boxEl;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      if (e.ctrlKey) return zoomBy(e.deltaY < 0 ? ZOOM_LIMITS.step : 1 / ZOOM_LIMITS.step, e.clientX, e.clientY, false);
      gliding = false;
      view = { ...view, x: view.x - e.deltaX, y: view.y - e.deltaY, set: true };
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return () => el.removeEventListener('wheel', wheel);
  });

  /** Keyboard focus reaching a card off the box brings it in: the user's own move. */
  function reveal(e: FocusEvent) {
    const el = e.target as HTMLElement;
    if (!boxEl || !el.matches(':focus-visible')) return;
    const [c, b] = [el.getBoundingClientRect(), boxEl.getBoundingClientRect()];
    const dx = Math.max(0, b.left - c.left + SIDE) - Math.max(0, c.right - b.right + SIDE);
    const dy = Math.max(0, b.top - c.top + SIDE) - Math.max(0, c.bottom - b.bottom + SIDE);
    if (dx || dy) view = { ...view, x: view.x + dx, y: view.y + dy, set: true };
  }

  /** A press on a card: past DRAG_PX it drags the card, and the press is then no click. */
  let dragged = false;
  function cardDown(e: PointerEvent, id: string) {
    if (e.button !== 0) return;
    const [x0, y0] = [e.clientX, e.clientY];
    const from = at(id);
    const z = view.zoom;
    dragged = false;
    const move = (ev: PointerEvent) => {
      if (!dragged && Math.hypot(ev.clientX - x0, ev.clientY - y0) <= DRAG_PX) return;
      dragged = true;
      dragging = id;
      positions = { ...positions, [id]: { x: from.x + (ev.clientX - x0) / z, y: from.y + (ev.clientY - y0) / z } };
    };
    const up = () => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      removeEventListener('pointercancel', up);
      if (dragging) onmoved?.();
      dragging = null;
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
    addEventListener('pointercancel', up);
  }

  const nameOf = (id: string) => config?.nodes.find((n) => n.id === id)?.name ?? id;

  const drawn = $derived(
    (config?.links ?? []).map((l) => {
      const key = `link:${l.to}`;
      return bundleOf(l, config, routes[key], shapes[key] ?? restPoints(routes[key]), selected === key);
    }),
  );

  /** Focus stops in screen order: nodes, then link chips. */
  const stops = $derived([
    ...(config?.nodes ?? []).map((n) => ({ key: `node:${n.id}`, at: { x: at(n.id).x + NODE_W / 2, y: at(n.id).y + 40 } })),
    ...drawn.map((d) => ({ key: d.key, at: d.mid })),
  ]);

  const DIRS = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' } as const;

  /** The card a click places next (after the menu's move item), or null. */
  let placing = $state<string | null>(null);
  let stopPlacing: (() => void) | null = null;

  function moveTo(id: string, p: Pt) {
    positions = { ...positions, [id]: p };
    onmoved?.();
    onannounce?.(ANNOUNCE.moved(nameOf(id)));
  }

  /** The next click on the canvas puts the card's centre there; a click elsewhere or Escape leaves it. */
  export function startMove(id: string) {
    stopPlacing?.();
    placing = id;
    onannounce?.(CANVAS.moveHint(nameOf(id)));
    stopPlacing = placeByClick(
      () => boxEl?.getBoundingClientRect(),
      (cx, cy) => {
        const p = toCanvas(cx, cy);
        placing = stopPlacing = null;
        moveTo(id, { x: p.x - NODE_W / 2, y: p.y - hOf(id) / 2 });
        focusKey = `node:${id}`;
        canvas.querySelector<HTMLElement>(`[data-key="${focusKey}"]`)?.focus();
      },
      () => {
        placing = stopPlacing = null;
        onannounce?.(ANNOUNCE.moveCancelled);
      },
    );
  }
  $effect(() => () => stopPlacing?.());

  function onKey(e: KeyboardEvent) {
    const step = stepOf(e);
    if (step) {
      if (!focusKey.startsWith('node:')) return;
      e.preventDefault();
      const id = focusKey.slice(5);
      const from = pos[id];
      stepped = id;
      if (from) moveTo(id, { x: from.x + step.x, y: from.y + step.y });
      return;
    }
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
    if (dragged) return void (dragged = false);
    onselect(key);
  }

  /** Each node's state mark: from the page's chip, the water level and the watch. */
  const states = $derived(
    Object.fromEntries(
      (config?.nodes ?? []).map((n): [string, NodeState] => {
        const l = live[n.id];
        return [n.id, nodeState({ connected, responding: l?.responding, trial: !!l?.hello?.trial, chip: chips[n.id]?.chip, writing: fill[n.id] !== undefined || building.includes(n.id), unreachable: !!n.skip })];
      }),
    ),
  );
  const still = $derived(lightsStill(Object.values(states)));

  /** Coordinators nobody links to, each with its uplink (what drives it, as the 構成 writes it) above it. */
  const tops = $derived((config?.nodes ?? []).filter((n) => isHub(n) && tree.roots.includes(n.id)));
  /** The coordinator this page connects to: the one updating reaches (the cut's), else the first one the 構成 names. */
  const firstHub = $derived(
    (hubIn(config, true) ?? hubIn(config, false))?.id,
  );

  function l0Of(n: Node): string {
    if (connected && n.id === firstHub) return CANVAS.pc;
    const by = n.uplink ? (CANVAS.pcBy[n.uplink.host] ?? CANVAS.pc) : CANVAS.pc;
    return n.id === firstHub ? by : `${by}${CANVAS.notThisPc}`;
  }

  /** Client point to canvas coordinates. */
  function toCanvas(cx: number, cy: number): Pt {
    const r = canvas.getBoundingClientRect();
    return { x: (cx - r.left) / view.zoom, y: (cy - r.top) / view.zoom };
  }

  /**
   * Where a part dropped at (cx, cy) goes: under the nearest card just above
   * the point, at any depth; else on its own. Null when the point is off the canvas.
   */
  export function dropTarget(cx: number, cy: number): DropTarget | null {
    const r = boxEl?.getBoundingClientRect();
    if (!r || cx < r.left || cx > r.right || cy < r.top || cy > r.bottom) return null;
    const p = toCanvas(cx, cy);
    const near = (config?.nodes ?? [])
      .map((n) => ({ n, bottom: at(n.id).y + hOf(n.id), cx: at(n.id).x + NODE_W / 2 }))
      .filter((c) => p.y > c.bottom && p.y - c.bottom < 1.6 * (hOf(c.n.id) + S.gapY) && Math.abs(p.x - c.cx) < 1.25 * NODE_W)
      .sort((a, b) => p.y - a.bottom - (p.y - b.bottom) || Math.abs(p.x - a.cx) - Math.abs(p.x - b.cx))[0];
    return { parent: near?.n.id ?? null };
  }

  /** Port drag: from a card's lower port to another card's upper port. */
  let wire = $state<{ from: string; a: Pt; b: Pt; target: string | null; bad: boolean } | null>(null);
  const SNAP_PX = 28;
  function portDown(e: PointerEvent, from: string) {
    if (!onlink || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const a = { x: at(from).x + NODE_W / 2, y: at(from).y + hOf(from) };
    wire = { from, a, b: a, target: null, bad: false };
  }
  /** Why each node cannot take the dragged link (null: it can), while dragging. */
  const reasons = $derived(
    wire ? Object.fromEntries((config?.nodes ?? []).filter((n) => n.id !== wire!.from).map((n) => [n.id, canLink?.(wire!.from, n.id) ?? null])) : {},
  );
  function portMove(e: PointerEvent) {
    if (!wire) return;
    const p = toCanvas(e.clientX, e.clientY);
    let best = SNAP_PX / view.zoom;
    let target: string | null = null;
    let b = p;
    let over: string | null = null;
    for (const n of config?.nodes ?? []) {
      if (n.id === wire.from) continue;
      const q = { x: at(n.id).x + NODE_W / 2, y: at(n.id).y };
      const d = Math.hypot(q.x - p.x, q.y - p.y);
      const inCard = p.x > at(n.id).x && p.x < at(n.id).x + NODE_W && p.y > at(n.id).y - 10 && p.y < at(n.id).y + hOf(n.id);
      if (reasons[n.id] === null && (d < best || inCard)) {
        best = Math.min(best, d);
        target = n.id;
        b = q;
      } else if (inCard || d < SNAP_PX / view.zoom) over = n.id;
    }
    wire = { ...wire, b, target, bad: !target && over !== null };
    if (!target && over !== null && reasons[over]) onrefuse?.(reasons[over]!, e.clientX + 12, e.clientY + 12);
  }
  function portUp() {
    if (!wire) return;
    const w = wire;
    wire = null;
    if (w.target) onlink?.(w.from, w.target);
  }
</script>

<div class="graph" class:placing={placing !== null}>
  {#if placing}
    <p class="placing-hint">{CANVAS.moveHint(nameOf(placing))}</p>
  {/if}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="box" bind:this={boxEl} onpointerdown={panStart} onfocusin={reveal}>
    {#if !config}
      <p class="empty">{CANVAS.empty}</p>
    {/if}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="canvas"
      class:compact
      class:gliding
      bind:this={canvas}
      style:transform="translate({view.x}px, {view.y}px) scale({view.zoom})"
      style:--zoom={view.zoom}
      onkeydown={onKey}
      onpointermove={portMove}
      onpointerup={portUp}
      onpointercancel={() => (wire = null)}
    >
      <svg class="links" aria-hidden="true">
        {#each tops as h (h.id)}
          {@const x = at(h.id).x + NODE_W / 2}
          {@const y0 = at(h.id).y - UPLINK_ROOM + 40}
          <line x1={x} y1={y0} x2={x} y2={at(h.id).y} class="pc-link" class:dashed={!connected || h.id !== firstHub} />
          <circle cx={x} cy={y0} r="5" class="end" />
        {/each}

        {#each drawn as d (d.key)}
          <g transition:draw class="bundle" class:open={d.open} class:uns={d.skipped}>
            {#each d.strands as strand, i (i)}
              <path d={strand} class="wire" class:shared={d.l.wires[i]?.shared} />
            {/each}
            {#each d.ties as tie, i (i)}
              <rect class="tie" x={-tie.len / 2} y={-3} width={tie.len} height={6} rx="3" transform="translate({tie.p.x} {tie.p.y}) rotate({tie.deg})" />
            {/each}
          </g>
        {/each}
        {#if wire}<path d={pathD(verticalLink(wire.a, wire.b))} class="temp" class:bad={wire.bad} />{/if}
      </svg>

      {#each tops as h (h.id)}
        <div class="uplink" class:off={!connected || h.id !== firstHub} title={l0Of(h)} style:left="{at(h.id).x + NODE_W / 2}px" style:top="{at(h.id).y - UPLINK_ROOM + 12}px" style:max-width="{NODE_W + S.gapX}px">{l0Of(h)}</div>
      {/each}

      {#each config?.nodes ?? [] as n (n.id)}
        {@const key = `node:${n.id}`}
        {@const linked = (config?.links ?? []).some((l) => l.to === n.id)}
        {@const check = checks[n.id]}
        <NodeCard
          {n}
          {config}
          live={live[n.id]}
          chip={chips[n.id]}
          {connected}
          {compact}
          state={states[n.id]}
          {still}
          level={fill[n.id]}
          confirmed={confirmedOf(n.id)}
          dragging={dragging === n.id}
          drop={dropOn === n.id}
          {check}
          {checksLocked}
          oncheck={() => oncheck?.(n.id)}
          at={at(n.id)}
          width={NODE_W}
          {key}
          focused={focusKey === key}
          pressed={selected === key}
          bind:height={heights[n.id]}
          onpointerdown={(e) => cardDown(e, n.id)}
          onclick={() => pick(key)}
        />
        {#if !isHub(n) || linked}
          <span transition:fade|global class="port in" class:linked aria-hidden="true" style:left="{at(n.id).x + NODE_W / 2}px" style:top="{at(n.id).y}px" class:ok={wire !== null && wire.from !== n.id && reasons[n.id] === null}></span>
        {/if}
        {#if onlink && !(config?.links ?? []).some((x) => x.from === n.id)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            transition:fade|global
            class="port out"
            aria-hidden="true"
            title={LINK.portOut(n.name)}
            style:left="{at(n.id).x + NODE_W / 2}px"
            style:top="{at(n.id).y + hOf(n.id)}px"
            onpointerdown={(e) => portDown(e, n.id)}
          ></span>
        {/if}
      {/each}

      {#each drawn as d (d.key)}
        <!-- A link's end on the node nearer the PC, over the card's edge; in 準備 another link can start from it. -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <span
          transition:fade
          class="port end"
          class:out={!!onlink}
          class:i2c={d.l.kind === LinkKind.I2c}
          aria-hidden="true"
          title={onlink ? LINK.portOut(nameOf(d.l.from)) : undefined}
          style:left="{d.start.x}px"
          style:top="{d.start.y}px"
          onpointerdown={(e) => portDown(e, d.l.from)}
        ></span>
        <button
          transition:fade
          class="link-chip"
          class:selected={d.open}
          class:uns={d.skipped}
          class:leftward={d.outward < 0}
          style:left="{d.mid.x + d.outward * d.clear}px"
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
</div>

<style>
  .graph {
    position: relative;
    display: grid;
    min-width: 0;
    min-height: 0;
  }
  /* The window on the canvas: its size is the stage's alone, and clip (not hidden) lets nothing scroll it; only the view moves what is in it. */
  .box {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: clip;
    cursor: grab;
    touch-action: none;
  }
  .box:active {
    cursor: grabbing;
  }
  .placing .box {
    cursor: crosshair;
  }
  /* What the next click does, over the canvas's top edge while a card waits for it. */
  .placing-hint {
    position: absolute;
    z-index: var(--z-chip);
    top: var(--s-3);
    left: 50%;
    translate: -50% 0;
    max-width: calc(100% - 2 * var(--s-3));
    margin: 0;
    padding: var(--s-1) var(--s-3);
    border: 1px solid var(--c-accent);
    border-radius: var(--r-s);
    background: var(--c-layer-2);
    color: var(--c-paper);
    font-size: var(--t-small);
    pointer-events: none;
  }
  /* The zoom alone scales the canvas: the 2560 theme's type stays out of it, so its 150 % reads true. */
  .canvas {
    --t-version: 20px;
    --t-body: 16px;
    --t-small: 14px;
    --t-caption: 12px;
    --t-mono: 13px;
    --i-state: 16px;
    position: absolute;
    top: 0;
    left: 0;
    width: 0;
    height: 0;
    transform-origin: 0 0;
    user-select: none;
  }
  .canvas.gliding {
    transition: transform var(--dur-line) var(--ease-out);
  }
  .links {
    position: absolute;
    top: 0;
    left: 0;
    width: 1px;
    height: 1px;
    overflow: visible;
  }
  .pc-link {
    stroke: var(--c-line);
    stroke-width: 2;
  }
  .pc-link.dashed {
    stroke-dasharray: 4 3;
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
  /* ◇ skipped by updating: paper, dashed, never red. */
  .bundle.uns .wire {
    stroke: var(--c-paper);
    stroke-dasharray: 2 4;
    opacity: 0.8;
  }
  .tie {
    fill: var(--c-line);
  }
  .end {
    fill: var(--c-ink);
    stroke: var(--c-line);
    stroke-width: 2;
  }
  .temp {
    fill: none;
    stroke: var(--c-paper);
    stroke-width: 2;
    stroke-dasharray: 4 4;
  }
  .temp.bad {
    stroke: var(--c-signal);
  }
  .uplink {
    position: absolute;
    z-index: 1;
    display: block;
    text-align: center;
    min-width: 168px;
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-muted);
    font: 400 var(--t-caption) / 24px var(--f-ja);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transform: translateX(-50%);
  }
  .uplink.off {
    border-style: dashed;
  }
  .empty {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 352px;
    margin: 0;
    padding: 12px 16px;
    transform: translate(-50%, -50%);
    border: 1px dashed var(--c-line);
    border-radius: var(--r-m);
    color: var(--c-muted);
    font: 400 var(--t-small) / 20px var(--f-ja);
    text-align: center;
  }

  /* Ports: the lower one starts a drag, the upper one takes it. */
  /* Over the cards, also the one in the hand: a port is never half hidden. */
  .port {
    position: absolute;
    z-index: calc(var(--z-drag) + 1);
    width: 16px;
    height: 16px;
    margin: -8px 0 0 -8px;
    border: 2px solid var(--c-line);
    border-radius: 50%;
    background: var(--c-ink);
    pointer-events: none;
    transition:
      transform var(--dur-nudge) var(--ease-out),
      border-color var(--dur-quick) var(--ease-out),
      background-color var(--dur-quick) var(--ease-out);
  }
  .port.end {
    background: var(--c-ink);
  }
  /* An I2C bus's end: the double ring the links draw for a shared bus. */
  .port.i2c::after {
    content: '';
    position: absolute;
    inset: 4px;
    border-radius: 50%;
    background: var(--c-line);
  }
  .port.out {
    pointer-events: auto;
    cursor: crosshair;
    touch-action: none;
  }
  @media (hover: hover) and (pointer: fine) {
    .port.out:hover {
      border-color: var(--c-paper);
      transform: scale(1.3);
    }
  }
  .port.ok {
    border-color: var(--c-paper);
    background: var(--c-paper);
    transform: scale(1.3);
  }

  .link-chip {
    position: absolute;
    z-index: var(--z-chip);
    height: 24px;
    padding: 0 var(--s-2);
    transform: translateY(-50%);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 700 var(--t-caption) / 20px var(--f-ja);
    white-space: nowrap;
    cursor: pointer;
  }
  .link-chip.leftward {
    transform: translate(-100%, -50%);
  }
  .link-chip.uns {
    border-style: dashed;
    border-color: var(--c-paper);
  }
  .link-chip.selected {
    border-color: var(--c-paper);
    box-shadow: 0 0 0 1px var(--c-paper);
  }
  .link-chip:focus-visible {
    outline: 2px solid var(--c-paper);
  }
</style>
