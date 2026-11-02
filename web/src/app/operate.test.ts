import { describe, expect, test } from 'vitest';
import { devHub } from '../serial/devHub.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { topo_compile } from '../wasm/dagon.ts';
import { ConnectError } from './connect.ts';
import { ANNOUNCE, BAND } from './copy.ts';
import { Operate } from './operate.svelte.ts';
import { DESK_DEMO } from './sample.ts';

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

// dagon-host (through dagon-wasm) against the in-memory desk hub.
describe('operating mode against the desk hub', () => {
  test('find automatically → found → send → watching, with every node answering', async () => {
    const { t, op } = session();
    await op.connect();
    expect(op.state).toBe('L09');
    expect(op.config).toBe(DESK_DEMO);
    expect(op.found).toBe(3);
    expect(op.status).toBe(ANNOUNCE.A06(3));
    await settle();
    await op.round();
    expect(op.responding).toBe(3);

    await op.push();
    expect(op.state).toBe('L12');
    expect(op.sent).toBe(true);
    expect(t.received).toEqual(topo_compile(DESK_DEMO.yaml));
  }, 20_000);

  test('a node that misses two rounds in a row is not answering; it comes back on the next reply', async () => {
    const { t, op } = session();
    await op.connect();
    await op.push();
    await op.round();
    t.setQuiet('uart', true);
    await op.round();
    expect([op.state, op.live.watch.responding]).toEqual(['L12', true]);
    await op.round();
    expect([op.state, op.live.watch.responding, op.stuck?.id]).toEqual(['L13', false, 'watch']);
    expect(op.alert).toBe(ANNOUNCE.A21silent('ウォッチ'));
    expect(op.status).toBe(ANNOUNCE.A08(2, 3));
    expect(op.live.watch.hello).not.toBeNull();

    t.setQuiet('uart', false);
    await op.round();
    expect([op.state, op.responding]).toEqual(['L12', 3]);
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
    expect(op.state).toBe('L09');
  }, 20_000);

  test('a placed sample, then connecting: send first', async () => {
    const { op } = session();
    op.placeSample();
    expect(op.state).toBe('L02');
    await op.connect();
    expect(op.state).toBe('L09');
  }, 20_000);

  test('a device that does not answer: choose again', async () => {
    const { op } = session({ silent: true });
    await op.connect();
    expect([op.state, op.failure, op.status]).toEqual(['L07', 'no-response', BAND.M08.chip]);
  }, 20_000);

  test('closing the chooser and a busy port end in their own states', async () => {
    for (const [f, want] of [['not-found', 'L04'], ['busy', 'L05']] as const) {
      const op = new Operate({ open: () => Promise.reject(new ConnectError(f)) });
      await op.connect();
      expect([op.state, op.failure]).toEqual([want, f]);
    }
  });

  test('dagon-wasm missing: reload is the way on', async () => {
    const op = new Operate({ open: () => Promise.reject(new Error('unused')), ready: () => Promise.reject(new Error('404')) });
    await op.connect();
    expect([op.state, op.failure]).toEqual(['L05', 'unavailable']);
  });

  test('unplugging while watching: L26 with the alert', async () => {
    const { op } = session();
    await op.connect();
    op.lose();
    expect([op.state, op.alert]).toEqual(['L26', ANNOUNCE.A21lost]);
  }, 20_000);
});
