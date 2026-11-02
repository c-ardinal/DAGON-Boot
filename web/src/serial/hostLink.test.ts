import { describe, expect, test } from 'vitest';
import { WasmHost, watch_interval_ms } from '../wasm/pkg/dagon_wasm.js';
import { hostLink } from './hostLink.ts';
import { MockTransport, SAMPLE_HUB_HELLO } from './mock.ts';

// dagon-host's own HELLO and monitoring round, driven through the JS link.
describe('dagon-host over the page link', () => {
  test('hub HELLO', async () => {
    const t = new MockTransport();
    await t.open();
    const h = await new WasmHost(hostLink(t)).hello('hub', 0);
    expect(h).toEqual(SAMPLE_HUB_HELLO);
  });

  test('monitoring: a silent hub still counts as answering after one miss, not after two', async () => {
    const t = new MockTransport({ silent: true });
    await t.open();
    const host = new WasmHost(hostLink(t));
    expect((await host.watch('hub', 0)).responding).toBe(true);
    const second = await host.watch('hub', 0);
    expect([second.responding, second.rttMs, second.hello]).toEqual([false, null, null]);
  }, 10_000);

  test('a second call while one runs is refused', async () => {
    const t = new MockTransport({ delayMs: 50 });
    await t.open();
    const host = new WasmHost(hostLink(t));
    const first = host.hello('hub', 0);
    await expect(host.hello('hub', 0)).rejects.toThrow('busy');
    await first;
  });

  test('monitoring interval comes from Rust', () => {
    expect(watch_interval_ms()).toBe(5000);
  });
});
