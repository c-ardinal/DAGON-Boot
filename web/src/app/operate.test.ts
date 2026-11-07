import { describe, expect, test } from 'vitest';
import { devHub } from '../serial/devHub.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { topo_compile } from '../wasm/dagon.ts';
import { ConnectError } from './connect.ts';
import { ANNOUNCE, BAND } from './copy.ts';
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

// dagon-host (through dagon-wasm) against the in-memory hub.
describe('operating mode against the in-memory hub', () => {
  test('find automatically → found → send → watching, with every node answering', async () => {
    const { t, op } = session();
    await op.connect();
    expect(op.state).toBe('scanned');
    expect(op.config).toBe(SAMPLE_CONFIG);
    expect(op.found).toBe(3);
    expect(op.status).toBe(ANNOUNCE.A06(3));
    await settle();
    await op.round();
    expect(op.responding).toBe(3);

    await op.push();
    expect(op.state).toBe('ready');
    expect(t.received).toEqual(topo_compile(SAMPLE_CONFIG.yaml));
  }, 20_000);

  test('a node that misses two rounds in a row is not answering; it comes back on the next reply', async () => {
    const { t, op } = session();
    await op.connect();
    await op.push();
    await op.round();
    t.setQuiet('uart', true);
    await op.round();
    expect([op.state, op.live.watch.responding]).toEqual(['ready', true]);
    await op.round();
    expect([op.state, op.live.watch.responding, op.stuck?.id]).toEqual(['nodesSilent', false, 'watch']);
    expect(op.alert).toBe(ANNOUNCE.A21silent('ウォッチ'));
    expect(op.status).toBe(ANNOUNCE.A08(2, 3));
    expect(op.live.watch.hello).not.toBeNull();

    t.setQuiet('uart', false);
    await op.round();
    expect([op.state, op.responding]).toEqual(['ready', 3]);
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

  test('the page’s ?mock hub answers inside dagon-host’s request time', async () => {
    const op = new Operate({
      open: async () => {
        const t = devHub(new URLSearchParams('mock'))!.transport;
        await t.open();
        return t;
      },
    });
    await op.connect();
    expect(op.state).toBe('scanned');
  }, 20_000);

  // The project's YAML is the one source of truth: connecting goes straight to watching.
  test('a 構成 on the canvas, then connecting: watching at once, nothing to send first', async () => {
    const { t, op } = session();
    op.useConfig(SAMPLE_CONFIG);
    expect(op.state).toBe('idle');
    await op.connect();
    expect(op.state).toBe('ready');
    expect(t.received).toBeNull();
  }, 20_000);

  test('"最初からやり直す" drops the connection and the records and waits at idle', async () => {
    const { op } = session();
    await op.connect();
    op.reset();
    expect([op.state, op.config, op.file, op.live]).toEqual(['idle', null, null, {}]);
  }, 20_000);

  test('a device that does not answer: choose again', async () => {
    const { op } = session({ silent: true });
    await op.connect();
    expect([op.state, op.failure, op.status]).toEqual(['silentHub', 'no-response', BAND.M08.chip]);
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
    expect([op.state, op.alert]).toEqual(['lost', ANNOUNCE.A21lost]);
  }, 20_000);
});
