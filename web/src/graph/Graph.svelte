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
    /** The number is a failure count (問題 n > 0): red. */
    alarm?: boolean;
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

  /** Where a part dropped on the canvas goes: under `parent`, or on its own (a new tree). */
  export interface DropTarget {
    parent: string | null;
  }
</script>

<script lang="ts">
  import { CANVAS, LINK, NODE, SLOT, STATE } from '../app/copy.ts';
  import { lightsStill, nodeState, STATE_ICON, type NodeState } from '../app/nodeState.ts';
  import Icon from '../screens/Icon.svelte';
  import { KIND_LABEL, type LinkKind } from '../app/define/model.ts';
  import type { Live } from '../app/operate.svelte.ts';
  import type { Config, Link, Node } from '../app/sample.ts';
  import { versionText } from '../wasm/dagon.ts';
  import { lengthTable, neighbour, pathD, pointAt, shifted, tAtLength, tangentAt, tieLengths, verticalLink, wireOffsets, type Pt } from './geometry.ts';
  import { treeLayout } from './layout.ts';
  import { untrack, type Snippet } from 'svelte';

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
    /** The user's zoom on top of the fit (1 = fitted). */
    zoom?: number;
    /** Nodes being written: how full their water is, 0–100 (SC-O5). */
    fill?: Record<string, number>;
    /** Room the floating windows take on the left and right (px): the graph centres in what is left. */
    inset?: { left: number; right: number; bottom?: number };
    /** Port drag (定義 only): why `from` cannot take `to` under it, or null when it can. */
    canLink?: (from: string, to: string) => string | null;
    /** A port drag ended on a node that can be linked. */
    onlink?: (from: string, to: string) => void;
    /** A refused port drag: the reason (a failure, shown in red by the page). */
    onrefuse?: (why: string, x: number, y: number) => void;
    /** Small controls bottom-right of the canvas (zoom, view, undo). */
    controls?: Snippet<[number]>;
    /** The id of the node a dragged part would go under (highlighted). */
    dropOn?: string | null;
  }

  let {
    config,
    live,
    connected,
    sending,
    selected,
    onselect,
    dot,
    chips = {},
    zoom = 1,
    fill = {},
    inset = { left: 0, right: 0 },
    canLink,
    onlink,
    onrefuse,
    controls,
    dropOn = null,
  }: Props = $props();

  /**
   * Sizes in rem: the root font follows the window, so cards,
   * gaps and text grow together. `rem` is the root font in px, read again on resize.
   */
  let rem = $state(16);
  /** A phone-wide window (the CSS breakpoint 767 px): the controls sit top-right. */
  let phone = $state(false);
  const readRem = () => {
    rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
    phone = innerWidth < 768;
  };
  $effect(readRem);

  /** Names on the cards never get smaller than this on screen (px): past it the cards turn compact, then the canvas scrolls. */
  const NAME_MIN_PX = 13;
  /** From this many ノード the cards are compact whatever the window (name, version and state only). */
  const COMPACT_FROM = 9;
  /** Card heights before they are measured (rem): to decide the card class without a measuring loop. */
  const EST_H = { normal: 7.5, compact: 3.5 };

  let box = $state<DOMRectReadOnly>();
  const boxW = $derived(box?.width ?? 800);
  const boxH = $derived(box?.height ?? 600);
  let heights = $state<Record<string, number>>({});
  let dotH = $state(0);
  let dotW = $state(0);
  let focusKey = $state('node:hub');
  let canvas: HTMLDivElement;
  let boxEl: HTMLDivElement;

  /** Card geometry of each class (normal, compact): width, gaps, the uplink card above each top 親機. */
  const sizes = (compact: boolean) => ({
    w: (compact ? 11 : 15) * rem,
    gapX: (compact ? 1.25 : 2.5) * rem,
    gapY: (compact ? 3 : 4.5) * rem,
  });
  const nodesOf = $derived((config?.nodes ?? []).map((n) => ({ id: n.id, hub: n.role === 'hub' })));
  const UPLINK_ROOM = $derived(5.25 * rem);
  const SIDE = $derived(1.5 * rem);
  let zoomH = $state(0);
  /** Room under the graph kept free for the focal number and the controls. */
  /** Phones put the controls at the top (CSS): only the focal number needs the room below there. */
  const below = $derived(Math.max(dotH, phone ? 0 : zoomH) + rem + (inset.bottom ?? 0));
  const availW = $derived(Math.max(1, boxW - inset.left - inset.right));
  const availH = $derived(Math.max(1, boxH - below));
  const fitOf = (w: number, h: number) => Math.min(1.25, availW / (w + 2 * SIDE), availH / (h + UPLINK_ROOM + SIDE));
  /** Compact when there are many ノード, or when normal cards would have to shrink names under 13 px to fit. */
  const compact = $derived.by(() => {
    if (nodesOf.length >= COMPACT_FROM) return true;
    const est = treeLayout(nodesOf, config?.links ?? [], { ...sizes(false), h: () => EST_H.normal * rem });
    return fitOf(est.width, est.height) * rem < NAME_MIN_PX;
  });
  const S = $derived(sizes(compact));
  const NODE_W = $derived(S.w);
  const tree = $derived(treeLayout(nodesOf, config?.links ?? [], { ...S, h: (id) => heights[id] ?? (compact ? EST_H.compact : EST_H.normal) * rem }));
  /** Never under the scale where names reach 13 px; past that the canvas scrolls. */
  const wanted = $derived(Math.max(NAME_MIN_PX / rem, fitOf(tree.width, tree.height)));
  /**
   * The fit in use: it shrinks at once and grows only by more than GROW. Text wraps a
   * little differently at each scale, so card heights follow the fit; without the
   * margin two scales could take turns forever.
   */
  const GROW = 1.04;
  let fit = $state(1);
  $effect(() => {
    const w = wanted;
    const kept = untrack(() => fit);
    if (w < kept || w > kept * GROW) fit = w;
  });
  const scale = $derived(zoom * fit);
  const contentW = $derived(tree.width + 2 * SIDE);
  const contentH = $derived(UPLINK_ROOM + tree.height + SIDE);
  // Floored, so a fitted canvas never brings up a scrollbar (which would change the box and the fit again).
  const W = $derived(Math.floor(Math.max(boxW / scale, contentW + (inset.left + inset.right) / scale)));
  const H = $derived(Math.floor(Math.max(boxH / scale, contentH + below / scale)));
  /** The graph centred between the windows, and vertically in the room above the focal number. */
  const left = $derived(inset.left / scale + Math.max(SIDE, (availW / scale - tree.width) / 2));
  const top = $derived(UPLINK_ROOM + Math.max(0, (availH / scale - contentH) / 2));
  const pos = $derived(Object.fromEntries(Object.entries(tree.at).map(([id, p]) => [id, { x: left + p.x, y: top + p.y }])));
  const hOf = (id: string) => heights[id] ?? (compact ? EST_H.compact : EST_H.normal) * rem;

  const nameOf = (id: string) => config?.nodes.find((n) => n.id === id)?.name ?? id;

  const drawn = $derived(
    (config?.links ?? []).map((l) => {
      const key = `link:${l.to}`;
      const open = selected === key;
      const from = pos[l.from];
      const to = pos[l.to];
      const siblings = (config?.links ?? []).filter((x) => x.from === l.from);
      const slot = (siblings.indexOf(l) + 1) / (siblings.length + 1);
      const base = verticalLink({ x: from.x + NODE_W * slot, y: from.y + hOf(l.from) }, { x: to.x + NODE_W / 2, y: to.y });
      const table = lengthTable(base);
      const total = table[table.length - 1];
      const gap = open ? 8 : 4;
      const count = Math.max(1, l.wires.length);
      const offsets = wireOffsets(count, gap);
      const width = (count - 1) * gap;
      const ties = open
        ? []
        : tieLengths(total).map((s) => {
            const t = tAtLength(table, s);
            const d = tangentAt(base, t);
            return { p: pointAt(base, t), deg: (Math.atan2(d.y, d.x) * 180) / Math.PI - 90, len: width + 4 };
          });
      // Compact cards: the chip sits lower on the link, clear of the card above.
      const mid = pointAt(base, tAtLength(table, total * (compact ? 0.7 : 0.5)));
      const label = bundleLabel(l);
      return { l, key, open, base, offsets, ties, mid, label, name: LINK.name(label, nameOf(l.from), nameOf(l.to), l.addr), skipped: !!config?.nodes.find((n) => n.id === l.to)?.notYet };
    }),
  );

  function bundleLabel(l: Link): string {
    // Compact level of detail keeps the bus and its value; the wire count stays in the sheet.
    const value = l.addr || config?.nodes.find((n) => n.id === l.to)?.via.split(' ')[1] || '';
    if (compact) return [l.kind, value].filter(Boolean).join(' ');
    return LINK.bundle(l.kind, l.wires.length, l.wires.filter((w) => w.shared).length, value);
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

  /** Marks of the boot state: ✓ settled, ◐ waiting for the check, ↺ went back. */
  const BOOT_MARK: Record<number, string> = { 1: '…', 2: '…', 3: '◐', 4: '✓', 5: '↺' };
  /** The boot-state chip: the running slot's state from STATUS, else what HELLO says (on trial or not). */
  function bootOf(n: Node): { text: string; trial: boolean } | null {
    const l = connected ? live[n.id] : undefined;
    if (!l?.responding) return null;
    const st = l.status;
    if (st && st.active < 2) {
      const state = st.slots[st.active].state;
      return { text: `${BOOT_MARK[state] ?? ''} ${SLOT.states[state] ?? ''} ${SLOT.rows[st.active]}`.trim(), trial: state === 3 };
    }
    if (!l.hello) return null;
    // On trial the state chip (● 確認待ち) already says it.
    return l.hello.trial ? null : { text: `✓ ${SLOT.states[4]}`, trial: false };
  }

  /** Each node's state mark: from the page's chip, the water level and the watch. */
  const states = $derived(
    Object.fromEntries(
      (config?.nodes ?? []).map((n): [string, NodeState] => {
        const l = live[n.id];
        return [n.id, nodeState({ connected, responding: l?.responding, trial: !!l?.hello?.trial, chip: chips[n.id]?.chip, writing: fill[n.id] !== undefined, unreachable: !!n.notYet })];
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
    return { x: (cx - r.left) / scale, y: (cy - r.top) / scale };
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
      .map((n) => ({ n, bottom: pos[n.id].y + hOf(n.id), cx: pos[n.id].x + NODE_W / 2 }))
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
    const a = { x: pos[from].x + NODE_W / 2, y: pos[from].y + hOf(from) };
    wire = { from, a, b: a, target: null, bad: false };
  }
  /** Why each node cannot take the dragged link (null: it can), while dragging. */
  const reasons = $derived(
    wire ? Object.fromEntries((config?.nodes ?? []).filter((n) => n.id !== wire!.from).map((n) => [n.id, canLink?.(wire!.from, n.id) ?? null])) : {},
  );
  function portMove(e: PointerEvent) {
    if (!wire) return;
    const p = toCanvas(e.clientX, e.clientY);
    let best = SNAP_PX / scale;
    let target: string | null = null;
    let b = p;
    let over: string | null = null;
    for (const n of config?.nodes ?? []) {
      if (n.id === wire.from) continue;
      const q = { x: pos[n.id].x + NODE_W / 2, y: pos[n.id].y };
      const d = Math.hypot(q.x - p.x, q.y - p.y);
      const inCard = p.x > pos[n.id].x && p.x < pos[n.id].x + NODE_W && p.y > pos[n.id].y - 10 && p.y < pos[n.id].y + hOf(n.id);
      if (reasons[n.id] === null && (d < best || inCard)) {
        best = Math.min(best, d);
        target = n.id;
        b = q;
      } else if (inCard || d < SNAP_PX / scale) over = n.id;
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

<svelte:window onresize={readRem} />

<div class="graph">
  <div class="box" bind:this={boxEl} bind:contentRect={box}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="canvas"
      class:compact
      bind:this={canvas}
      style:width="{W}px"
      style:height="{H}px"
      style:zoom={scale === 1 ? undefined : scale}
      onkeydown={onKey}
      onpointermove={portMove}
      onpointerup={portUp}
      onpointercancel={() => (wire = null)}
    >
      <svg class="links" width={W} height={H} viewBox="0 0 {W} {H}" aria-hidden="true">
        {#each tops as h (h.id)}
          {@const x = pos[h.id].x + NODE_W / 2}
          {@const y0 = pos[h.id].y - UPLINK_ROOM + 2.5 * rem}
          <line x1={x} y1={y0} x2={x} y2={pos[h.id].y} class="pc-link" class:dashed={!connected || h.id !== firstHub} class:busy={sending && h.id === firstHub} />
          <circle cx={x} cy={y0} r="5" class="end" />
        {/each}

        {#each drawn as d (d.key)}
          <g class="bundle" class:open={d.open} class:uns={d.skipped}>
            {#each d.offsets as o, i (i)}
              <path d={pathD(shifted(d.base, o))} class="wire" class:shared={d.l.wires[i]?.shared} />
            {/each}
            {#each d.ties as tie, i (i)}
              <rect class="tie" x={-tie.len / 2} y={-3} width={tie.len} height={6} rx="3" transform="translate({tie.p.x} {tie.p.y}) rotate({tie.deg})" />
            {/each}
          </g>
          <circle cx={d.base[0].x} cy={d.base[0].y} r="6" class="end" />
          {#if d.l.kind === 'I2C'}<circle cx={d.base[0].x} cy={d.base[0].y} r="3" class="end" />{/if}
        {/each}
        {#if wire}<path d={pathD(verticalLink(wire.a, wire.b))} class="temp" class:bad={wire.bad} />{/if}
      </svg>

      {#each tops as h (h.id)}
        <div class="uplink" class:off={!connected || h.id !== firstHub} title={l0Of(h)} style:left="{pos[h.id].x + NODE_W / 2}px" style:top="{pos[h.id].y - UPLINK_ROOM + 0.75 * rem}px" style:max-width="{NODE_W + S.gapX}px">{l0Of(h)}</div>
      {/each}

      {#if !config}
        <p class="empty" style:top="{UPLINK_ROOM + 2 * rem}px">{CANVAS.empty}</p>
      {/if}

      {#each config?.nodes ?? [] as n (n.id)}
        {@const key = `node:${n.id}`}
        {@const m = markOf(n)}
        {@const v = versionOf(n)}
        {@const level = fill[n.id]}
        {@const linked = (config?.links ?? []).some((l) => l.to === n.id)}
        {@const slots = slotsOf(n)}
        {@const boot = m !== null && m !== 'skeleton' && m.silent ? null : bootOf(n)}
        <button
          class="node st-{states[n.id]}"
          class:still
          class:uns={!!n.notYet}
          class:err={m !== null && m !== 'skeleton' && m.silent}
          class:drop={dropOn === n.id}
          style:left="{pos[n.id].x}px"
          style:top="{pos[n.id].y}px"
          style:width="{NODE_W}px"
          data-key={key}
          data-avoid=""
          tabindex={focusKey === key ? 0 : -1}
          aria-pressed={selected === key}
          aria-label="{spokenName(n, m, v)}、{STATE[states[n.id]]}"
          bind:offsetHeight={heights[n.id]}
          onclick={() => pick(key)}
        >
          {#if level !== undefined}
            <!-- The water rises with the node's progress; its edge is two CSS-masked strips drifting at different speeds. -->
            <span class="water" aria-hidden="true">
              <span class="level" style:transform="translateY({100 - Math.min(100, Math.max(0, level))}%)">
                <span class="crest back"></span><span class="crest front"></span>
              </span>
            </span>
          {/if}
          <span class="hd">
            <b>[{n.no}] {n.name}</b><span class="role">{roleOf(n)}</span>
            {#if v}<span class="ver" translate="no">{v}</span>{:else if m === 'skeleton'}<span class="skeleton ver-sk"></span>{/if}
            <span class="state" title={STATE[states[n.id]]}><Icon name={STATE_ICON[states[n.id]]} size="var(--i-state)" /><span class="visually-hidden">{STATE[states[n.id]]}</span></span>
          </span>
          <span class="bd">
            <span class="prod" translate="no" title={n.product}>{n.product}</span>
            <span class="meta" translate="no">{metaOf(n)}</span>
            {#if slots}
              <span class="slotline" translate="no">{#each slots as sl (sl.row)}<span class:on={sl.on}>{sl.row} {sl.text}</span>{/each}</span>
            {/if}
            {#if m === 'skeleton'}
              <span class="chips"><span class="skeleton chip-sk"></span></span>
            {:else if m || boot}
              <span class="chips"
                >{#if m?.chip}<span class="chip" class:silent={m.silent} class:uns={!!n.notYet && m.chip === NODE.notYet}>{m.chip}</span>{/if}{#if boot}<span
                    class="chip boot"
                    class:trial={boot.trial}>{boot.text}</span
                  >{/if}</span
              >
              {#if m?.note}<span class="note">{m.note}</span>{/if}
            {/if}
          </span>
          {#if level !== undefined}<span class="pct" aria-hidden="true">{Math.round(level)}%</span>{/if}
        </button>
        {#if n.role !== 'hub' || linked}
          <span class="port in" class:linked aria-hidden="true" style:left="{pos[n.id].x + NODE_W / 2}px" style:top="{pos[n.id].y}px" class:ok={wire !== null && wire.from !== n.id && reasons[n.id] === null}></span>
        {/if}
        {#if onlink}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            class="port out"
            aria-hidden="true"
            title={LINK.portOut(n.name)}
            style:left="{pos[n.id].x + NODE_W / 2}px"
            style:top="{pos[n.id].y + hOf(n.id)}px"
            onpointerdown={(e) => portDown(e, n.id)}
          ></span>
        {/if}
      {/each}

      {#each drawn as d (d.key)}
        <button
          class="link-chip"
          class:selected={d.open}
          class:uns={d.skipped}
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
  {#if dot}
    <!-- The focal number: bottom-left, just above what is under the canvas; it never scrolls away. -->
    <div class="hero" class:small={compact} style:bottom={inset.bottom ? `${inset.bottom + 0.75 * rem}px` : undefined} data-avoid="" role="img" aria-label={dot.name} bind:offsetHeight={dotH} bind:offsetWidth={dotW}>
      <span class="lbl">{dot.label}</span>
      <span class="row"><span class="num" class:err={dot.alarm && dot.value > 0}>{dot.value}</span>{#if dot.of !== undefined}<span class="of">/{dot.of}</span>{/if}{#if dot.unit}<span class="of">{dot.unit}</span>{/if}</span>
      {#if dot.sub}<span class="sub"
          ><span class="k">{dot.sub[0]}</span>{dot.sub[1]}{#each dot.more ?? [] as [label, value] (label)}&emsp;<span class="k">{label}</span>{value}{/each}</span
        >{/if}
    </div>
  {/if}
  {#if controls}<div class="zoom" data-avoid="" bind:offsetHeight={zoomH}>{@render controls(scale)}</div>{/if}
</div>

<style>
  .graph {
    position: relative;
    display: grid;
    min-width: 0;
    min-height: 0;
  }
  .box {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    scrollbar-gutter: stable;
    scrollbar-width: thin;
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
  /* The link that carries the update right now marches from 親機 to 子機. */
  .pc-link.busy {
    stroke: var(--c-paper);
    stroke-dasharray: 6 6;
    animation: march 600ms linear infinite;
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
  /* ◇ not updatable yet: paper, dashed, never red (V-23). */
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
    min-width: 10.5rem;
    height: 1.75rem;
    padding: 0 0.75rem;
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-muted);
    font: 400 var(--t-12) / calc(1.75rem - 2px) var(--f-ja);
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
    left: 50%;
    width: 22rem;
    margin: 0;
    padding: 0.75rem 1rem;
    transform: translateX(-50%);
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
    transition: box-shadow 160ms var(--ease-out);
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
    animation: orbit var(--dur-orbit) linear infinite;
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
    width: 2.5rem;
    height: 1rem;
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
    flex-wrap: wrap;
    gap: 0.25rem;
    margin-top: 0.25rem;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: 1.25rem;
    padding: 0 0.5rem;
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
    font: 700 var(--t-11) / 1 var(--f-ja);
    white-space: nowrap;
  }
  .chip.boot.trial {
    border-style: dashed;
  }
  .slotline {
    display: flex;
    gap: 0.5rem;
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
  .compact .slotline,
  .compact .chip.boot {
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
    width: 4.5rem;
    height: 1.25rem;
    border-radius: var(--r-full);
  }
  .note {
    color: var(--c-muted);
    font: 400 var(--t-11) / 1.4 var(--f-ja);
  }
  .pct {
    position: absolute;
    right: 0.75rem;
    bottom: 0.625rem;
    font: 400 1.5rem / 1 var(--f-version);
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
  .compact .hd b {
    flex: 1;
    min-width: 0;
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
    transition: transform 250ms linear;
  }
  .crest {
    position: absolute;
    left: 0;
    width: 200%;
    height: 0.75rem;
    -webkit-mask: radial-gradient(2.25rem 0.5625rem at 50% 100%, #000 96%, transparent 100%) 0 0 / 4rem 0.75rem repeat-x;
    mask: radial-gradient(2.25rem 0.5625rem at 50% 100%, #000 96%, transparent 100%) 0 0 / 4rem 0.75rem repeat-x;
  }
  .crest.front {
    top: -10px;
    background: var(--wave-front);
    animation: drift 4.2s linear infinite;
  }
  .crest.back {
    top: -0.875rem;
    background: var(--wave-back);
    animation: drift 7s linear infinite reverse;
  }
  @keyframes drift {
    to {
      transform: translateX(-50%);
    }
  }

  /* Ports: the lower one starts a drag, the upper one takes it. */
  .port {
    position: absolute;
    z-index: calc(var(--z-node) + 1);
    width: 0.875rem;
    height: 0.875rem;
    margin: -0.4375rem 0 0 -0.4375rem;
    border: 2px solid var(--c-line);
    border-radius: 50%;
    background: var(--c-ink);
    pointer-events: none;
    transition: transform 160ms var(--ease-out), border-color 160ms ease, background-color 160ms ease;
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
    height: 1.375rem;
    padding: 0 0.625rem;
    transform: translate(-50%, -50%);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 700 var(--t-11) / 1.25rem var(--f-ja);
    white-space: nowrap;
    cursor: pointer;
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

  /* The focal number: today's dot numerals, bottom-left. */
  .hero {
    position: absolute;
    z-index: var(--z-chip);
    left: var(--m-edge);
    bottom: var(--m-edge);
    display: grid;
    gap: 2px;
    pointer-events: none;
  }
  .lbl {
    color: var(--c-muted);
    font: 400 var(--t-13) / 1.3 var(--f-ja);
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: 0.375rem;
  }
  .num {
    color: var(--c-paper);
    font: 700 7rem / 0.9 var(--f-dot);
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
  }
  .num.err {
    color: var(--c-signal);
  }
  .of {
    color: var(--c-muted);
    font: 700 2.75rem / 1 var(--f-dot);
  }
  .small .num {
    font-size: 4rem;
  }
  .small .of {
    font-size: 1.75rem;
  }
  .sub {
    color: var(--c-paper);
    font: 400 var(--t-13) / 1.45 var(--f-ja);
  }
  .sub .k {
    margin-right: 0.25rem;
    color: var(--c-muted);
  }
  @media (max-width: 767px) {
    .num {
      font-size: 3.5rem;
    }
    .of {
      font-size: 1.5rem;
    }
  }
  /* Canvas controls: bottom-right at the edge margin; the zoom pill is the right-most. */
  .zoom {
    position: absolute;
    right: var(--m-edge);
    bottom: var(--m-edge);
    z-index: var(--z-chrome);
    display: flex;
    align-items: flex-end;
    gap: var(--s-2);
  }
  /* Phones: the focal number takes the bottom-left; the controls move to the top-right corner. */
  @media (max-width: 767px) {
    .zoom {
      top: 0.5rem;
      bottom: auto;
      right: 0.5rem;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .crest,
    .pc-link.busy {
      animation: none;
    }
    .level {
      transition: none;
    }
    .port,
    .node {
      transition: none;
    }
  }
</style>
