import { describe, expect, test } from 'vitest';
import { WasmHost, watch_interval_ms } from '../wasm/pkg/dagon_wasm.js';
import { hostLink } from './hostLink.ts';
import { MockTransport, SAMPLE_HUB_HELLO } from './mock.ts';
import { LOCAL } from '../wasm/dagon.ts';

// dagon-host's own HELLO and monitoring round, driven through the JS link.
describe('dagon-host over the page link', () => {
  test('PC-connected node HELLO', async () => {
    const t = new MockTransport();
    await t.open();
    const h = await new WasmHost(hostLink(t)).hello(LOCAL);
    expect(h).toEqual(SAMPLE_HUB_HELLO);
  });

  test('monitoring: a silent PC-connected node still counts as answering after one miss, not after two', async () => {
    const t = new MockTransport({ silent: true });
    await t.open();
    const host = new WasmHost(hostLink(t));
    expect((await host.watch(LOCAL)).responding).toBe(true);
    const second = await host.watch(LOCAL);
    expect([second.responding, second.rttMs, second.hello]).toEqual([false, null, null]);
  }, 10_000);

  test('a second call while one runs is refused', async () => {
    const t = new MockTransport({ delayMs: 50 });
    await t.open();
    const host = new WasmHost(hostLink(t));
    const first = host.hello(LOCAL);
    await expect(host.hello(LOCAL)).rejects.toThrow('busy');
    await first;
  });

  test('monitoring interval comes from Rust', () => {
    expect(watch_interval_ms()).toBe(5000);
  });
});
