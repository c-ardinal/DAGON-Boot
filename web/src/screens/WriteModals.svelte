<script lang="ts">
  // The modals a write opens when it needs the user: writing directly over USB, a package that
  // cannot be written, a write that stopped at a node, the ROM write's consent, a connection to
  // fix and the reconnect after the automatic one failed.
  import type { OperateView } from '../app/console/operateView.svelte.ts';
  import type { WriteFlow } from '../app/console/writeFlow.svelte.ts';
  import { HUB_TOGGLE, NODE, NOTICE, WRITE } from '../app/copy.ts';
  import type { Cli } from '../app/define/cli.svelte.ts';
  import type { Action } from '../app/machine.ts';
  import type { Operate } from '../app/operate.svelte.ts';
  import { isHub } from '../app/sample.ts';
  import { mcuConnection } from '../wasm/dagon.ts';
  import BandButtons from './BandButtons.svelte';
  import FactoryModal from './FactoryModal.svelte';
  import Modal from './Modal.svelte';
  import Reconnect from './Reconnect.svelte';
  import RefusalModal from './RefusalModal.svelte';
  import RomConfirm from './RomConfirm.svelte';
  import StopModal from './StopModal.svelte';

  let {
    op,
    ov,
    writes,
    cli,
    fix,
    mcuOf,
    act,
    onmore,
    onreconnect,
  }: {
    op: Operate;
    ov: OperateView;
    writes: WriteFlow;
    cli: Cli;
    /** What to do, in one sentence (the status row's). */
    fix: string;
    mcuOf: (id: string) => string;
    act: (a: Action) => void;
    /** 詳細表示: the drawer's tab with the record behind the notice. */
    onmore: (tab: 'jobs' | 'log') => void;
    onreconnect: () => void;
  } = $props();

  const band = $derived(ov.band);
  const factoryMcu = $derived(mcuOf(writes.factoryNode?.id ?? ''));
  const stopNow = (a: Action) => ((ov.stopClosed = op.failed), act(a));
</script>

{#if writes.factory && writes.factoryNode}
  {@const n = writes.factoryNode}
  <FactoryModal
    node={{ label: `[${n.no}] ${n.name}`, target: `[${n.no}] ${n.name}（${isHub(n) ? NODE.roleHub : NODE.roleOther}）· ${n.product}` }}
    child={!isHub(n)}
    how={mcuConnection(factoryMcu)}
    canWrite={cli.canFlash(factoryMcu)}
    job={writes.factoryJob}
    onwrite={() => void writes.factoryWrite()}
    oncancel={() => (writes.factoryJob ? void cli.cancel(writes.factoryJob) : (writes.factory = null))}
    onclose={() => (writes.factory = null)}
    onmore={() => (onmore('jobs'), (writes.factory = null))}
  />
{/if}
{#if ov.refusalOpen && op.file}
  <RefusalModal file={op.file} diff={ov.fileDiff} {fix} secondary={band.secondary} primary={band.primary} onclose={() => (ov.refusalClosed = op.file)} onpress={(a) => ((ov.refusalClosed = op.file), act(a))} />
{/if}
{#if ov.stopOpen && ov.stopNode && !op.wentBack}
  <StopModal
    target={`[${ov.stopNode.no}] ${ov.stopNode.name}`}
    label={op.coreWentBack ? WRITE.coreBack.label : undefined}
    reason={op.coreWentBack ? WRITE.coreBack.detail : ov.failWhy.reason}
    fix={op.coreWentBack ? WRITE.coreBack.next : ov.failWhy.fix}
    onclose={() => (ov.stopClosed = op.failed)}
    onmore={() => (onmore('log'), (ov.stopClosed = op.failed))}
  >
    {#snippet buttons()}
      <BandButtons secondary={band.secondary} primary={band.primary} onpress={stopNow} />
    {/snippet}
  </StopModal>
{/if}
{#if writes.romNode}
  <RomConfirm node={writes.romNode.name} onwrite={() => writes.writeRom()} oncancel={() => (writes.romAsk = null)} />
{/if}
{#if ov.connectModal}
  <Modal title={HUB_TOGGLE.connect} chip={{ text: band.chip ?? '', kind: 'error' }} onclose={() => (ov.connectClosed = true)}>
    <h3 class="modal-h">{NOTICE.fix}</h3>
    <p>{band.sentence}</p>
    {#snippet actions()}
      <BandButtons secondary={[]} primary={band.primary} onpress={(a) => ((ov.connectClosed = true), act(a))} />
    {/snippet}
  </Modal>
{/if}
{#if ov.dialogOpen}
  <Reconnect notes={op.reconnectNotes} {onreconnect} onclose={() => (ov.dialogDismissed = true)} />
{/if}
