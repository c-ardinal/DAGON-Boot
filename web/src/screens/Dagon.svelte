<script lang="ts">
  import { tick } from 'svelte';
  import type { Agent, AskKind } from '../app/agent.svelte.ts';
  import { AGENT } from '../app/copy.ts';
  import { rise, tokenMs } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    agent: Agent;
    /** A demo: the mark stays, its bubble says it does not talk. */
    demo?: boolean;
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
    /** The bubble is open; the page closes it on Escape wherever focus is. */
    open?: boolean;
  }

  let { agent, demo = false, place, onask, onapply, ontalk, openRequest, open = $bindable(false) }: Props = $props();

  const MARGIN = 12;
  const DRAG_PX = 4;

  type Point = { x: number; y: number };
  /** A place as the zoom pill has one: from the free area's bottom-right corner, so the opening drawer carries it up. */
  type Corner = { r: number; b: number };

  let root = $state<HTMLDivElement>();
  let bubbleEl = $state<HTMLElement>();
  let field = $state<HTMLInputElement>();
  let text = $state('');
  let answered = $state(false);
  /** Where the user put it; null: the default place, kept by the stylesheet. Only a drag changes it. */
  let pos = $state<Corner | null>(null);
  let stage = $state({ w: 0, h: 0, mark: 0 });
  /** The bubble's corner, from the mark's. */
  let bubbleAt = $state<Point>({ x: 0, y: 0 });
  let bubbleMax = $state(0);

  const key = $derived(`dagon-pos:${place}`);
  const last = $derived(agent.messages.map((m) => m.role).lastIndexOf('dagon'));
  const reply = $derived(last < 0 ? null : agent.messages[last]);

  function inStage(p: Point): Point {
    return {
      x: Math.min(Math.max(MARGIN, p.x), Math.max(MARGIN, stage.w - stage.mark - MARGIN)),
      y: Math.min(Math.max(MARGIN, p.y), Math.max(MARGIN, stage.h - stage.mark - MARGIN)),
    };
  }
  const toPoint = (c: Corner): Point => ({ x: stage.w - stage.mark - c.r, y: stage.h - stage.mark - c.b });
  const toCorner = (p: Point): Corner => ({ r: stage.w - stage.mark - p.x, b: stage.h - stage.mark - p.y });
  /** The kept place is shown inside a smaller window without being forgotten: a larger one brings it back. */
  const shown = $derived(pos && (stage.w ? toCorner(inStage(toPoint(pos))) : pos));

  $effect(() => {
    pos = readPlace(key);
  });
  function readPlace(k: string): Corner | null {
    try {
      const kept = JSON.parse(localStorage.getItem(k) ?? 'null');
      return Number.isFinite(kept?.r) && Number.isFinite(kept?.b) ? { r: kept.r, b: kept.b } : null;
    } catch {
      // Storage may be off: the place then lasts for this page only.
      return null;
    }
  }

  /** What the mark's drop and the bubble keep off on the stage, in the coordinates of the free area the mark stands in. */
  function obstacles(cards = true): { left: number; top: number; right: number; bottom: number }[] {
    const scope = root?.closest('.stage');
    if (!scope || !root?.parentElement) return [];
    const o = root.parentElement.getBoundingClientRect();
    return [...scope.querySelectorAll<HTMLElement>('[data-avoid], .toast')]
      .filter((e) => !root?.contains(e) && (cards || !e.closest('main')))
      .map((e) => e.getBoundingClientRect())
      .map((r) => ({ left: r.left - o.left, top: r.top - o.top, right: r.right - o.left, bottom: r.bottom - o.top }));
  }
  type Box = Point & { w: number; h: number };
  const covers = (r: { left: number; top: number; right: number; bottom: number }, b: Box) => b.x < r.right && b.x + b.w > r.left && b.y < r.bottom && b.y + b.h > r.top;

  /** A drop: inside the stage and off every obstacle, moved sideways (then up) until it is free. */
  function settle(p: Point): Point {
    const s = stage.mark;
    let q = inStage(p);
    for (let i = 0; i < 6; i++) {
      const o = obstacles().find((r) => covers(r, { ...q, w: s, h: s }));
      if (!o) break;
      const left = o.left - s - MARGIN;
      const right = o.right + MARGIN;
      q = inStage(left >= MARGIN ? { x: left, y: q.y } : right + s <= stage.w - MARGIN ? { x: right, y: q.y } : { x: q.x, y: o.top - s - MARGIN });
    }
    return q;
  }

  /** Beside the mark where that is free; else the free spot nearest it, over the cards when the windows leave no other: nothing of the bubble lies over the sheet or a toast, wherever the mark is. */
  function placeBubble() {
    if (!open || !bubbleEl || !root) return;
    const [mx, my, s] = [root.offsetLeft, root.offsetTop, root.offsetWidth];
    // A low window: the bubble is no taller than the free area, its body scrolls.
    bubbleMax = Math.max(0, stage.h - 2 * MARGIN);
    const [w, h] = [bubbleEl.offsetWidth, Math.min(bubbleEl.offsetHeight, bubbleMax)];
    const sides: [number, number][] = [
      [mx - MARGIN - w, my + s - h],
      [mx - MARGIN - w, my],
      [mx + s + MARGIN, my + s - h],
      [mx + s + MARGIN, my],
    ];
    const spot = (cards: boolean): [number, number] | undefined => {
      // A gap around each: the bubble's measured width is whole pixels, the boxes are not.
      const obs = [...obstacles(cards).map((r) => ({ left: r.left - MARGIN, top: r.top - MARGIN, right: r.right + MARGIN, bottom: r.bottom + MARGIN })), { left: mx, top: my, right: mx + s, bottom: my + s }];
      const free = (x: number, y: number) => x >= MARGIN && y >= MARGIN && x + w <= stage.w - MARGIN && y + h <= stage.h - MARGIN && !obs.some((r) => covers(r, { x, y, w, h }));
      const beside = sides.find(([x, y]) => free(x, y));
      if (beside) return beside;
      let best: [number, number] | undefined;
      let nearest = Infinity;
      const step = s / 3;
      for (let x = MARGIN; x + w <= stage.w - MARGIN; x += step) {
        for (let y = MARGIN; y + h <= stage.h - MARGIN; y += step) {
          const d = Math.hypot(x + w / 2 - mx - s / 2, y + h / 2 - my - s / 2);
          if (d < nearest && free(x, y)) [nearest, best] = [d, [x, y]];
        }
      }
      return best;
    };
    const [x, y] = spot(true) ?? spot(false) ?? sides[0];
    bubbleAt = { x: x - mx, y: y - my };
  }

  $effect(() => {
    void open;
    void bubbleEl;
    void shown;
    void tick().then(placeSoon);
  });
  /** A sheet or toast still rising measures small: look again when it has settled. */
  let settled = 0;
  function placeSoon() {
    placeBubble();
    clearTimeout(settled);
    settled = window.setTimeout(placeBubble, tokenMs('--dur-line'));
  }

  /** The window and what stands on the stage change: the mark stays; the bubble finds its side again. */
  $effect(() => {
    const st = root?.parentElement;
    const scope = root?.closest('.stage');
    if (!st || !scope || !root) return;
    const mark = root;
    const sync = () => {
      stage = { w: st.clientWidth, h: st.clientHeight, mark: mark.offsetWidth };
      // A sheet or toast growing with its content is no mutation of the stage: watched for size too.
      for (const e of scope.querySelectorAll('.float, .toast')) ro.observe(e);
      placeSoon();
    };
    // Next frame: changing a box inside the observer's callback would report a resize loop.
    let frame = 0;
    const later = () => (cancelAnimationFrame(frame), (frame = requestAnimationFrame(sync)));
    const ro = new ResizeObserver(later);
    ro.observe(st);
    const mo = new MutationObserver(later);
    mo.observe(scope, { subtree: true, childList: true });
    sync();
    return () => (cancelAnimationFrame(frame), clearTimeout(settled), ro.disconnect(), mo.disconnect());
  });

  let drag: { id: number; dx: number; dy: number; x0: number; y0: number; moved: boolean; at: Point } | null = null;
  /** The click that ends a drag opens nothing. */
  let dragged = false;
  function down(e: PointerEvent) {
    if (!root || e.button !== 0) return;
    dragged = false;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, dx: e.clientX - root.offsetLeft, dy: e.clientY - root.offsetTop, x0: e.clientX, y0: e.clientY, moved: false, at: { x: root.offsetLeft, y: root.offsetTop } };
  }
  function move(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    if (Math.hypot(e.clientX - drag.x0, e.clientY - drag.y0) > DRAG_PX) drag.moved = true;
    if (!drag.moved) return;
    drag.at = { x: e.clientX - drag.dx, y: e.clientY - drag.dy };
    pos = toCorner(drag.at);
  }
  function up(e: PointerEvent) {
    if (!drag || e.pointerId !== drag.id) return;
    dragged = drag.moved;
    const at = drag.at;
    drag = null;
    if (!dragged) return;
    pos = toCorner(settle(at));
    try {
      localStorage.setItem(key, JSON.stringify(pos));
    } catch {
      // Storage may be off: the place then lasts for this page only.
    }
  }
  function press() {
    if (!dragged) open = !open;
    dragged = false;
  }

  $effect(() => {
    if (openRequest === 0) return;
    open = true;
    void tick().then(() => field?.focus());
  });

  /** A reply came: a 1.2 s blue edge, then calm again. */
  const REPLIED_MS = 1200;
  let seen = 0;
  $effect(() => {
    const n = agent.messages.length;
    if (n <= seen) return void (seen = n);
    seen = n;
    if (agent.messages[n - 1].role !== 'dagon') return;
    open = true;
    answered = true;
    const t = setTimeout(() => (answered = false), REPLIED_MS);
    return () => clearTimeout(t);
  });

  /** Escape closes the bubble and gives the focus back to the mark. */
  let markEl = $state<HTMLButtonElement>();
  function keys(e: KeyboardEvent) {
    if (e.key !== 'Escape' || !open) return;
    e.stopPropagation();
    open = false;
    markEl?.focus();
  }

  function send(e: SubmitEvent) {
    e.preventDefault();
    const q = text.trim();
    if (!q) return;
    onask(q, 'free', e.submitter ?? (e.currentTarget as Element));
    text = '';
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="dagon" class:thinking={agent.thinking} class:answered bind:this={root} style:right={shown && `${shown.r}px`} style:bottom={shown && `${shown.b}px`} data-avoid="" onkeydown={keys}>
  <button class="mark" bind:this={markEl} aria-label={AGENT.name} aria-expanded={open} title={open ? AGENT.markClose : AGENT.markOpen} onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={() => (drag = null)} onclick={press}>
    <span class="shadow" aria-hidden="true"></span>
    <img src="./icons/dagon-256.png" srcset="./icons/dagon-256.png 256w, ./icons/dagon-512.png 512w" sizes="96px" alt="" draggable="false" />
  </button>

  {#if open}
    <section transition:rise={{ dur: '--dur-quick' }} class="bubble" style:left="{bubbleAt.x}px" style:top="{bubbleAt.y}px" style:max-height={bubbleMax ? `${bubbleMax}px` : null} aria-label={AGENT.name} aria-live="polite" bind:this={bubbleEl}>
      {#if demo}
        <p>{AGENT.demoBubble}</p>
      {:else if agent.thinking}
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
      {#if !demo}
        <div class="qs">
          {#each AGENT.questions as [kind, q] (kind)}
            <button class="q" data-control="ask" onclick={(e) => onask(q, kind, e.currentTarget)}>{q}</button>
          {/each}
        </div>
        <form onsubmit={send} data-control="ask">
          <input bind:this={field} bind:value={text} aria-label={AGENT.question} placeholder={AGENT.placeholder} maxlength="2000" />
          <button class="button-icon send" type="submit" aria-label={AGENT.send} title={text.trim() ? AGENT.send : AGENT.sendWhy} aria-disabled={!text.trim()}><span class="layer"><Icon name="send-horizontal" /></span></button>
        </form>
      {/if}
    </section>
  {/if}
</div>

<style>
  /* In the free area over the canvas, as the zoom pill is: by default just left of the zoom, its foot on the zoom's foot, in every view; the opening drawer carries it up. A drag's place comes as right and bottom. */
  .dagon {
    position: absolute;
    z-index: 1;
    right: calc(var(--m-edge) + var(--h-button) + var(--s-3));
    bottom: var(--m-edge);
    pointer-events: auto;
    width: var(--w-mark);
    height: var(--w-mark);
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
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
    border-radius: var(--r-m);
  }
  .mark img {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
    user-select: none;
    animation: float var(--dur-float) var(--ease-in-out) infinite alternate;
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
    animation: breathe var(--dur-float) var(--ease-in-out) infinite alternate;
  }
  .answered .shadow {
    background: radial-gradient(closest-side, color-mix(in srgb, var(--c-accent) 55%, transparent), transparent);
  }
  .answered img {
    filter: drop-shadow(0 0 0.4rem color-mix(in srgb, var(--c-accent) 70%, transparent));
  }
  .thinking img {
    animation:
      float var(--dur-float) var(--ease-in-out) infinite alternate,
      think var(--dur-think) var(--ease-in-out) infinite alternate;
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
  /* Its place comes from `placeBubble`. */
  .bubble {
    position: absolute;
    overflow: auto;
    width: var(--w-bubble);
    display: grid;
    gap: var(--s-2);
    padding: var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.45);
    transform-origin: right bottom;
  }
  .bubble p {
    margin: 0;
    font: 400 var(--t-caption) / 16px var(--f-ja);
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
    font: 400 var(--t-caption) / 16px var(--f-ja);
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
    font: 400 var(--t-caption) / 1 var(--f-ja);
  }
  .small {
    height: 2rem;
    padding: 0 var(--s-3);
    font-size: var(--t-caption);
  }
  @media (max-width: 767px) {
    .q,
    .small,
    input {
      min-height: 2.75rem;
    }
  }
</style>
