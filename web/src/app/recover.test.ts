import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import type { LinkClock } from '../serial/hostLink.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { simDesk } from '../serial/simHub.ts';
import type { Transport } from '../serial/transport.ts';
import { versionText } from '../wasm/dagon.ts';
import { bandOf } from './band.ts';
import { ANNOUNCE, BAND, BUTTON_FOR } from './copy.ts';
import { Operate, bootcorePath, LATEST_RELEASE } from './operate.svelte.ts';

const repo = (rel: string) => new Uint8Array(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
/** The public site's files: the desk release 1.1.0 and the F401 boot core, both under the test key. */
const SITE: Record<string, Uint8Array> = {
  [LATEST_RELEASE]: repo('crates/dagon-sim/fixtures/desk-1.1.0.dgb'),
  [bootcorePath('seg')]: repo('crates/dagon-wasm/fixtures/f401-bootcore-1.0.0.img'),
};
type Fetch = (path: string) => Promise<Uint8Array | null>;
const site: Fetch = async (path) => SITE[path] ?? null;
const nothing: Fetch = async () => null;
const v = (raw: number | undefined) => (raw === undefined ? '' : versionText(raw));

function fastClock(): LinkClock {
  let t = 0;
  return {
    now: () => (t += 2),
    sleep: (ms) => {
      t += ms;
      return new Promise((r) => setTimeout(r, 0));
    },
  };
}

function session(t: Transport, clock: LinkClock, fetchAsset: Fetch = site) {
  return new Operate({
    open: async () => {
      await t.open();
      return t;
    },
    clock,
    fetchAsset,
  });
}

async function silentNode(op: Operate) {
  await op.connect();
  await op.push();
  await op.round();
  await op.round();
  expect(op.state).toBe('L13');
}

async function stoppedAt(opts: MockOptions, fetchAsset: Fetch = site) {
  const t = new MockTransport(opts);
  const op = session(t, fastClock(), fetchAsset);
  await op.connect();
  await op.push();
  op.takeFile('desk-1.1.0.dgb', SITE[LATEST_RELEASE]);
  await op.update();
  expect(op.state).toBe('L22');
  return { t, op };
}

describe('recovery from the page with dagon-host recover', () => {
  test('ROM path against the simulated F401 ROM loader (no hub): an erased 7セグ comes back on 1.1.0', async () => {
    const sim = simDesk(7)!;
    expect(sim, 'built with the sim feature (npm run wasm:dev)').not.toBeNull();
    expect(sim.desk().wipe(0x42)).toBe(true);
    const op = session(sim.transport, sim.clock);
    await silentNode(op);
    expect(op.recoverTarget?.id).toBe('seg');
    expect(bandOf(op.state, op.ctx, { stuck: op.recoverTarget?.name }).primary?.label).toBe(BUTTON_FOR.recover('7セグ'));

    await op.recover();
    expect(op.state).toBe('L23');
    expect(op.recovered).toMatchObject({ path: 'rom', rom: { attempts: 1 } });
    expect(v(op.recovered?.version)).toBe('1.1.0');
    expect(op.progress.seg.stage).toBe('done');
    expect(op.status).toBe(ANNOUNCE.A16('7セグ', 0));
    expect(v(op.live.seg.hello?.activeVersion)).toBe('1.1.0');
    expect(op.live.seg.hello?.trial).toBe(false);
  }, 120_000);

  test('re-transfer: a refusal that stops the update, recovery fails (L25), again after the fix, then the rest', async () => {
    const { t, op } = await stoppedAt({ refuse: { 'i2c:66': 0x07 } });
    expect(op.recoverTarget?.id).toBe('seg');

    await op.recover();
    expect([op.state, op.progress.seg.stage]).toEqual(['L25', 'cannotFix']);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M33.sentence);
    expect(op.recoverTarget?.id).toBe('seg');

    t.setRefuse('i2c:66');
    await op.recover();
    expect(op.recovered?.path).toBe('retransfer');
    expect([op.state, op.targets, op.rest]).toEqual(['L18', ['hub', 'watch'], true]);
    expect(bandOf(op.state, op.ctx, { rest: op.rest }).primary?.label).toBe(BUTTON_FOR.updateRest);

    await op.update();
    expect(op.state).toBe('L23');
    for (const k of ['hub', 'i2c:66', 'uart'] as const) expect(v(t.helloOf(k).activeVersion), k).toBe('1.1.0');
  }, 120_000);

  test('to the factory agent: a watch answering as its agent gets the image through the normal transfer', async () => {
    const { t, op } = await stoppedAt({ refuse: { uart: 0x07 }, factory: ['uart'] });
    expect(op.recoverTarget?.id).toBe('watch');
    t.setRefuse('uart');
    await op.recover();
    expect(op.recovered?.path).toBe('factory');
    expect(op.state).toBe('L18');
    expect(t.helloOf('uart').mode).toBe(0);
  }, 120_000);

  test('no release on the screen nor on the site: the page asks for one, then recovers with it', async () => {
    const sim = simDesk(3)!;
    sim.desk().wipe(0x42);
    const op = session(sim.transport, sim.clock, async (p) => (p === LATEST_RELEASE ? null : site(p)));
    await silentNode(op);
    expect(await op.recover()).toBe('needFile');
    expect(op.state).toBe('L13');
    await op.recover({ name: 'desk-1.1.0.dgb', bytes: SITE[LATEST_RELEASE] });
    expect([op.state, op.recovered?.path]).toEqual(['L23', 'rom']);
  }, 120_000);

  test('ROM path without the boot core on the site cannot fix the node (L25)', async () => {
    const sim = simDesk(5)!;
    sim.desk().wipe(0x42);
    const op = session(sim.transport, sim.clock, async (p) => (p === LATEST_RELEASE ? site(p) : null));
    await silentNode(op);
    await op.recover();
    expect([op.state, op.progress.seg.stage]).toEqual(['L25', 'cannotFix']);
  }, 120_000);

  test('back to the previous version after a finished update (UC-O10)', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    op.takeFile('desk-1.1.0.dgb', SITE[LATEST_RELEASE]);
    await op.update();
    expect(op.state).toBe('L23');
    await op.rollback('seg');
    expect(op.state).toBe('L23');
    expect(op.status).toBe(ANNOUNCE.A15('7セグ', '1.0.0'));
    expect([v(t.helloOf('i2c:66').activeVersion), t.helloOf('i2c:66').trial]).toEqual(['1.0.0', false]);
  }, 120_000);

  test('"配布中の最新バージョンを使う" takes the published release; without it the page says so', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    expect(await op.useLatest()).toBe(true);
    expect([op.state, op.file?.name]).toEqual(['L18', 'latest.dgb']);

    const t2 = new MockTransport();
    const op2 = session(t2, fastClock(), nothing);
    await op2.connect();
    await op2.push();
    expect(await op2.useLatest()).toBe(false);
    expect([op2.state, op2.alert]).toEqual(['L12', ANNOUNCE.A22latest]);
  }, 60_000);
});
