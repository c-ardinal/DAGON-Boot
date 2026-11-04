<script lang="ts">
  import { AGENT, HELP, MENU, START } from '../app/copy.ts';
  import type { StartChoice } from '../app/start.svelte.ts';

  interface Props {
    /** 新しく作る / 見本から新しく作る / 開く: all go to SC-S0 (the project is saved, nothing to confirm). */
    onstart: (c: StartChoice) => void;
    /** "最初からやり直す": asks first (M53). */
    onrestart: () => void;
    /** Held while an update or recovery runs. */
    locked: boolean;
    restartButton?: HTMLButtonElement;
    /** DAGON: not linked, "DAGON と連携する…"; linked, "DAGON と話す" and "DAGON との連携を外す…". */
    agentLinked: boolean;
    onlink: (at: Element) => void;
    ontalk: () => void;
    onunlink: () => void;
    /** 困ったとき (on phones the bar has no room for its button). */
    onhelp: () => void;
  }

  let { onstart, onrestart, locked, restartButton = $bindable(), agentLinked, onlink, ontalk, onunlink, onhelp }: Props = $props();
  const ITEMS: [StartChoice, string][] = [
    ['new', START.new],
    ['sample', START.sample],
    ['open', START.open],
  ];
</script>

<!-- SC-M: a fold under the header that pushes the content down; the right column stays as it was. -->
<nav id="menu" class="menu" aria-label={MENU.button}>
  <ul>
    {#each ITEMS as [c, label] (c)}
      <li><button class="secondary" data-control="project" aria-disabled={locked} onclick={() => !locked && onstart(c)}>{label}</button></li>
    {/each}
    <li><button class="secondary" data-control="project" aria-disabled={locked} bind:this={restartButton} onclick={() => !locked && onrestart()}>{MENU.restart}</button></li>
    <li class="help"><button class="secondary" onclick={onhelp}>{HELP.heading}</button></li>
    {#if agentLinked}
      <li class="agent"><button class="secondary" data-control="ask" onclick={ontalk}>{AGENT.menuTalk}</button></li>
      <li><button class="secondary" onclick={onunlink}>{AGENT.menuUnlink}</button></li>
    {:else}
      <li class="agent"><button class="secondary" data-control="link" onclick={(e) => onlink(e.currentTarget)}>{AGENT.menuLink}</button></li>
    {/if}
  </ul>
</nav>

<style>
  .menu {
    grid-area: menu;
    padding: var(--s-4) var(--s-5);
    border-bottom: 1px solid var(--c-line);
    background: var(--c-layer-1);
  }
  ul {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .agent {
    margin-left: auto;
  }
  /* The bar shows 困ったとき on wider windows; the menu carries it on phones. */
  @media (min-width: 768px) {
    .help {
      display: none;
    }
  }
</style>
