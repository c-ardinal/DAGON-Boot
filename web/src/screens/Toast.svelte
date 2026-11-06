<script lang="ts" module>
  /** A non-blocking notice: a short word, and the place its detail is kept. */
  export interface Notice {
    word: string;
    error?: boolean;
    /** Says more than the word (tooltip and screen readers). */
    detail?: string;
  }

  /** A toast leaves after 5 s, the upper limit of reading one line; pointing at it holds it. */
  export const TOAST_MS = 5000;
</script>

<script lang="ts">
  import { NOTICE } from '../app/copy.ts';
  import { rise } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    notice: Notice;
    /** 詳しく見る: opens the record behind the notice. */
    onmore?: () => void;
    ondone: () => void;
  }

  let { notice, onmore, ondone }: Props = $props();
  let held = $state(false);

  $effect(() => {
    void notice;
    if (held) return;
    const t = setTimeout(ondone, TOAST_MS);
    return () => clearTimeout(t);
  });
</script>

<!-- Toast: at the foot of the canvas's middle column, so right beside the zoom pill; it rises once as it arrives and sinks as it goes, and never moves to catch the eye. -->
<div
  transition:rise
  class="toast"
  class:error={notice.error}
  role={notice.error ? 'alert' : 'status'}
  title={notice.detail}
  onpointerenter={() => (held = true)}
  onpointerleave={() => (held = false)}
  onfocusin={() => (held = true)}
  onfocusout={() => (held = false)}
>
  <Icon name={notice.error ? 'octagon-alert' : 'check'} />
  <span class="word">{notice.word}</span>
  {#if notice.detail}<span class="visually-hidden">{notice.detail}</span>{/if}
  {#if onmore}<button class="more" onclick={onmore}>{NOTICE.more}</button>{/if}
</div>

<style>
  .toast {
    display: flex;
    flex: none;
    align-items: center;
    align-self: flex-end;
    gap: var(--s-3);
    width: min(var(--w-toast), 100%);
    pointer-events: auto;
    padding: var(--s-3) var(--s-4);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    color: var(--c-paper);
  }
  .toast.error {
    border: var(--st-w-strong) solid var(--st-error);
  }
  .toast.error :global(.icon) {
    color: var(--st-error);
  }
  .word {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font: 700 var(--t-body) / 24px var(--f-ja);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .more {
    flex: none;
    white-space: nowrap;
    height: var(--h-text-button);
    padding: 0;
    border: 0;
    background: none;
    color: var(--c-accent);
    font: 700 var(--t-small) / 1 var(--f-ja);
    cursor: pointer;
  }
  .more:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
</style>
