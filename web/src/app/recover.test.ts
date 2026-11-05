import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import type { LinkClock } from '../serial/hostLink.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { simDesk } from '../serial/simHub.ts';
import type { Transport } from '../serial/transport.ts';
import { versionText } from '../wasm/dagon.ts';
import { bandOf } from './band.ts';
import { ANNOUNCE, BAND, BUTTON_FOR, NODE } from './copy.ts';
import { DESK_DEMO } from './sample.ts';
import { Operate, failureStage } from './operate.svelte.ts';

const repo = (rel: string) => new Uint8Array(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
/** The project's files dagon serve hands out (release/latest, the boot core): the desk release 1.1.0 and the F401 boot core, both under the test key. */
const LATEST_RELEASE = 'release/latest.dgb';
const bootcorePath = (id: string) => `bootcore/${id}`;
const SITE: Record<string, Uint8Array> = {
  [LATEST_RELEASE]: repo('crates/dagon-sim/fixtures/desk-1.1.0.dgb'),
  [bootcorePath('seg')]: repo('crates/dagon-wasm/fixtures/f401-bootcore-1.0.0.img'),
};
type Fetch = (path: string) => Promise<Uint8Array | null>;
const site: Fetch = async (path) => SITE[path] ?? null;
const nothing: Fetch = async () => null;
const SIM_PAIR_YAML = new TextDecoder().decode(repo('topology/sim-pair.yaml'));
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
    reselect: async () => {
      await t.open();
      return t;
    },
    clock,
    latest: () => fetchAsset(LATEST_RELEASE),
    bootcore: (id) => fetchAsset(bootcorePath(id)),
    // The F401's stage0 for the ROM loader's full rewrite (the simulated
    // node runs no code, so any bytes stand in).
    stage0: async () => new Uint8Array(0x400).fill(0x5a),
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
    // Superseded (owner 11/2): recovery was the main action; with other nodes answering it is now beside "update those first".
    expect(bandOf(op.state, op.ctx, { stuck: op.recoverTarget?.name }).secondary.map((b) => b.label)).toEqual([BUTTON_FOR.recover('7セグ')]);

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
    // The node refused again: its reason stays (× 書き込めない), not a generic failure nor 応答なし.
    expect(op.state).toBe('L25');
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason: 'E_FLASH' });
    expect(NODE.refused[op.progress.seg.reason!]).toBe('× 書き込めない');
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

  test('no release on the screen nor in the project: the page asks for one, then recovers with it', async () => {
    const sim = simDesk(3)!;
    sim.desk().wipe(0x42);
    const op = session(sim.transport, sim.clock, async (p) => (p === LATEST_RELEASE ? null : site(p)));
    await silentNode(op);
    expect(await op.recover()).toBe('needFile');
    expect(op.state).toBe('L13');
    await op.recover({ name: 'desk-1.1.0.dgb', bytes: SITE[LATEST_RELEASE] });
    expect([op.state, op.recovered?.path]).toEqual(['L23', 'rom']);
  }, 120_000);

  test('ROM path without the project boot core cannot fix the node (L25)', async () => {
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

  test('dagon-host reasons map to their chips: no answer, previous version, refusals, release refusals, the rest', () => {
    expect(failureStage('NO ANSWER')).toEqual({ stage: 'silent' });
    expect(failureStage('ROLLED BACK')).toEqual({ stage: 'previous' });
    expect(failureStage('E_SIGNATURE')).toEqual({ stage: 'refused', reason: 'E_SIGNATURE' });
    for (const k of ['source', 'key', 'topology', 'format']) expect(failureStage(k)).toEqual({ stage: 'refused', reason: k });
    expect(failureStage('NO IMAGE')).toEqual({ stage: 'cannotFix' });
  });

  test('recovery refused for another configuration says 構成 不一致, as the update does', async () => {
    const { op } = await stoppedAt({ refuse: { 'i2c:66': 0x07 } });
    op.config = { ...DESK_DEMO, yaml: SIM_PAIR_YAML };
    await op.recover();
    expect(op.state).toBe('L25');
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason: 'topology' });
    expect(NODE.refused.topology).toBe('× 構成 不一致');
    expect(op.status).toBe(ANNOUNCE.A10topology);
  }, 120_000);

  test('recovery with a published file that is not a release says 更新ファイルではない', async () => {
    const t = new MockTransport({ quiet: ['i2c:66'] });
    const op = session(t, fastClock(), async (p) => (p === LATEST_RELEASE ? new TextEncoder().encode('not a release') : null));
    await silentNode(op);
    await op.recover();
    expect(op.state).toBe('L25');
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason: 'format' });
    expect(NODE.refused.format).toBe('× 更新ファイルではない');
    expect(op.status).toBe(ANNOUNCE.A10format);
  }, 120_000);

  test('a file chosen for recovery is refused with its own words: source, configuration, not a release', async () => {
    const t = new MockTransport({ quiet: ['i2c:66'] });
    const op = session(t, fastClock(), nothing);
    await silentNode(op);
    const bad = SITE[LATEST_RELEASE].slice();
    bad[bad.length - 1] ^= 1;
    await op.recover({ name: 'tampered.dgb', bytes: bad });
    expect([op.state, op.alert]).toEqual(['L13', ANNOUNCE.A10source]);
    await op.recover({ name: 'notes.txt', bytes: new TextEncoder().encode('hello') });
    expect([op.state, op.alert]).toEqual(['L13', ANNOUNCE.A10format]);
    op.config = { ...DESK_DEMO, yaml: SIM_PAIR_YAML };
    await op.recover({ name: 'desk-1.1.0.dgb', bytes: SITE[LATEST_RELEASE] });
    expect([op.state, op.alert]).toEqual(['L13', ANNOUNCE.A10topology]);
  }, 120_000);

  test('a rollback that gets no answer shows 応答なし; "もう一度直す" rolls back again, it does not install the release', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    op.takeFile('desk-1.1.0.dgb', SITE[LATEST_RELEASE]);
    await op.update();
    expect(op.state).toBe('L23');

    t.setQuiet('i2c:66', true);
    await op.rollback('seg');
    expect([op.state, op.progress.seg.stage, op.recoverTarget?.id]).toEqual(['L25', 'silent', 'seg']);

    t.setQuiet('i2c:66', false);
    await op.retry();
    expect(op.state).toBe('L23');
    expect(op.status).toBe(ANNOUNCE.A15('7セグ', '1.0.0'));
    expect([v(t.helloOf('i2c:66').activeVersion), t.helloOf('i2c:66').trial]).toEqual(['1.0.0', false]);
  }, 120_000);

  test('recovering the hub: not back in 45 s, chosen again (L21), then confirmed on the release', async () => {
    const { t, op } = await stoppedAt({ refuse: { hub: 0x07 } });
    expect([op.failed, op.recoverTarget?.id]).toEqual(['hub', 'hub']);
    t.setRefuse('hub');
    t.setHubAway(Infinity);
    await op.recover();
    expect([op.state, op.alert]).toEqual(['L21', ANNOUNCE.A21timeout]);
    expect(t.helloOf('hub').trial).toBe(true);

    t.replug();
    await op.reconnectHub();
    expect(op.state).toBe('L23');
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.1.0', false]);
  }, 120_000);

  test('rolling the hub back: not back in 45 s, chosen again (L21), then confirmed on the previous version', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    op.takeFile('desk-1.1.0.dgb', SITE[LATEST_RELEASE]);
    await op.update();
    expect(op.state).toBe('L23');

    t.setHubAway(Infinity);
    await op.rollback('hub');
    expect(op.state).toBe('L21');
    t.replug();
    await op.reconnectHub();
    expect(op.state).toBe('L23');
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.0.0', false]);
  }, 120_000);

  test('"最新のリリースを使う" takes the project latest release through serve; without one the band says M58', async () => {
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
    expect([op2.state, op2.alert, op2.latestMissing]).toEqual(['L12', ANNOUNCE.A22latest, true]);

    const bad = SITE[LATEST_RELEASE].slice();
    bad[bad.length - 1] ^= 1;
    const op3 = session(new MockTransport(), fastClock(), async () => bad);
    await op3.connect();
    await op3.push();
    expect(await op3.useLatest()).toBe(false);
    expect(op3.state).toBe('L16');
  }, 60_000);
});
