<script lang="ts">
  import { tick } from 'svelte';
  import type { Agent, AskKind } from '../app/agent.svelte.ts';
  import { AGENT } from '../app/copy.ts';

  interface Props {
    agent: Agent;
    /** Where the position is kept: one per project. */
    place: string;
    /** Sends a question (the lock table may stop it: `ask`). */
    onask: (text: string, kind: AskKind, at: Element) => void;
    /** Puts message `i`'s proposal into the 構成 (lock table: `apply`). */
    onapply: (i: number, at: Element) => void;
    /** Opens the drawer's 会話 tab. */
    ontalk: () => void;
    /** Bumped from outside (the menu's "DAGON と話す", the / key): opens the bubble and focuses its field. */
    openRequest: number;
  }

  let { agent, place, onask, onapply, ontalk, openRequest }: Props = $props();

  /** DES-UX3-186: 72 px at 1440×900 (the root's 4.5 rem), never under 66 px. */
  const MIN_MARK = 66;
  const MARGIN = 12;
  const DRAG_PX = 4;

  let root = $state<HTMLDivElement>();
  let field = $state<HTMLInputElement>();
  let mark = $state(72);
  let pos = $state<{ x: number; y: number } | null>(null);
  let open = $state(true);
  let text = $state('');
  let answered = $state(false);
  let bubbleEl = $state<HTMLElement>();
  /** Where the bubble opens: left or right of the character, and up from its bottom or down from its top. */
  let side = $state({ left: true, up: true });
  /** The user dragged it: it stays there (stepping aside only when covered); else it picks a free spot itself. */
  let auto = true;

  const key = $derived(`dagon-pos:${place}`);
  const last = $derived(agent.messages.map((m) => m.role).lastIndexOf('dagon'));
  const reply = $derived(last < 0 ? null : agent.messages[last]);

  /** The stage it lives on, and what it must not cover there (windows, the focal number, the selected node). */
  function stageRect(): DOMRect | null {
    return root?.parentElement?.getBoundingClientRect() ?? null;
  }
  function obstacles(): DOMRect[] {
    const stage = root?.parentElement;
    return stage ? [...stage.querySelectorAll<HTMLElement>('[data-avoid]')].filter((e) => !root?.contains(e)).map((e) => e.getBoundingClientRect()) : [];
  }
  const hits = (a: DOMRect, x: number, y: number, s: number) => x < a.right && x + s > a.left && y < a.bottom && y + s > a.top;

  /** Inside the stage and off every obstacle: moved sideways (then up) until it is free. */
  function settle(p: { x: number; y: number }): { x: number; y: number } {
    const st = stageRect();
    if (!st) return p;
    const s = mark;
    const clamp = (q: { x: number; y: number }) => ({
      x: Math.min(Math.max(MARGIN, q.x), Math.max(MARGIN, st.width - s - MARGIN)),
      y: Math.min(Math.max(MARGIN, q.y), Math.max(MARGIN, st.height - s - MARGIN)),
    });
    let q = clamp(p);
    for (let i = 0; i < 6; i++) {
      const o = obstacles().find((r) => hits(r, st.left + q.x, st.top + q.y, s));
      if (!o) break;
      const left = o.left - st.left - s - MARGIN;
      const right = o.right - st.left + MARGIN;
      const up = o.top - st.top - s - MARGIN;
      q = clamp(left >= MARGIN ? { x: left, y: q.y } : right + s <= st.width - MARGIN ? { x: right, y: q.y } : { x: q.x, y: up });
    }
    return q;
  }

  type Box = { x: number; y: number; w: number; h: number };
  const free = (b: Box, st: DOMRect, obs: DOMRect[]) =>
    b.x >= MARGIN && b.y >= MARGIN && b.x + b.w <= st.width - MARGIN && b.y + b.h <= st.height - MARGIN && !obs.some((r) => b.x + st.left < r.right && b.x + st.left + b.w > r.left && b.y + st.top < r.bottom && b.y + st.top + b.h > r.top);
  /** The bubble's box for a side, beside a character at (x, y). */
  function bubbleBox(x: number, y: number, s: { left: boolean; up: boolean }): Box {
    const rem = mark / 4.5;
    const w = bubbleEl?.offsetWidth ?? 19 * rem;
    const h = bubbleEl?.offsetHeight ?? 11 * rem;
    const gap = 0.75 * rem;
    return { x: s.left ? x - gap - w : x + mark + gap, y: s.up ? y + mark - h : y, w, h };
  }
  const SIDES = [
    { left: true, up: true },
    { left: true, up: false },
    { left: false, up: true },
    { left: false, up: false },
  ];
  /** A spot where neither the character nor its open bubble covers a card, a window or the focal number (the mock's freeSpot). */
  function freeSpot(): { x: number; y: number } | null {
    const st = stageRect();
    if (!st) return null;
    const obs = obstacles();
    const step = mark / 4;
    for (let y = st.height - mark - 3 * MARGIN; y >= MARGIN; y -= step) {
      for (let x = st.width - mark - 3 * MARGIN; x >= MARGIN; x -= step) {
        if (!free({ x, y, w: mark, h: mark }, st, obs)) continue;
        const s = SIDES.find((v) => !open || free(bubbleBox(x, y, v), st, obs));
        if (s) {
          side = s;
          return { x, y };
        }
      }
    }
    return null;
  }
  /** For a place the user chose: the side where the bubble covers the least. */
  function chooseSide(p: { x: number; y: number }) {
    const st = stageRect();
    if (!st) return;
    const obs = obstacles();
    side = SIDES.find((v) => free(bubbleBox(p.x, p.y, v), st, obs)) ?? { left: p.x > st.width / 2, up: p.y > st.height / 2 };
  }

  function measure() {
    mark = Math.max(MIN_MARK, 4.5 * (parseFloat(getComputedStyle(document.documentElement).fontSize) || 16));
  }

  /** First place: the kept one, else bottom-right of the stage, clear of the windows. */
  $effect(() => {
    measure();
    let kept: { x: number; y: number } | null = null;
    try {
      kept = JSON.parse(localStorage.getItem(key) ?? 'null');
    } catch {
      // Storage may be off: the place then lasts for this page only.
    }
    const st = stageRect();
    auto = !(kept && Number.isFinite(kept.x));
    const start = kept && Number.isFinite(kept.x) ? kept : { x: (st?.width ?? 800) - mark - 3 * MARGIN, y: (st?.height ?? 600) * 0.55 };
    void tick().then(place);
    function place() {
      pos = (auto && freeSpot()) || settle(start);
      if (!auto) chooseSide(pos);
    }
  });

  /** Windows open and close, the selection moves: step aside again. */
  $effect(() => {
    const st = root?.parentElement;
    if (!st) return;
    const again = () => {
      if (!pos) return;
      pos = (auto && freeSpot()) || settle(pos);
      if (!auto) chooseSide(pos);
    };
    // Next frame: moving inside the observer's callback would report a resize loop.
    let frame = 0;
    const later = () => (cancelAnimationFrame(frame), (frame = requestAnimationFrame(again)));
    const ro = new ResizeObserver(later);
    ro.observe(st);
    const mo = new MutationObserver(later);
    mo.observe(st, { subtree: true, childList: true, attributeFilter: ['data-avoid'] });
    const resize = () => (measure(), again());
    addEventListener('resize', resize);
    return () => (cancelAnimationFrame(frame), ro.disconnect(), mo.disconnect(), removeEventListener('resize', resize));
  });

  $effect(() => {
    if (openRequest === 0) return;
    open = true;
    void tick().then(() => field?.focus());
  });

  /** A reply came: a 1.2 s blue edge (DES-UX3-170), then calm again. */
  const REPLIED_MS = 1200;
  let seen = 0;
  $effect(() => {
    const n = agent.messages.length;
    if (n <= seen) return void (seen = n);
    seen = n;
    if (agent.messages[n - 1].role !== 'dagon') return;
    answered = true;
    const t = setTimeout(() => (answered = false), REPLIED_MS);
    return () => clearTimeout(t);
  });

  let drag: { id: number; dx: number; dy: number; x0: number; y0: number; moved: boolean } | null = null;
  function down(e: PointerEvent) {
    if (!pos || e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, dx: e.clientX - pos.x, dy: e.clientY - pos.y, x0: e.clientX, y0: e.clientY, moved: false };
  }
  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    if (Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0) > DRAG_PX) drag.moved = true;
    if (drag.moved) pos = { x: e.clientX - drag.dx, y: e.clientY - drag.dy };
  }
  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    const moved = drag.moved;
    drag = null;
    if (!moved) return void (open = !open);
    auto = false;
    pos = settle(pos!);
    chooseSide(pos);
    try {
      localStorage.setItem(key, JSON.stringify(pos));
    } catch {
      // Storage may be off: the place then lasts for this page only.
    }
  }

  function send(e: SubmitEvent) {
    e.preventDefault();
    const q = text.trim();
    if (!q) return;
    onask(q, 'free', e.submitter ?? (e.currentTarget as Element));
    text = '';
  }

</script>

<div
  class="dagon"
  class:thinking={agent.thinking}
  class:answered
  bind:this={root}
  style:left="{pos?.x ?? -999}px"
  style:top="{pos?.y ?? -999}px"
  style:--mark="{mark}px"
  data-avoid=""
>
  <button
    class="mark"
    aria-label={AGENT.name}
    aria-expanded={open}
    title={open ? AGENT.markClose : AGENT.markOpen}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), (open = !open))}
  >
    <span class="shadow" aria-hidden="true"></span>
    <img src="./icons/dagon-256.png" srcset="./icons/dagon-256.png 256w, ./icons/dagon-512.png 512w" sizes="{Math.round(mark)}px" alt="" draggable="false" />
  </button>

  {#if open}
    <section class="bubble" class:left={side.left} class:up={side.up} aria-label={AGENT.name} aria-live="polite" bind:this={bubbleEl}>
      {#if agent.thinking}
        <p class="muted">{AGENT.thinking}</p>
      {:else if reply}
        <p class:failed={reply.failed}>{reply.text}</p>
        {#if reply.proposal || reply.more}
          <div class="acts">
            <button class="secondary small" onclick={ontalk}>{reply.proposal ? AGENT.showDiff : AGENT.allTalk}</button>
            {#if reply.proposal && !reply.applied}
              <button class="primary small" data-control="apply" onclick={(e) => onapply(last, e.currentTarget)}>{AGENT.apply}</button>
            {/if}
          </div>
        {/if}
      {:else}
        <p class="muted">{AGENT.hello}</p>
      {/if}
      <div class="qs">
        {#each AGENT.questions as [kind, q] (kind)}
          <button class="q" data-control="ask" onclick={(e) => onask(q, kind, e.currentTarget)}>{q}</button>
        {/each}
      </div>
      <form onsubmit={send} data-control="ask">
        <input bind:this={field} bind:value={text} aria-label={AGENT.question} placeholder={AGENT.placeholder} maxlength="2000" />
        <button class="secondary small" type="submit">{AGENT.send}</button>
      </form>
    </section>
  {/if}
</div>

<style>
  .dagon {
    position: absolute;
    z-index: var(--z-chrome);
    width: var(--mark);
    height: var(--mark);
  }
  .mark {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    cursor: grab;
    touch-action: none;
  }
  .mark:active {
    cursor: grabbing;
  }
  .mark:focus-visible {
    outline: 2px solid var(--c-paper);
    outline-offset: 4px;
    border-radius: var(--r-m);
  }
  .mark img {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
    user-select: none;
    animation: float 3.6s var(--ease-in-out) infinite alternate;
  }
  /* A soft shadow on the canvas: darker and smaller as DAGON sinks, lighter and wider as it rises. */
  .shadow {
    position: absolute;
    left: 18%;
    right: 18%;
    bottom: -10%;
    height: 14%;
    border-radius: 50%;
    background: radial-gradient(closest-side, rgb(0 0 0 / 0.75), transparent);
    animation: breathe 3.6s var(--ease-in-out) infinite alternate;
  }
  .answered .shadow {
    background: radial-gradient(closest-side, color-mix(in srgb, var(--c-accent) 55%, transparent), transparent);
  }
  .answered img {
    filter: drop-shadow(0 0 0.4rem color-mix(in srgb, var(--c-accent) 70%, transparent));
  }
  .thinking img {
    animation:
      float 3.6s var(--ease-in-out) infinite alternate,
      think 1.2s ease-in-out infinite alternate;
  }
  @keyframes float {
    to {
      transform: translateY(-4px);
    }
  }
  @keyframes breathe {
    to {
      transform: scaleX(1.18);
      opacity: 0.55;
    }
  }
  @keyframes think {
    to {
      opacity: 0.55;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .mark img,
    .thinking img,
    .shadow {
      animation: none;
    }
  }
  .bubble {
    position: absolute;
    top: 0;
    left: calc(100% + var(--s-3));
    width: min(19rem, calc(100vw - var(--mark) - 3rem));
    display: grid;
    gap: var(--s-2);
    padding: var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    box-shadow: 0 0.5rem 1.5rem rgb(0 0 0 / 0.45);
    animation: pop 160ms var(--ease-out);
    transform-origin: left top;
  }
  .bubble.left {
    left: auto;
    right: calc(100% + var(--s-3));
    transform-origin: right top;
  }
  .bubble.up {
    top: auto;
    bottom: 0;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .bubble {
      animation: none;
    }
  }
  .bubble p {
    margin: 0;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .muted {
    color: var(--c-muted);
  }
  .failed {
    color: var(--c-signal);
  }
  .acts,
  .qs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
  .q {
    padding: var(--s-1) var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: none;
    color: var(--c-paper);
    font: 400 var(--t-caption-size) / 1.4 var(--f-ja);
    cursor: pointer;
    transition: transform var(--dur-press) var(--ease-out);
  }
  .q:active {
    transform: scale(0.97);
  }
  form {
    display: flex;
    gap: var(--s-2);
  }
  input {
    flex: 1;
    min-width: 0;
    min-height: 2rem;
    padding: 0 var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 400 var(--t-small-size) / 1 var(--f-ja);
  }
  .small {
    height: 2rem;
    padding: 0 var(--s-3);
    font-size: var(--t-small-size);
  }
  @media (max-width: 767px) {
    .q,
    .small,
    input {
      min-height: 2.75rem;
    }
  }
</style>
