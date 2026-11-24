<script lang="ts" module>
  import { kindLabel } from '../app/define/model.ts';
  import { isHub, type Config, type Node } from '../app/sample.ts';
  import { NODE } from '../app/copy.ts';

  /** A coordinator's own connection line: its uplink as the 構成 writes it, or the link from the node above it. */
  function hubVia(n: Node): string {
    if (!n.uplink) return n.via || NODE.usb;
    return NODE.uplinkVia(kindLabel(n.uplink.kind), n.uplink.host);
  }

  /** The card's one mono line: MCU and how it is reached. */
  const metaOf = (n: Node) => [n.mcu, isHub(n) ? hubVia(n) : n.via].filter(Boolean).join(' · ');

  /** The role on the card: coordinator; links from the nearest coordinator above; or ノード when no coordinator is above it. */
  function roleOf(config: Config | null, n: Node): string {
    if (isHub(n)) return NODE.roleHub;
    if (!underCoordinator(config, n.id)) return NODE.roleOther;
    const d = belowHub(config, n.id);
    return d <= 1 ? NODE.roleNode : d === 2 ? NODE.roleGrandchild : NODE.roleDeeper(d);
  }

  /** Links from the nearest coordinator above node `id` (1 for a node; a node no coordinator reaches counts as a node). */
  function belowHub(config: Config | null, id: string, seen = new Set<string>()): number {
    const up = config?.links.find((l) => l.to === id)?.from;
    const p = config?.nodes.find((x) => x.id === up);
    if (!p || isHub(p) || seen.has(id)) return 1;
    seen.add(id);
    return belowHub(config, p.id, seen) + 1;
  }

  /** Whether the links above node `id` lead up to a coordinator (not to a node nobody links to, nor round a loop). */
  function underCoordinator(config: Config | null, id: string, seen = new Set<string>()): boolean {
    const up = config?.links.find((l) => l.to === id)?.from;
    const p = config?.nodes.find((x) => x.id === up);
    if (!p || seen.has(id)) return false;
    if (isHub(p)) return true;
    seen.add(id);
    return underCoordinator(config, p.id, seen);
  }
</script>

<script lang="ts">
  // One node's card on the canvas: name, role, version, state mark, product, one mono line, the slot line, chips and the water.
  import { SLOT, STATE, WRITE } from '../app/copy.ts';
  import { fade, lateFade, rise, waterEnd } from '../app/motion.ts';
  import { STATE_ICON, type NodeState } from '../app/nodeState.ts';
  import type { Live } from '../app/writeProgress.ts';
  import { imageRows } from '../app/slots.ts';
  import Icon from '../screens/Icon.svelte';
  import { SLOT_STATE, versionText } from '../wasm/dagon.ts';
  import type { Chip } from './Graph.svelte';
  import type { Pt } from './geometry.ts';

  interface Props {
    n: Node;
    /** The 構成 the card stands in: its role counts the links above it. */
    config: Config | null;
    live: Live | undefined;
    /** The page's chip for this node (update progress), over the watch result. */
    chip: Chip | undefined;
    connected: boolean;
    compact: boolean;
    state: NodeState;
    /** The edge light stands still (reduced motion, or another node needs attention). */
    still: boolean;
    /** How full the water is, 0–100, while the node is written. */
    level: number | undefined;
    /** The node confirmed its new version: the water rises to the top as it goes. */
    confirmed: boolean;
    dragging: boolean;
    /** A dragged part would go under this node. */
    drop: boolean;
    /** 書込: the card's check (true: it is written next); none in 準備. */
    check: boolean | undefined;
    /** The check stays as it is (a write runs). */
    checksLocked: boolean;
    oncheck: () => void;
    at: Pt;
    width: number;
    key: string;
    /** The canvas's focus stop. */
    focused: boolean;
    pressed: boolean;
    /** The card's measured height. */
    height?: number;
    onpointerdown: (e: PointerEvent) => void;
    onclick: () => void;
  }

  let { n, config, live, chip, connected, compact, state, still, level, confirmed, dragging, drop, check, checksLocked, oncheck, at, width, key, focused, pressed, height = $bindable(), onpointerdown, onclick }: Props = $props();

  type Mark = Chip | 'skeleton' | null;

  const m: Mark = $derived.by(() => {
    if (n.skip) {
      const why = NODE.skippedWhy[n.skip] ?? '';
      return { chip: NODE.skipped, spoken: NODE.skippedName(why), silent: false, note: compact ? undefined : why };
    }
    if (chip) return chip;
    if (!connected) return null;
    if (!live || live.responding === null) return 'skeleton';
    // 応答あり・応答なし are the state mark itself: no chip repeats them.
    if (!live.responding) return { chip: '', spoken: NODE.silentName, silent: true };
    if (live.hello?.trial) return { chip: NODE.trial, spoken: NODE.trialName, silent: false };
    return { chip: '', spoken: NODE.respondingName, silent: false };
  });

  const v = $derived(connected && live?.hello ? versionText(live.hello.activeVersion) : '');

  /** The slot line (each app slot the node's definition gives, with its version, ▸ on the running one), from the node's last STATUS. */
  const slots = $derived.by((): { row: string; text: string; on: boolean }[] | null => {
    const st = connected ? live?.status : null;
    if (!st) return null;
    return imageRows(n.mcuId).map((i) => {
      const s = st.slots[i];
      return { row: SLOT.rows[i], text: s.state === SLOT_STATE.empty ? SLOT.states[SLOT_STATE.empty] : versionText(s.version), on: st.active === i };
    });
  });

  const role = $derived(roleOf(config, n));
  const meta = $derived(metaOf(n));

  function spokenName(): string {
    const mark = m && m !== 'skeleton' ? m : null;
    return NODE.name({
      no: n.no,
      name: n.name,
      role,
      version: v,
      product: n.product,
      meta,
      slots: (slots ?? []).map((sl) => `${sl.row} ${sl.text}`).join(' '),
      chip: mark?.chip ?? '',
      note: compact ? '' : (mark?.note ?? ''),
      // A state no chip shows (応答あり, 応答なし) is said after the words.
      state: mark && !mark.chip ? mark.spoken : '',
    });
  }
</script>

<button
  transition:rise
  class="node st-{state}"
  class:dragging
  class:still
  class:uns={!!n.skip}
  class:err={m !== null && m !== 'skeleton' && m.silent}
  class:drop
  class:checkable={check !== undefined}
  class:filling={level !== undefined}
  style:left="{at.x}px"
  style:top="{at.y}px"
  style:width="{width}px"
  data-key={key}
  data-avoid=""
  tabindex={focused ? 0 : -1}
  aria-pressed={pressed}
  aria-label="{spokenName()}、{STATE[state]}"
  bind:offsetHeight={height}
  {onpointerdown}
  {onclick}
>
  {#if level !== undefined}
    <!-- The water rises with the node's progress; its edge is two CSS-masked strips drifting at different speeds. -->
    <span in:fade out:lateFade class="water" aria-hidden="true">
      <span out:waterEnd={{ confirmed: confirmed }} class="level" style:transform="translateY({100 - Math.min(100, Math.max(0, level))}%)">
        <span class="crest back"></span><span class="crest front"></span>
      </span>
    </span>
  {/if}
  <span class="hd">
    <b>[{n.no}] {n.name}</b> <span class="role">{role}</span>
    {#if v}{#key v}<span in:rise={{ dur: '--dur-quick' }} class="ver" translate="no">{v}</span>{/key}{:else if m === 'skeleton'}<span class="skeleton ver-sk"></span>{/if}
    {#key state}<span in:rise={{ dur: '--dur-quick' }} class="state" title={STATE[state]}><Icon name={STATE_ICON[state]} size="var(--i-state)" /><span class="visually-hidden">{STATE[state]}</span></span>{/key}
  </span>
  <span class="bd">
    <span class="prod" translate="no" title={n.product}>{n.product}</span>
    <span class="meta" translate="no">{meta}</span>
    <!-- Its row, and the chip's below it, are always there: no card changes height when one fills. -->
    <span class="slotline" translate="no">{#each slots ?? [] as sl (sl.row)}<span class:on={sl.on}>{sl.row} {sl.text}</span> {/each}</span>
    <span class="chips"
      >{#if m === 'skeleton'}<span class="skeleton chip-sk"></span>{:else if m?.chip}<span in:rise={{ dur: '--dur-quick' }} out:lateFade class="chip" class:silent={m.silent} class:uns={!!n.skip && m.chip === NODE.skipped}>{m.chip}</span>{/if} {#if m && m !== 'skeleton' && m.note}<span class="note" title={m.note}>{m.note}</span>{/if}</span
    >
  </span>
  {#if level !== undefined}<span in:fade out:lateFade={{ shown: confirmed ? 100 : undefined }} class="pct" aria-hidden="true" style:--shown={Math.round(level)}></span>{/if}
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
    title={checksLocked ? WRITE.checksLockedWhy : undefined}
    data-avoid=""
    style:left="{at.x}px"
    style:top="{at.y}px"
    onclick={() => !checksLocked && oncheck()}><Icon name={check ? 'square-check' : 'square'} size="var(--i-state)" /></button
  >
{/if}

<style>
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
    font: 400 var(--t-small) / 20px var(--f-ja);
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
    font: 400 var(--t-version) / 24px var(--f-mono);
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
    font: 700 var(--t-caption) / 1 var(--f-ja);
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
    font: 500 var(--t-caption) / 20px var(--f-mono);
  }
  .slotline .on {
    color: var(--c-paper);
  }
  .slotline .on::before {
    content: '▸';
    margin-right: var(--s-1);
  }
  :global(.canvas.compact) .slotline {
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
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  /* The number counts up with the water: the browser moves --shown between the values it is given. */
  @property --shown {
    syntax: '<integer>';
    inherits: false;
    initial-value: 0;
  }
  .pct {
    position: absolute;
    right: var(--s-3);
    bottom: var(--s-3);
    font: 400 var(--t-version) / 24px var(--f-mono);
    counter-reset: shown var(--shown);
    transition: --shown var(--dur-rise) var(--ease-steady);
  }
  .pct::before {
    content: counter(shown) '%';
  }
  /* Compact (9 ノード and on, or when normal cards would shrink names under 13 px): name, version, state. */
  :global(.canvas.compact) .prod,
  :global(.canvas.compact) .meta,
  :global(.canvas.compact) .role,
  :global(.canvas.compact) .note {
    display: none;
  }
  :global(.canvas.compact) .hd {
    border-bottom: 0;
  }
  /* Past this zoom the name holds its 13 px on screen. */
  :global(.canvas.compact) .hd b {
    flex: 1;
    min-width: 0;
    font-size: max(var(--t-caption), calc(13 * var(--t-caption) / 12 / var(--zoom)));
  }
  :global(.canvas.compact) .bd {
    padding-top: 0;
  }
  :global(.canvas.compact) .chips {
    margin-top: var(--s-1);
  }

  /* Water gauge: the level moves by transform; the edge is a CSS mask (no drawn illustration). */
  /* The water lies under the card's words: muted words lighten on it to hold 4.5:1 (WCAG 1.4.3). */
  .node.filling :is(.role, .meta, .slotline, .note) {
    color: color-mix(in srgb, var(--c-paper) 50%, var(--c-muted));
  }
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
    top: -12px;
    background: var(--wave-front);
    animation: drift var(--dur-drift-front) var(--ease-steady) infinite;
  }
  .crest.back {
    top: -16px;
    background: var(--wave-back);
    animation: drift var(--dur-drift-back) var(--ease-steady) infinite reverse;
  }
  @keyframes drift {
    to {
      transform: translateX(-50%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .crest {
      animation: none;
    }
  }
</style>
