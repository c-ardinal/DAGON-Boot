<script lang="ts" module>
  /** A node chip set by the page (update progress) instead of the watch result. */
  export interface Chip {
    chip: string;
    spoken: string;
    silent: boolean;
    /** A label-value line under the chip ("目安 約 1 分", "自動の再接続 あと 30 s"). */
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
  /** The zoom's limits and one step of it. */
  export const ZOOM_LIMITS = { min: 0.5, max: 2, step: 1.2 };
  /** From this many ノード the cards are compact whatever the zoom (name, version and state only). */
  export const COMPACT_FROM = 9;
</script>

<script lang="ts">
  import { CANVAS, LINK, NODE, SLOT, STATE, WRITE } from '../app/copy.ts';
  import { easeOut, fade, draw, lateFade, moves, rise, tokenMs, tokenNumber, waterEnd } from '../app/motion.ts';
  import { lightsStill, nodeState, STATE_ICON, type NodeState } from '../app/nodeState.ts';
  import Icon from '../screens/Icon.svelte';
  import { KIND_LABEL, type LinkKind } from '../app/define/model.ts';
  import type { Live } from '../app/operate.svelte.ts';
  import type { Config, Link, Node } from '../app/sample.ts';
  import { versionText } from '../wasm/dagon.ts';
  import { along, CableField, FrameLoop, restPoints, runLengths, strandPath } from './cable.ts';
  import { neighbour, pathD, tieLengths, verticalLink, wireOffsets, type Bezier, type Pt } from './geometry.ts';
  import { treeLayout } from './layout.ts';
  import { untrack } from 'svelte';

  interface Props {
    config: Config | null;
    live: Record<string, Live>;
    /** The hub is connected: the PC link is solid and nodes carry live marks. */
    connected: boolean;
    /** The configuration is on its way to the hub. */
    sending: boolean;
    selected: string | null;
    onselect: (key: string) => void;
    chips?: Record<string, Chip>;
    /** The canvas's zoom and pan: the user's. */
    view?: View;
    /** Where the user put each card; the others stand in the tree layout. */
    positions?: Record<string, Pt>;
    /** A card drag ended. */
    onmoved?: () => void;
    /** The free part of the window (client px) a fit puts the graph in; read only when fitting. */
    freeArea?: () => DOMRect | undefined;
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
    sending,
    selected,
    onselect,
    chips = {},
    view = $bindable(newView()),
    positions = $bindable({}),
    onmoved,
    freeArea,
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
  /** Card heights before they are measured (px). */
  const EST_H = { normal: 140, compact: 56 };
  /** Room above a top 親機 for its uplink card, and around the graph when fitting (px). */
  const UPLINK_ROOM = 84;
  const SIDE = 24;
  /** Between a bundle's outer strand and its chip, and the chip's height (px). */
  const CHIP_CLEAR = 12;
  const CHIP_H = 22;
  /** How far from a leaning run a chip stands so the run passes clear over its whole height. */
  const clearOf = (width: number, dir: Pt) => {
    const down = Math.max(0.2, Math.abs(dir.y));
    return (width / 2) / down + (CHIP_H / 2) * (Math.abs(dir.x) / down) + CHIP_CLEAR;
  };
  /** A press moves this far before it is a drag, not a click (px). */
  const DRAG_PX = 4;

  let heights = $state<Record<string, number>>({});
  let focusKey = $state('node:hub');
  let canvas: HTMLDivElement;
  let boxEl = $state<HTMLDivElement>();

  /** Card geometry of each class (normal, compact): width and gaps, in canvas px; the zoom alone scales them. The rows stand far apart: the links' length is what lets a cable sweep and read. */
  const sizes = (compact: boolean) => (compact ? { w: 208, gapX: 20, gapY: 96 } : { w: 260, gapX: 80, gapY: 176 });
  const nodesOf = $derived((config?.nodes ?? []).map((n) => ({ id: n.id, hub: n.role === 'hub' })));
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

  // A new layout: cards that have a place glide to the new one; a new or dragged card is there at once.
  $effect(() => {
    const to = pos;
    untrack(() => {
      const now = Object.fromEntries(Object.entries(to).map(([id, q]): [string, Pt] => [id, arriving || id === dragging || !shown[id] ? q : shown[id]]));
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

  /** Everything placed, in canvas px. */
  function bounds() {
    const cards = (config?.nodes ?? []).map((n) => ({ ...pos[n.id], h: hOf(n.id), top: tops.some((t) => t.id === n.id) ? UPLINK_ROOM : 0 }));
    const x = Math.min(...cards.map((c) => c.x));
    const y = Math.min(...cards.map((c) => c.y - c.top));
    return { x, y, w: Math.max(...cards.map((c) => c.x + NODE_W)) - x, h: Math.max(...cards.map((c) => c.y + c.h)) - y };
  }

  /** The canvas moves smoothly for a pressed fit or zoom; never for a drag or the wheel, which follow the hand. */
  let gliding = $state(false);
  const clampZoom = (z: number) => Math.min(ZOOM_LIMITS.max, Math.max(ZOOM_LIMITS.min, z));

  /** The explicit fit: the whole graph in the free part of the window, at most `cap`. */
  export function fit(cap: number = firstZoom(), smooth = true) {
    if (!boxEl || !config?.nodes.length) return;
    const box = boxEl.getBoundingClientRect();
    const free = freeArea?.() ?? box;
    const b = bounds();
    const zoom = clampZoom(Math.min(cap, free.width / (b.w + 2 * SIDE), free.height / (b.h + 2 * SIDE)));
    gliding = smooth;
    view = {
      zoom,
      x: free.left - box.left + (free.width - b.w * zoom) / 2 - b.x * zoom,
      y: free.top - box.top + (free.height - b.h * zoom) / 2 - b.y * zoom,
      set: true,
    };
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
      const open = selected === key;
      const route = routes[key];
      const points = shapes[key] ?? restPoints(route);
      const lengths = runLengths(points);
      const total = lengths[lengths.length - 1];
      const gap = open ? 8 : 4;
      const count = Math.max(1, l.wires.length);
      const offsets = wireOffsets(count, gap);
      const width = (count - 1) * gap;
      const ties = open
        ? []
        : tieLengths(total).map((s) => {
            const { p, dir } = along(points, lengths, s);
            return { p, deg: (Math.atan2(dir.y, dir.x) * 180) / Math.PI - 90, len: width + 4 };
          });
      const { p: mid, dir: midDir } = along(points, lengths, total * 0.5);
      // Beside the strands, on the side the cable leans out to.
      const outward = route[3].x < route[0].x ? -1 : 1;
      const label = bundleLabel(l);
      return { l, key, open, outward, clear: clearOf(width, midDir), start: route[0], strands: offsets.map((o) => strandPath(points, o)), ties, mid, label, name: LINK.name(LINK.bundle(l.kind, l.wires.length, l.wires.filter((w) => w.shared).length, label.slice(l.kind.length + 1)), nameOf(l.from), nameOf(l.to), l.addr), skipped: !!config?.nodes.find((n) => n.id === l.to)?.notYet };
    }),
  );

  function bundleLabel(l: Link): string {
    // The bus and its value: the strands show their count, and the words for it are in the name and the sheet.
    const value = l.addr || config?.nodes.find((n) => n.id === l.to)?.via.split(' ')[1] || '';
    return [l.kind, value].filter(Boolean).join(' ');
  }

  /** Focus stops in screen order: nodes, then link chips. */
  const stops = $derived([
    ...(config?.nodes ?? []).map((n) => ({ key: `node:${n.id}`, at: { x: at(n.id).x + NODE_W / 2, y: at(n.id).y + 40 } })),
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
    if (dragged) return void (dragged = false);
    onselect(key);
  }

  type Mark = Chip | 'skeleton' | null;

  function markOf(n: Node): Mark {
    if (n.notYet) {
      const why = NODE.notYetWhy[n.notYet] ?? '';
      return { chip: NODE.notYet, spoken: NODE.notYetName(why), silent: false, note: compact ? undefined : why };
    }
    if (chips[n.id]) return chips[n.id];
    if (!connected) return null;
    const l = live[n.id];
    if (!l || l.responding === null) return 'skeleton';
    // 応答あり・応答なし are the state mark itself: no chip repeats them.
    if (!l.responding) return { chip: '', spoken: NODE.silentName, silent: true };
    if (l.hello?.trial) return { chip: NODE.trial, spoken: NODE.trialName, silent: false };
    return { chip: '', spoken: NODE.respondingName, silent: false };
  }

  function versionOf(n: Node): string {
    const h = connected ? live[n.id]?.hello : null;
    return h ? versionText(h.activeVersion) : '';
  }

  /** The slot line (A / B with versions, ▸ on the running one), from the node's last STATUS. */
  function slotsOf(n: Node): { row: string; text: string; on: boolean }[] | null {
    const st = connected ? live[n.id]?.status : null;
    if (!st) return null;
    return st.slots.slice(0, 2).map((s, i) => ({ row: SLOT.rows[i], text: s.state === 0 ? SLOT.states[0] : versionText(s.version), on: st.active === i }));
  }

  /** Each node's state mark: from the page's chip, the water level and the watch. */
  const states = $derived(
    Object.fromEntries(
      (config?.nodes ?? []).map((n): [string, NodeState] => {
        const l = live[n.id];
        return [n.id, nodeState({ connected, responding: l?.responding, trial: !!l?.hello?.trial, chip: chips[n.id]?.chip, writing: fill[n.id] !== undefined || building.includes(n.id), unreachable: !!n.notYet })];
      }),
    ),
  );
  const still = $derived(lightsStill(Object.values(states)));

  /** A 親機's own connection line: its uplink as the 構成 writes it, or the link from the node above it. */
  function hubVia(n: Node): string {
    if (!n.uplink) return n.via || NODE.usb;
    return NODE.uplinkVia(KIND_LABEL[n.uplink.kind as LinkKind] ?? n.uplink.kind, n.uplink.host);
  }

  /** The card's one mono line: MCU and how it is reached. */
  const metaOf = (n: Node) => [n.mcu, n.role === 'hub' ? hubVia(n) : n.via].filter(Boolean).join(' · ');

  /** 親機 nobody links to, each with its uplink (what drives it, as the 構成 writes it) above it. */
  const tops = $derived((config?.nodes ?? []).filter((n) => n.role === 'hub' && tree.roots.includes(n.id)));
  /** The 親機 this page connects to: the one updating reaches (the cut's), else the first one the 構成 names. */
  const firstHub = $derived(
    config?.nodes.find((n) => n.role === 'hub' && config.reach.includes(n.id))?.id ?? config?.nodes.find((n) => n.role === 'hub')?.id,
  );

  /** The role on the card: 親機, 子機, 孫機, and below that its row. */
  function roleOf(n: Node): string {
    if (n.role === 'hub') return NODE.roleHub;
    const d = belowHub(n.id);
    return d <= 1 ? NODE.roleNode : d === 2 ? NODE.roleGrandchild : NODE.roleDeeper(d);
  }

  /** Links from the nearest 親機 above node `id` (1 for a 子機; a node no 親機 reaches counts as a 子機). */
  function belowHub(id: string, seen = new Set<string>()): number {
    const up = config?.links.find((l) => l.to === id)?.from;
    const p = config?.nodes.find((x) => x.id === up);
    if (!p || p.role === 'hub' || seen.has(id)) return 1;
    seen.add(id);
    return belowHub(p.id, seen) + 1;
  }

  function l0Of(n: Node): string {
    if (connected && n.id === firstHub) return CANVAS.pc;
    const by = n.uplink ? (CANVAS.pcBy[n.uplink.host] ?? CANVAS.pc) : CANVAS.pc;
    return n.id === firstHub ? by : `${by}${CANVAS.notThisPc}`;
  }

  function spokenName(n: Node, m: Mark, version: string): string {
    return NODE.name({
      name: n.name,
      product: n.product,
      mcu: n.mcu,
      role: roleOf(n),
      via: n.role === 'hub' ? hubVia(n) : NODE.viaName(n.via),
      state: m && m !== 'skeleton' ? m.spoken : '',
      version,
    });
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

<div class="graph">
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
          <line x1={x} y1={y0} x2={x} y2={at(h.id).y} class="pc-link" class:dashed={!connected || h.id !== firstHub} class:busy={sending && h.id === firstHub} />
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
        {@const m = markOf(n)}
        {@const v = versionOf(n)}
        {@const level = fill[n.id]}
        {@const linked = (config?.links ?? []).some((l) => l.to === n.id)}
        {@const slots = slotsOf(n)}
        {@const check = checks[n.id]}
        <button
          transition:rise
          class="node st-{states[n.id]}"
          class:dragging={dragging === n.id}
          class:still
          class:uns={!!n.notYet}
          class:err={m !== null && m !== 'skeleton' && m.silent}
          class:drop={dropOn === n.id}
          class:checkable={check !== undefined}
          style:left="{at(n.id).x}px"
          style:top="{at(n.id).y}px"
          style:width="{NODE_W}px"
          data-key={key}
          data-avoid=""
          tabindex={focusKey === key ? 0 : -1}
          aria-pressed={selected === key}
          aria-label="{spokenName(n, m, v)}、{STATE[states[n.id]]}"
          bind:offsetHeight={heights[n.id]}
          onpointerdown={(e) => cardDown(e, n.id)}
          onclick={() => pick(key)}
        >
          {#if level !== undefined}
            <!-- The water rises with the node's progress; its edge is two CSS-masked strips drifting at different speeds. -->
            <span in:fade out:lateFade class="water" aria-hidden="true">
              <span out:waterEnd={{ confirmed: confirmedOf(n.id) }} class="level" style:transform="translateY({100 - Math.min(100, Math.max(0, level))}%)">
                <span class="crest back"></span><span class="crest front"></span>
              </span>
            </span>
          {/if}
          <span class="hd">
            <b>[{n.no}] {n.name}</b><span class="role">{roleOf(n)}</span>
            {#if v}{#key v}<span in:rise={{ dur: '--dur-quick' }} class="ver" translate="no">{v}</span>{/key}{:else if m === 'skeleton'}<span class="skeleton ver-sk"></span>{/if}
            {#key states[n.id]}<span in:rise={{ dur: '--dur-quick' }} class="state" title={STATE[states[n.id]]}><Icon name={STATE_ICON[states[n.id]]} size="var(--i-state)" /><span class="visually-hidden">{STATE[states[n.id]]}</span></span>{/key}
          </span>
          <span class="bd">
            <span class="prod" translate="no" title={n.product}>{n.product}</span>
            <span class="meta" translate="no">{metaOf(n)}</span>
            <!-- Its row, and the chip's below it, are always there: no card changes height when one fills. -->
            <span class="slotline" translate="no">{#each slots ?? [] as sl (sl.row)}<span class:on={sl.on}>{sl.row} {sl.text}</span>{/each}</span>
            <span class="chips"
              >{#if m === 'skeleton'}<span class="skeleton chip-sk"></span>{:else if m?.chip}<span in:rise={{ dur: '--dur-quick' }} out:lateFade class="chip" class:silent={m.silent} class:uns={!!n.notYet && m.chip === NODE.notYet}>{m.chip}</span>{/if}{#if m && m !== 'skeleton' && m.note}<span class="note" title={m.note}>{m.note}</span>{/if}</span
            >
          </span>
          {#if level !== undefined}<span in:fade out:lateFade={{ shown: confirmedOf(n.id) ? 100 : undefined }} class="pct" aria-hidden="true" style:--shown={Math.round(level)}></span>{/if}
        </button>
        {#if check !== undefined}
          <!-- Beside the card's button, never inside it: the check is its own control. -->
          <button
            in:fade
            class="check"
            role="checkbox"
            aria-checked={check}
            aria-label={WRITE.pick(`[${n.no}] ${n.name}`)}
            aria-disabled={checksLocked}
            data-avoid=""
            style:left="{at(n.id).x}px"
            style:top="{at(n.id).y}px"
            onclick={() => !checksLocked && oncheck?.(n.id)}><Icon name={check ? 'square-check' : 'square'} size="var(--i-state)" /></button
          >
        {/if}
        {#if n.role !== 'hub' || linked}
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
        <!-- A link's end on its parent, over the card's edge; in 準備 another link can start from it. -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <span
          transition:fade
          class="port end"
          class:out={!!onlink}
          class:i2c={d.l.kind === 'I2C'}
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
  /* The link that carries the update right now marches from 親機 to 子機. */
  .pc-link.busy {
    stroke: var(--c-paper);
    stroke-dasharray: 6 6;
    animation: march var(--dur-march) var(--ease-steady) infinite;
  }
  @keyframes march {
    to {
      stroke-dashoffset: -12;
    }
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
  /* ◇ not updatable yet: paper, dashed, never red. */
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
    font: 400 var(--t-12) / calc(28px - 2px) var(--f-ja);
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
    font: 400 var(--t-13) / 1.45 var(--f-ja);
    text-align: center;
  }

  /* NodeCard: header (name, role, version, state mark), product, one mono line, chips.
     The frame carries the state: 1 px when normal, 2 px in the state colour for 要確認 and エラー. */
  .node {
    position: absolute;
    z-index: var(--z-node);
    display: grid;
    padding: 0;
    border: var(--st-w-thin) solid var(--st-idle);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-paper);
    text-align: left;
    font: 400 var(--t-small) / 1.45 var(--f-ja);
    cursor: pointer;
    overflow: hidden;
    transition:
      box-shadow var(--dur-quick) var(--ease-out),
      border-color var(--dur-ui) var(--ease-out),
      background-color var(--dur-quick) var(--ease-out);
  }
  .node.st-alive,
  .node.st-busy {
    border-color: var(--st-alive);
  }
  .node.st-silent {
    border-color: var(--st-silent);
  }
  .node.st-silent .prod {
    color: var(--c-muted);
  }
  .node.st-attention {
    border: var(--st-w-strong) solid var(--st-attention);
  }
  .node.st-error {
    border: var(--st-w-strong) solid var(--st-error);
  }
  .state {
    display: inline-flex;
    flex: none;
    align-self: center;
    padding: var(--s-1);
    color: var(--st-idle);
  }
  .st-alive .state {
    color: var(--c-paper);
  }
  .st-silent .state {
    color: var(--st-silent);
  }
  .st-busy .state {
    color: var(--st-busy);
  }
  .st-attention .state {
    color: var(--st-attention);
  }
  .st-error .state {
    color: var(--st-error);
  }
  /* 作業中: a 48 px light runs once round the frame every 1.6 s; the only moving thing on the canvas. */
  @property --orbit {
    syntax: '<angle>';
    inherits: false;
    initial-value: 0deg;
  }
  .node.st-busy::after {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 2;
    padding: var(--st-w-strong);
    border-radius: inherit;
    background: conic-gradient(from var(--orbit), var(--st-busy) 0 30deg, transparent 30deg);
    mask:
      linear-gradient(#000 0 0) content-box exclude,
      linear-gradient(#000 0 0);
    pointer-events: none;
    animation: orbit var(--dur-orbit) var(--ease-steady) infinite;
  }
  @keyframes orbit {
    to {
      --orbit: 360deg;
    }
  }
  /* Standing still (reduced motion, or another node needs attention): a dashed light along the top edge. */
  .node.st-busy.still::after {
    inset: 0 0 auto;
    height: var(--st-w-strong);
    padding: 0;
    mask: none;
    background: repeating-linear-gradient(90deg, var(--st-busy) 0 12px, transparent 12px 20px);
    animation: none;
  }
  @media (prefers-reduced-motion: reduce) {
    .node.st-busy::after {
      inset: 0 0 auto;
      height: var(--st-w-strong);
      padding: 0;
      mask: none;
      background: repeating-linear-gradient(90deg, var(--st-busy) 0 12px, transparent 12px 20px);
      animation: none;
    }
  }
  .node[aria-pressed='true'] {
    box-shadow: 0 0 0 var(--st-w-thin) var(--c-paper);
  }
  .node.drop {
    box-shadow: 0 0 0 2px var(--c-accent);
  }
  .node.uns {
    border-style: dashed;
  }
  .node:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
  @media (hover: hover) and (pointer: fine) {
    .node:not([aria-pressed='true']):hover {
      background: var(--c-layer-1);
    }
  }
  .hd,
  .bd,
  .pct {
    position: relative;
    z-index: 1;
  }
  /* 書込: the card check sits at the card's top-left, in the header's padding. */
  .check {
    position: absolute;
    /* Above its card (ports sit there too). */
    z-index: calc(var(--z-node) + 1);
    display: grid;
    place-items: center;
    width: calc(var(--i-state) + 2 * var(--s-2));
    height: calc(var(--i-state) + 2 * var(--s-2));
    margin: var(--s-1) 0 0 var(--s-1);
    padding: 0;
    border: 0;
    background: none;
    color: var(--c-muted);
    cursor: pointer;
  }
  .check[aria-checked='true'] {
    color: var(--c-accent);
  }
  .check[aria-disabled='true'] {
    color: var(--c-muted);
    cursor: default;
  }
  .check:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
  }
  .node.checkable .hd {
    padding-left: calc(var(--i-state) + 2 * var(--s-2) + var(--s-1));
  }
  .hd {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    min-width: 0;
    height: 40px;
    padding: var(--s-2) var(--s-3);
    background: var(--c-layer-1);
  }
  .hd b {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 700 var(--t-small) / 20px var(--f-ja);
  }
  /* The card in the hand is over the others. */
  .node.dragging {
    z-index: var(--z-drag);
    cursor: grabbing;
  }
  .role {
    color: var(--c-muted);
    font: 500 var(--t-caption) / 16px var(--f-ja);
    white-space: nowrap;
  }
  .ver {
    margin-left: auto;
    font: 400 var(--t-version) / 24px var(--f-version);
    font-variant-numeric: tabular-nums;
  }
  .ver-sk {
    width: 40px;
    height: 16px;
    margin-left: auto;
  }
  .state {
    margin-left: auto;
  }
  .ver + .state,
  .ver-sk + .state {
    margin-left: 0;
  }
  .bd {
    display: grid;
    gap: 0;
    min-width: 0;
    padding: var(--s-3) var(--s-4);
  }
  .prod {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 700 var(--t-body) / 24px var(--f-ja);
  }
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--c-muted);
    font: 500 var(--t-mono) / 24px var(--f-mono);
  }
  .chips {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    min-height: 20px;
    margin-top: 4px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
    font: 700 var(--t-11) / 1 var(--f-ja);
    white-space: nowrap;
    transition:
      border-color var(--dur-ui) var(--ease-out),
      background-color var(--dur-ui) var(--ease-out),
      color var(--dur-ui) var(--ease-out);
  }
  .slotline {
    display: flex;
    min-height: 16px;
    gap: 8px;
    color: var(--c-muted);
    font: 500 var(--t-11) / 1.5 var(--f-mono);
  }
  .slotline .on {
    color: var(--c-paper);
  }
  .slotline .on::before {
    content: '▸';
    margin-right: 2px;
  }
  .compact .slotline {
    display: none;
  }
  .chip.silent {
    border-color: var(--c-signal);
    background: var(--c-signal);
    color: var(--c-ink);
  }
  .chip.uns {
    border-style: dashed;
    border-color: var(--c-paper);
  }
  .chip-sk {
    width: 72px;
    height: 20px;
    border-radius: var(--r-full);
  }
  .note {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--c-muted);
    font: 400 var(--t-11) / 1.4 var(--f-ja);
  }
  /* The number counts up with the water: the browser moves --shown between the values it is given. */
  @property --shown {
    syntax: '<integer>';
    inherits: false;
    initial-value: 0;
  }
  .pct {
    position: absolute;
    right: 12px;
    bottom: 10px;
    font: 400 24px / 1 var(--f-version);
    counter-reset: shown var(--shown);
    transition: --shown var(--dur-rise) var(--ease-steady);
  }
  .pct::before {
    content: counter(shown) '%';
  }
  /* Compact (9 ノード and on, or when normal cards would shrink names under 13 px): name, version, state. */
  .compact .prod,
  .compact .meta,
  .compact .role,
  .compact .note {
    display: none;
  }
  .compact .hd {
    border-bottom: 0;
  }
  /* Past this zoom the name holds its 13 px on screen. */
  .compact .hd b {
    flex: 1;
    min-width: 0;
    font-size: max(var(--t-small), calc(13px / var(--zoom)));
  }
  .compact .bd {
    padding-top: 0;
  }
  .compact .chips {
    margin-top: 2px;
  }

  /* Water gauge: the level moves by transform; the edge is a CSS mask (no drawn illustration). */
  .water {
    position: absolute;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    border-radius: inherit;
    pointer-events: none;
    --wave-front: color-mix(in srgb, var(--c-accent) 32%, var(--c-ink));
    --wave-back: color-mix(in srgb, var(--c-accent) 18%, var(--c-ink));
  }
  .level {
    position: absolute;
    inset: 0;
    background: var(--wave-front);
    transition: transform var(--dur-rise) var(--ease-steady);
  }
  .crest {
    position: absolute;
    left: 0;
    width: 200%;
    height: 12px;
    -webkit-mask: radial-gradient(36px 9px at 50% 100%, #000 96%, transparent 100%) 0 0 / 64px 12px repeat-x;
    mask: radial-gradient(36px 9px at 50% 100%, #000 96%, transparent 100%) 0 0 / 64px 12px repeat-x;
  }
  .crest.front {
    top: -10px;
    background: var(--wave-front);
    animation: drift var(--dur-drift-front) var(--ease-steady) infinite;
  }
  .crest.back {
    top: -14px;
    background: var(--wave-back);
    animation: drift var(--dur-drift-back) var(--ease-steady) infinite reverse;
  }
  @keyframes drift {
    to {
      transform: translateX(-50%);
    }
  }

  /* Ports: the lower one starts a drag, the upper one takes it. */
  /* Over the cards, also the one in the hand: a port is never half hidden. */
  .port {
    position: absolute;
    z-index: calc(var(--z-drag) + 1);
    width: 14px;
    height: 14px;
    margin: -7px 0 0 -7px;
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
    inset: 2px;
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
    height: 22px;
    padding: 0 10px;
    transform: translateY(-50%);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 700 var(--t-11) / 20px var(--f-ja);
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

  @media (prefers-reduced-motion: reduce) {
    .crest,
    .pc-link.busy {
      animation: none;
    }
  }
</style>
