import { describe, expect, test } from 'vitest';
import { devHub } from '../serial/devHub.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { LOCAL, type Target } from '../wasm/dagon.ts';
import { ConnectError } from './connect.ts';
import { ANNOUNCE, BAND } from './copy.ts';
import { host_reconnect_ms } from '../wasm/pkg/dagon_wasm.js';
import { Operate } from './operate.svelte.ts';
import { SAMPLE_CONFIG } from './sample.ts';

function session(opts: MockOptions = {}) {
  const t = new MockTransport(opts);
  const op = new Operate({
    open: async () => {
      await t.open();
      return t;
    },
  });
  return { t, op };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

describe('the reconnect wait', () => {
  test("is dagon-host's one value, read through dagon-wasm", () => {
    const { op } = session();
    (op as unknown as { hubAway(v: undefined): void }).hubAway(undefined);
    const left = op.reconnectUntil! - op.now();
    expect(left).toBeGreaterThan(host_reconnect_ms() - 1000);
    expect(left).toBeLessThanOrEqual(host_reconnect_ms());
    expect(host_reconnect_ms()).toBe(45_000);
  });
});

// dagon-host (through dagon-wasm) against the in-memory PC-connected node.
describe('operating mode against the in-memory PC-connected node', () => {
  test('find automatically → found → watching, with every node answering', async () => {
    const { op } = session();
    await op.connect();
    expect(op.state).toBe('ready');
    expect(op.config).toBe(SAMPLE_CONFIG);
    expect(op.found).toBe(3);
    expect(op.status).toBe(ANNOUNCE.found(3));
    await settle();
    await op.round();
    expect(op.responding).toBe(3);
  }, 20_000);

  test('a node that misses two rounds in a row is not answering; it comes back on the next reply', async () => {
    const { t, op } = session();
    await op.connect();
    await op.round();
    t.setQuiet('uart', true);
    await op.round();
    expect([op.state, op.live.watch.responding]).toEqual(['ready', true]);
    await op.round();
    expect([op.state, op.live.watch.responding, op.stuck?.id]).toEqual(['nodesSilent', false, 'watch']);
    expect(op.alert).toBe(ANNOUNCE.nodeSilent('ウォッチ'));
    expect(op.status).toBe(ANNOUNCE.responding(2, 3));
    expect(op.live.watch.hello).not.toBeNull();

    t.setQuiet('uart', false);
    await op.round();
    expect([op.state, op.responding]).toEqual(['ready', 3]);
  }, 20_000);

  test('a configuration whose definitions do not load fails the call with that error and sends nothing', async () => {
    const { t, op } = session();
    await op.connect();
    op.useConfig({ ...SAMPLE_CONFIG, yaml: 'dagon: [2' });
    let writes = 0;
    const write = t.write.bind(t);
    t.write = async (raw: Uint8Array) => {
      writes += 1;
      return write(raw);
    };
    const call = (op as unknown as { call: (f: (h: { hello: (to: Target) => Promise<unknown> }) => Promise<unknown>) => Promise<unknown> }).call.bind(op);
    await expect(call((h) => h.hello(LOCAL))).rejects.toThrow(/./);
    expect(writes).toBe(0);
    // The configuration back: its definitions are given again and the call goes out.
    op.useConfig(SAMPLE_CONFIG);
    await call((h) => h.hello(LOCAL));
    expect(writes).toBeGreaterThan(0);
  }, 20_000);

  test('selecting a node reads its slots and health', async () => {
    const { op } = session();
    await op.connect();
    op.select('node:watch');
    await op.round();
    const s = op.live.watch.status!;
    expect(s.slots.map((x) => x.state)).toEqual([4, 0, 1]);
    expect([s.active, s.logGeneration]).toEqual([0, 12]);
    expect(op.live.watch.rttMs).not.toBeNull();
  }, 20_000);

  test('the page’s ?mock PC-connected node answers inside dagon-host’s request time', async () => {
    const op = new Operate({
      open: async () => {
        const t = devHub(new URLSearchParams('mock'))!.transport;
        await t.open();
        return t;
      },
    });
    await op.connect();
    expect(op.state).toBe('ready');
  }, 20_000);

  // The project's YAML is the one source of truth: connecting goes straight to watching.
  test('a 構成 on the canvas, then connecting: watching at once', async () => {
    const { op } = session();
    op.useConfig(SAMPLE_CONFIG);
    expect(op.state).toBe('idle');
    await op.connect();
    expect(op.state).toBe('ready');
  }, 20_000);

  test('"リスタート" drops the connection and the records and waits at idle', async () => {
    const { op } = session();
    await op.connect();
    op.reset();
    expect([op.state, op.config, op.file, op.live]).toEqual(['idle', null, null, {}]);
  }, 20_000);

  test('a device that does not answer: choose again', async () => {
    const { op } = session({ silent: true });
    await op.connect();
    expect([op.state, op.failure, op.status]).toEqual(['silentHub', 'no-response', BAND.noResponse.chip]);
  }, 20_000);

  test('closing the chooser and a busy port end in their own states', async () => {
    for (const [f, want] of [['not-found', 'notChosen'], ['busy', 'portFailed']] as const) {
      const op = new Operate({ open: () => Promise.reject(new ConnectError(f)) });
      await op.connect();
      expect([op.state, op.failure]).toEqual([want, f]);
    }
  });

  test('dagon-wasm missing: reload is the way on', async () => {
    const op = new Operate({ open: () => Promise.reject(new Error('unused')), ready: () => Promise.reject(new Error('404')) });
    await op.connect();
    expect([op.state, op.failure]).toEqual(['portFailed', 'unavailable']);
  });

  test('unplugging while watching: lost with the alert', async () => {
    const { op } = session();
    await op.connect();
    op.lose();
    expect([op.state, op.alert]).toEqual(['lost', ANNOUNCE.hubLost]);
  }, 20_000);
});
