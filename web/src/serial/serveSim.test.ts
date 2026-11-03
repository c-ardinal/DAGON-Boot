import { describe, expect, test } from 'vitest';
import { serveSim } from './serveSim.ts';

describe('dagon demo link', () => {
  test('bytes go through serve, reads wait in wall time, and the clock runs at the demo speed', async () => {
    const written: number[][] = [];
    const waits: number[] = [];
    let answer = Uint8Array.of(0xda, 0x6f);
    let t = 1000;
    const slept: number[] = [];
    const sim = serveSim(
      {
        simWrite: async (b) => void written.push([...b]),
        simRead: async (w) => {
          waits.push(w);
          const a = answer;
          answer = new Uint8Array(0);
          return a;
        },
      },
      4,
      { now: () => t, sleep: async (ms) => void slept.push(ms) },
    );
    await expect(sim.transport.write(Uint8Array.of(1))).rejects.toThrow();
    await sim.transport.open();
    await sim.transport.write(Uint8Array.of(1, 2));
    expect(written).toEqual([[1, 2]]);
    expect(await sim.transport.read(20)).toEqual(Uint8Array.of(0xda, 0x6f));
    expect(await sim.transport.read(20)).toBeNull();
    expect(waits).toEqual([5, 5]);
    expect(sim.clock.now()).toBe(4000);
    t = 1250;
    expect(sim.clock.now()).toBe(5000);
    await sim.clock.sleep(300);
    expect(slept).toEqual([75]);
    await sim.transport.close();
    expect(await sim.transport.read(20)).toBeNull();
  });
});
