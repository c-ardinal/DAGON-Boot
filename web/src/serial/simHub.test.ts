import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { Operate } from '../app/operate.svelte.ts';
import { trialMs } from '../wasm/dagon.ts';
import type { LinkClock } from './hostLink.ts';
import { simulator } from './simHub.ts';
import type { Transport } from './transport.ts';

const repo = (rel: string) => new Uint8Array(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
const RELEASE = repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb');
/** The sample's MCUs: each node's self-check counts against its MCU definition's trial time, as on the page. */
const MCU: Record<string, string> = { hub: 'rp2040', seg: 'stm32f401re', watch: 'esp32s3' };

describe('the browser simulator takes the links and flash their real time', () => {
  test('a write of the sample release shows the % of every node rise step by step, and each transfer takes its erase and link time', async () => {
    const sim = simulator(11)!;
    expect(sim, 'built with the sim feature (npm run wasm:dev)').not.toBeNull();
    // Every wait of dagon-host (a sleep, or a read waiting for an answer) is a moment the page could draw: note each node's progress there.
    const seen: Record<string, number[]> = {};
    let op: Operate | null = null;
    /** Simulated time at which each node was first and last seen in its transfer. */
    const writing: Record<string, [number, number]> = {};
    const note = () => {
      for (const [id, p] of Object.entries(op?.progress ?? {})) {
        if (p.stage === 'writing') writing[id] = [writing[id]?.[0] ?? sim.clock.now(), sim.clock.now()];
      }
      // The node's one % as its card shows it: transfer, restart and self-check.
      for (const id of Object.keys(op?.progress ?? {})) (seen[id] ??= []).push(op!.percentOf(id));
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
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('done');

    for (const id of ['hub', 'seg', 'watch']) {
      const between = new Set((seen[id] ?? []).filter((p) => p > 0 && p < 100));
      expect(between.size, `${id}: distinct steps between 0 and 100 %`).toBeGreaterThan(3);
    }
    // Each 子機's transfer takes its sector erase (600 ms) and its link's time on top: about 25 bytes per ms each way on I2C, 11 on the UART.
    for (const id of ['seg', 'watch']) {
      const [from, to] = writing[id] ?? [0, 0];
      expect(to - from, `${id}: simulated ms in transfer`).toBeGreaterThan(600);
    }
  }, 120_000);
});
