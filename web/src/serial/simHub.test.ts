import { readFileSync } from 'node:fs';
import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { Agent } from '../app/agent.svelte.ts';
import { BUILD } from '../app/copy.ts';
import { Operate } from '../app/operate.svelte.ts';
import StopModal from '../screens/StopModal.svelte';
import { trialMs } from '../wasm/dagon.ts';
import type { LinkClock } from './hostLink.ts';
import { simulator } from './simHub.ts';
import type { Transport } from './transport.ts';

const repo = (rel: string) => new Uint8Array(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
// The apps at their real size (48 KiB on each 子機), as the demo page writes them.
const RELEASE = repo('crates/dagon-sim/fixtures/sample-full-1.1.0.dgb');
/** The sample's MCUs: each node's self-check counts against its MCU definition's trial time, as on the page. */
const MCU: Record<string, string> = { hub: 'rp2040', seg: 'stm32f401re', watch: 'esp32s3' };

describe('the browser simulator takes the links and flash their real time', () => {
  test('a write of the sample release shows the % of every node rise step by step, and each transfer takes its erase and link time', async () => {
    const sim = simulator(11)!;
    expect(sim, 'built with the sim feature (npm run wasm:dev)').not.toBeNull();
    // Every wait of dagon-host (a sleep, or a read waiting for an answer) is a moment the page could draw: note each node's progress there.
    const seen: Record<string, number[]> = {};
    const overall: number[] = [];
    let op: Operate | null = null;
    /** Simulated time at which each node was first and last seen in its transfer. */
    const writing: Record<string, [number, number]> = {};
    const note = () => {
      for (const [id, p] of Object.entries(op?.progress ?? {})) {
        if (p.stage === 'writing') writing[id] = [writing[id]?.[0] ?? sim.clock.now(), sim.clock.now()];
      }
      // The node's one % as its card shows it: transfer, restart and self-check.
      for (const id of Object.keys(op?.progress ?? {})) {
        (seen[id] ??= []).push(op!.percentOf(id));
        if (op!.percentOf(id) === 100) expect(op!.progress[id].stage, `${id}: 100 % only on confirmation`).toBe('done');
      }
      // The overall number, as the status row and the focal number show it.
      if (op && op.targets.length) {
        const all = op.overallAt(sim.clock.now());
        overall.push(all);
        if (all === 100) expect(op.targets.every((id) => op!.progress[id]?.stage === 'done'), '100 % overall only when every node confirmed').toBe(true);
      }
    };
    const clock: LinkClock = { now: sim.clock.now, sleep: (ms) => (note(), sim.clock.sleep(ms)) };
    const transport: Transport = { ...sim.transport, read: (ms) => (note(), sim.transport.read(ms)) };
    op = new Operate({
      open: async () => (await transport.open(), transport),
      reselect: async () => (await transport.open(), transport),
      clock,
      latest: async () => RELEASE,
      bootcore: async () => null,
      stage0: async () => null,
      trialMs: (id) => trialMs(MCU[id] ?? ''),
    });
    await op.connect();
    await op.push();
    op.takeFile('sample-full-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('done');

    // The % shown never moves backwards, per node or overall.
    for (const [id, list] of Object.entries({ ...seen, overall })) {
      list.forEach((p, i) => expect(p, `${id} sample ${i}`).toBeGreaterThanOrEqual(list[i - 1] ?? 0));
    }
    for (const id of ['hub', 'seg', 'watch']) {
      const between = new Set((seen[id] ?? []).filter((p) => p > 0 && p < 100));
      expect(between.size, `${id}: distinct steps between 0 and 100 %`).toBeGreaterThan(3);
    }
    // 48 KiB crosses I2C at about 25 bytes per ms each way, and the UART at 11: seconds, not milliseconds.
    for (const id of ['seg', 'watch']) {
      const [from, to] = writing[id] ?? [0, 0];
      expect(to - from, `${id}: simulated ms in transfer`).toBeGreaterThan(3_000);
    }
  }, 120_000);
});

describe('a write the simulator sends back, as dagon demo --sim-fail watch does', () => {
  /** A write of the sample release to the simulator, with the watch's new version crashing at boot. */
  async function wentBack(): Promise<Operate> {
    const sim = simulator(12, (hub) => hub.failWatch())!;
    expect(sim, 'built with the sim feature (npm run wasm:dev)').not.toBeNull();
    const op = new Operate({
      open: async () => (await sim.transport.open(), sim.transport),
      reselect: async () => (await sim.transport.open(), sim.transport),
      clock: sim.clock,
      latest: async () => RELEASE,
      bootcore: async () => null,
      stage0: async () => null,
      trialMs: (id) => trialMs(MCU[id] ?? ''),
    });
    await op.connect();
    await op.push();
    op.takeFile('sample-full-1.1.0.dgb', RELEASE);
    await op.update();
    return op;
  }

  const modal = (op: Operate, linked: boolean) =>
    render(StopModal, {
      props: {
        target: `[3] ${op.failed}`,
        back: op.wentBack,
        reason: '',
        fix: '',
        linked,
        onask: () => undefined,
        onclose: () => undefined,
        onmore: () => undefined,
        buttons: createRawSnippet(() => ({ render: () => '<button>x</button>' })),
      },
    }).body;

  test('from the simulator\'s failure to the action, and the real record goes with the question', async () => {
    const op = await wentBack();
    expect([op.state, op.failed, op.progress.watch.stage, op.wentBack]).toEqual(['stopped', 'watch', 'previous', true]);
    expect(modal(op, true)).toContain(BUILD.askAgent);
    expect(modal(op, false)).not.toContain(BUILD.askAgent);

    const sent: Record<string, unknown>[] = [];
    const agent = new Agent(async (_m, path, body) => {
      const reply = path === 'agent' ? { linked: true } : (sent.push(body as Record<string, unknown>), { answer: 'はい。' });
      return new Response(JSON.stringify(reply), { status: 200 });
    });
    agent.record = () => op.settledRollbackRecord();
    await agent.refresh();
    await agent.askWhy(
      () => false,
      () => undefined,
    );
    expect(sent[0].rollback).toEqual({
      node: 'watch',
      failed_version: '1.1.0',
      returned_version: '1.0.0',
      reason: 'ROLLED BACK',
      node_state: { trial: false, last_reason: 3 },
    });
  }, 120_000);
});
