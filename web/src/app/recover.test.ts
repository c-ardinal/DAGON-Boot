import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import type { LinkClock } from '../serial/hostLink.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { simulator } from '../serial/simHub.ts';
import type { Transport } from '../serial/transport.ts';
import { PHASE, versionText } from '../wasm/dagon.ts';
import { bandOf } from './band.ts';
import { ANNOUNCE, BAND, BUTTON_FOR, NODE, WRITE } from './copy.ts';
import { SAMPLE_CONFIG } from './sample.ts';
import { Operate, failureStage, hubBack, writingTo } from './operate.svelte.ts';

const repo = (rel: string) => new Uint8Array(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
/** The project's files dagon serve hands out (release/latest, the boot core): the sample release 1.1.0 and the F401 boot core, both under the test key. */
const LATEST_RELEASE = 'release/latest.dgb';
const bootcorePath = (id: string) => `bootcore/${id}`;
const SITE: Record<string, Uint8Array> = {
  [LATEST_RELEASE]: repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb'),
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
  expect(op.state).toBe('nodesSilent');
}

async function stoppedAt(opts: MockOptions, fetchAsset: Fetch = site) {
  const t = new MockTransport(opts);
  const op = session(t, fastClock(), fetchAsset);
  await op.connect();
  await op.push();
  op.takeFile('sample-1.1.0.dgb', SITE[LATEST_RELEASE]);
  await op.update();
  expect(op.state).toBe('stopped');
  return { t, op };
}

describe('recovery from the page with dagon-host recover', () => {
  test('ROM path against the simulated F401 ROM loader (no hub): an erased 7セグ comes back on 1.1.0', async () => {
    const sim = simulator(7)!;
    expect(sim, 'built with the sim feature (npm run wasm:dev)').not.toBeNull();
    expect(sim.hub().wipe(0x42)).toBe(true);
    const op = session(sim.transport, sim.clock);
    await silentNode(op);
    expect(op.recoverTarget?.id).toBe('seg');
    // With other nodes answering, the band only updates those; the silent one is written from its card.
    expect(bandOf(op.state, op.ctx, { stuck: op.recoverTarget?.name }).secondary).toEqual([]);

    // Asked first, with nothing moved: cancelling here erases nothing.
    expect(await op.writeNode({ id: 'seg' })).toBe('confirm');
    expect(op.state).toBe('nodesSilent');

    await op.writeNode({ id: 'seg', confirmed: true });
    expect(op.state).toBe('done');
    expect(op.recovered).toMatchObject({ path: 'rom', rom: { attempts: 1 } });
    expect(v(op.recovered?.version)).toBe('1.1.0');
    expect(op.progress.seg.stage).toBe('done');
    expect(op.status).toBe(ANNOUNCE.A16('7セグ', 0));
    expect(v(op.live.seg.hello?.activeVersion)).toBe('1.1.0');
    expect(op.live.seg.hello?.trial).toBe(false);
  }, 120_000);

  test('a boot core that went back: its own outcome, the node answers on its confirmed core and the app stays', async () => {
    const sim = simulator(13)!;
    const before = sim.hub();
    expect(before.failCore(0x42, 1, 1, 0)).toBe(true);
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    await op.round();
    const app = op.live.seg.hello?.activeVersion;
    op.takeFile('sample-core-1.1.0.dgb', repo('crates/dagon-sim/fixtures/sample-core-1.1.0.dgb'));
    await op.update();
    expect(op.state).toBe('stopped');
    expect(op.progress.seg.stage).toBe('coreBack');
    expect(op.coreWentBack).toBe(true);
    expect(op.wentBack).toBe(false);
    expect(op.status).toBe(ANNOUNCE.A13core('7セグ'));
    // What the node says afterwards, read as a new connection would.
    await op.disconnect();
    const after = session(sim.transport, sim.clock);
    await after.connect();
    await after.push();
    after.selected = 'node:seg';
    await after.round();
    expect(after.live.seg.responding).toBe(true);
    expect(after.live.seg.hello?.activeVersion).toBe(app);
    expect(v(after.live.seg.core?.slots[after.live.seg.core.active].version)).toBe('1.0.0');
  }, 120_000);

  test('the same package with a good boot core writes through', async () => {
    const sim = simulator(13)!;
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    op.takeFile('sample-core-1.1.0.dgb', repo('crates/dagon-sim/fixtures/sample-core-1.1.0.dgb'));
    await op.update();
    expect(op.state).toBe('done');
    expect(op.coreWentBack).toBe(false);
  }, 120_000);

  test('a checked node whose boot loader differs is written though its app is the same, and says what is being written', async () => {
    const sim = simulator(13)!;
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    // Apps first: everything runs 1.1.0 on boot core 1.0.0.
    const apps = repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb');
    const both = repo('crates/dagon-sim/fixtures/sample-core-1.1.0.dgb');
    op.takeFile('sample-1.1.0.dgb', apps);
    await op.update();
    expect(op.state).toBe('done');
    await op.round();
    expect(await op.noWrite(apps)).toMatchObject({ same: op.fileVersions.seg });

    // The package holds only the F401's images. Only its boot loader differs now.
    op.toggleSkip('hub');
    op.toggleSkip('watch');
    expect(await op.noWrite(both)).toBe(null);
    op.takeFile('sample-core-1.1.0.dgb', both);
    const said: (string | null)[] = [];
    const on = op['onEvent'].bind(op);
    op['onEvent'] = (e) => {
      on(e);
      const p = op.progress.seg;
      const line = p?.stage === 'writing' ? writingTo(p, op.fileVersions.seg) : null;
      if (line && said[said.length - 1] !== line) said.push(line);
    };
    await op.update();
    expect(op.state).toBe('done');
    await op.round();
    // Both match: nothing stands to be written, and the same app version stays.
    expect(await op.noWrite(both)).toMatchObject({ same: op.fileVersions.seg });
    // The app is what the node runs: it is not written, and the card says only what was.
    expect(said).toEqual(['→ ブートローダ 1.1.0']);
    expect(op.progress.seg).toMatchObject({ stage: 'done', percent: 100 });
    expect(op.percentOf('seg')).toBe(100);
    expect(await op.noWrite(both)).toMatchObject({ coreToo: true });
    // The previous version stays to go back to.
    await op.rollback('seg');
    expect(v(op.live.seg.hello?.activeVersion)).toBe('1.0.0');
  }, 240_000);

  test('the one % never moves back across a boot loader and its app, and stays the number the node is told', async () => {
    const sim = simulator(13)!;
    const op = session(sim.transport, sim.clock);
    op['deps'].trialMs = () => 4000;
    await op.connect();
    await op.push();
    op.toggleSkip('hub');
    op.toggleSkip('watch');
    op.takeFile('sample-core-1.1.0.dgb', repo('crates/dagon-sim/fixtures/sample-core-1.1.0.dgb'));
    const seen: number[] = [];
    let confirmedCore = false;
    const on = op['onEvent'].bind(op);
    op['onEvent'] = (e) => {
      seen.push(op.percentOf('seg'));
      on(e);
      const after = op.percentOf('seg');
      seen.push(after);
      if (e.phase === PHASE.transfer) expect(after).toBe(e.shown);
      confirmedCore ||= e.phase === PHASE.confirm && e.bootCore;
    };
    await op.update();
    expect(op.state).toBe('done');
    expect(confirmedCore).toBe(true);
    expect(seen.filter((p, i) => i > 0 && p < seen[i - 1])).toEqual([]);
    expect(seen[seen.length - 1]).toBe(100);
  }, 120_000);

  test('the words say the boot loader was the same only when it was compared', async () => {
    expect(WRITE.alreadyOn('1.1.0', true).detail).toContain('ブートローダも同じ');
    expect(WRITE.alreadyOn('1.1.0', false).detail).not.toContain('ブートローダ');
    const sim = simulator(13)!;
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    const apps = repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb');
    op.takeFile('sample-1.1.0.dgb', apps);
    await op.update();
    await op.round();
    expect(await op.noWrite(apps)).toMatchObject({ coreToo: false });
  }, 120_000);

  test('the app differs and the boot loader is the same: written, labelled by the app', async () => {
    const sim = simulator(13)!;
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    const apps = repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb');
    expect(await op.noWrite(apps)).toBe(null);
    op.takeFile('sample-1.1.0.dgb', apps);
    await op.update();
    expect(op.state).toBe('done');
    expect(op.progress.seg.core).toBeUndefined();
  }, 120_000);

  test('a package whose only difference is a recovery agent is written, then said to be the same', async () => {
    const sim = simulator(13)!;
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    op.takeFile('sample-1.1.0.dgb', repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb'));
    await op.update();
    expect(op.state).toBe('done');
    await op.round();
    const agent = repo('crates/dagon-sim/fixtures/sample-agent-1.1.0.dgb');
    op.toggleSkip('hub');
    op.toggleSkip('seg');
    expect(await op.noWrite(agent)).toBe(null);
    op.takeFile('sample-agent-1.1.0.dgb', agent);
    await op.update();
    expect(op.state).toBe('done');
    await op.round();
    expect(await op.noWrite(agent)).toMatchObject({ same: op.fileVersions.watch });
    expect(op.progress.watch).toMatchObject({ stage: 'done', percent: 100 });
    // The app was left alone: the watch's previous version stays to go back to.
    await op.rollback('watch');
    expect(v(op.live.watch.hello?.activeVersion)).toBe('1.0.0');
  }, 180_000);

  test('a boot core that cannot be read is not called the same', async () => {
    const sim = simulator(13)!;
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    const both = repo('crates/dagon-sim/fixtures/sample-core-1.1.0.dgb');
    op.toggleSkip('hub');
    op.toggleSkip('watch');
    op.takeFile('sample-core-1.1.0.dgb', both);
    await op.update();
    expect(op.state).toBe('done');
    await op.round();
    expect(await op.noWrite(both)).toMatchObject({ same: op.fileVersions.seg });
    (op as unknown as { host: { bootCoreStatus: () => Promise<never> } }).host.bootCoreStatus = () => Promise.reject(new Error('no'));
    expect(await op.noWrite(both)).toBe(null);
  }, 120_000);

  test('the hub chosen again after a boot core took over, with an app that needs no writing, is done', () => {
    const core = 0x01010000;
    expect(hubBack({ outcome: 'notOnTrial', version: 0x01000000 } as never, undefined, core, core, true)).toBe('done');
    expect(hubBack({ outcome: 'notOnTrial', version: 0x01000000 } as never, undefined, core, core, false)).toBe('unwritten');
    expect(hubBack({ outcome: 'notOnTrial', version: 0x01000000 } as never, undefined, core, 0x01000000, true)).toBe('coreBack');
  });

  test('the hub chosen again by hand after its boot core write: what it says decides the words', () => {
    const on = (version: number) => ({ outcome: 'notOnTrial', version }) as never;
    // The app was not sent: the boot core is what stopped the write.
    expect(hubBack(on(5), undefined, 9, 5)).toBe('coreBack');
    expect(hubBack(on(5), undefined, 9, null)).toBe('coreBack');
    expect(hubBack(on(5), undefined, 9, 9)).toBe('unwritten');
    // The app was sent and it is not what runs.
    expect(hubBack(on(5), 7, 9, 9)).toBe('previous');
    expect(hubBack(on(5), 7, undefined, null)).toBe('previous');
    expect(hubBack(on(7), 7, 9, 9)).toBe('done');
    expect(hubBack({ outcome: 'confirmed', version: 7 } as never, 7, undefined, null)).toBe('done');
  });

  // Every state a write can start in: the node's answer decides, not the page.
  test.each(['ready', 'fileReady', 'done'] as const)('%s: a silent node with a ROM entry is asked, then written through its ROM', async (at) => {
    const sim = simulator(11)!;
    const op = session(sim.transport, sim.clock);
    await op.connect();
    await op.push();
    await op.round();
    if (at !== 'ready') op.takeFile('sample-1.1.0.dgb', SITE[LATEST_RELEASE]);
    if (at === 'done') await op.update();
    expect(op.state).toBe(at);
    expect(sim.hub().wipe(0x42)).toBe(true);
    expect(await op.writeNode({ id: 'seg' })).toBe('confirm');
    expect(op.state).toBe(at);
    await op.writeNode({ id: 'seg', confirmed: true });
    expect(op.recovered?.path).toBe('rom');
  }, 120_000);

  test.each(['ready', 'fileReady'] as const)('%s: a silent node without a ROM entry gets the direct write', async (at) => {
    const t = new MockTransport({ quiet: ['uart'] });
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    if (at === 'fileReady') op.takeFile('sample-1.1.0.dgb', SITE[LATEST_RELEASE]);
    expect(op.state).toBe(at);
    expect(await op.writeNode({ id: 'watch' })).toBe('direct');
    expect(op.state).toBe(at);
  }, 120_000);

  test('a silent node without a ROM entry gets the direct write: nothing runs', async () => {
    const t = new MockTransport({ quiet: ['uart'] });
    const op = session(t, fastClock());
    await silentNode(op);
    expect(await op.writeNode({ id: 'watch' })).toBe('direct');
    expect(op.state).toBe('nodesSilent');
    expect(op.recovered).toBeNull();
  }, 120_000);

  test('an answering node is not asked: the normal transfer', async () => {
    const { t, op } = await stoppedAt({ refuse: { 'i2c:66': 0x07 } });
    t.setRefuse('i2c:66');
    expect(await op.writeNode()).toBeUndefined();
    expect(op.recovered?.path).toBe('retransfer');
  }, 120_000);

  test('re-transfer: a refusal that stops the update, recovery fails (recoverFailed), again after the fix, then the rest', async () => {
    const { t, op } = await stoppedAt({ refuse: { 'i2c:66': 0x07 } });
    expect(op.recoverTarget?.id).toBe('seg');

    await op.writeNode();
    // The node refused again: its reason stays (× 書き込めない), not a generic failure nor 応答なし.
    expect(op.state).toBe('recoverFailed');
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason: 'E_FLASH' });
    expect(NODE.refused[op.progress.seg.reason!]).toBe('× 書き込み失敗');
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M33.sentence);
    expect(op.recoverTarget?.id).toBe('seg');

    t.setRefuse('i2c:66');
    await op.writeNode();
    expect(op.recovered?.path).toBe('retransfer');
    expect([op.state, op.targets, op.rest]).toEqual(['fileReady', ['hub', 'watch'], true]);
    expect(bandOf(op.state, op.ctx, { rest: op.rest }).primary?.label).toBe(BUTTON_FOR.updateRest);

    await op.update();
    expect(op.state).toBe('done');
    for (const k of ['hub', 'i2c:66', 'uart'] as const) expect(v(t.helloOf(k).activeVersion), k).toBe('1.1.0');
  }, 120_000);

  test('to the factory agent: a watch answering as its agent gets the image through the normal transfer', async () => {
    const { t, op } = await stoppedAt({ refuse: { uart: 0x07 }, factory: ['uart'] });
    expect(op.recoverTarget?.id).toBe('watch');
    t.setRefuse('uart');
    await op.writeNode();
    expect(op.recovered?.path).toBe('factory');
    expect(op.state).toBe('fileReady');
    expect(t.helloOf('uart').mode).toBe(0);
  }, 120_000);

  test('no release on the screen nor in the project: the page asks for one, then recovers with it', async () => {
    const sim = simulator(3)!;
    sim.hub().wipe(0x42);
    const op = session(sim.transport, sim.clock, async (p) => (p === LATEST_RELEASE ? null : site(p)));
    await silentNode(op);
    expect(await op.writeNode()).toBe('needFile');
    expect(op.state).toBe('nodesSilent');
    expect(await op.writeNode({ chosen: { name: 'sample-1.1.0.dgb', bytes: SITE[LATEST_RELEASE] } })).toBe('confirm');
    await op.writeNode({ confirmed: true });
    expect([op.state, op.recovered?.path]).toEqual(['done', 'rom']);
  }, 120_000);

  test('ROM path without the project boot core cannot fix the node (recoverFailed)', async () => {
    const sim = simulator(5)!;
    sim.hub().wipe(0x42);
    const op = session(sim.transport, sim.clock, async (p) => (p === LATEST_RELEASE ? site(p) : null));
    await silentNode(op);
    await op.writeNode({ confirmed: true });
    expect([op.state, op.progress.seg.stage]).toEqual(['recoverFailed', 'cannotFix']);
  }, 120_000);

  test('back to the previous version after a finished update (UC-O10)', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    op.takeFile('sample-1.1.0.dgb', SITE[LATEST_RELEASE]);
    await op.update();
    expect(op.state).toBe('done');
    await op.rollback('seg');
    expect(op.state).toBe('done');
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

  test('recovery refused for another configuration says 構成不一致, as the update does', async () => {
    const { op } = await stoppedAt({ refuse: { 'i2c:66': 0x07 } });
    op.config = { ...SAMPLE_CONFIG, yaml: SIM_PAIR_YAML };
    await op.writeNode();
    expect(op.state).toBe('recoverFailed');
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason: 'topology' });
    expect(NODE.refused.topology).toBe('× 構成不一致');
    expect(op.status).toBe(ANNOUNCE.A10topology);
  }, 120_000);

  test('recovery with a published file that is not a release says 形式違い', async () => {
    const t = new MockTransport({ quiet: ['i2c:66'] });
    const op = session(t, fastClock(), async (p) => (p === LATEST_RELEASE ? new TextEncoder().encode('not a release') : null));
    await silentNode(op);
    await op.writeNode();
    expect(op.state).toBe('recoverFailed');
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason: 'format' });
    expect(NODE.refused.format).toBe('× 形式違い');
    expect(op.status).toBe(ANNOUNCE.A10format);
  }, 120_000);

  test('a file chosen for recovery is refused with its own words: source, configuration, not a release', async () => {
    const t = new MockTransport({ quiet: ['i2c:66'] });
    const op = session(t, fastClock(), nothing);
    await silentNode(op);
    const bad = SITE[LATEST_RELEASE].slice();
    bad[bad.length - 1] ^= 1;
    await op.writeNode({ chosen: { name: 'tampered.dgb', bytes: bad } });
    expect([op.state, op.alert]).toEqual(['nodesSilent', ANNOUNCE.A10source]);
    await op.writeNode({ chosen: { name: 'notes.txt', bytes: new TextEncoder().encode('hello') } });
    expect([op.state, op.alert]).toEqual(['nodesSilent', ANNOUNCE.A10format]);
    op.config = { ...SAMPLE_CONFIG, yaml: SIM_PAIR_YAML };
    await op.writeNode({ chosen: { name: 'sample-1.1.0.dgb', bytes: SITE[LATEST_RELEASE] } });
    expect([op.state, op.alert]).toEqual(['nodesSilent', ANNOUNCE.A10topology]);
  }, 120_000);

  test('a rollback that gets no answer shows 応答なし; "もう一度直す" rolls back again, it does not install the release', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    op.takeFile('sample-1.1.0.dgb', SITE[LATEST_RELEASE]);
    await op.update();
    expect(op.state).toBe('done');

    t.setQuiet('i2c:66', true);
    await op.rollback('seg');
    expect([op.state, op.progress.seg.stage, op.recoverTarget?.id]).toEqual(['recoverFailed', 'silent', 'seg']);

    t.setQuiet('i2c:66', false);
    await op.retry();
    expect(op.state).toBe('done');
    expect(op.status).toBe(ANNOUNCE.A15('7セグ', '1.0.0'));
    expect([v(t.helloOf('i2c:66').activeVersion), t.helloOf('i2c:66').trial]).toEqual(['1.0.0', false]);
  }, 120_000);

  test('recovering the hub: not back in 45 s, chosen again (reconnectNeeded), then confirmed on the release', async () => {
    const { t, op } = await stoppedAt({ refuse: { hub: 0x07 } });
    expect([op.failed, op.recoverTarget?.id]).toEqual(['hub', 'hub']);
    t.setRefuse('hub');
    t.setHubAway(Infinity);
    await op.writeNode();
    expect([op.state, op.alert]).toEqual(['reconnectNeeded', ANNOUNCE.A21timeout]);
    expect(t.helloOf('hub').trial).toBe(true);

    t.replug();
    await op.reconnectHub();
    expect(op.state).toBe('done');
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.1.0', false]);
  }, 120_000);

  test('rolling the hub back: not back in 45 s, chosen again (reconnectNeeded), then confirmed on the previous version', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    op.takeFile('sample-1.1.0.dgb', SITE[LATEST_RELEASE]);
    await op.update();
    expect(op.state).toBe('done');

    t.setHubAway(Infinity);
    await op.rollback('hub');
    expect(op.state).toBe('reconnectNeeded');
    t.replug();
    await op.reconnectHub();
    expect(op.state).toBe('done');
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.0.0', false]);
  }, 120_000);

  test('"最新のリリースを使う" takes the project latest release through serve; without one the band says M58', async () => {
    const t = new MockTransport();
    const op = session(t, fastClock());
    await op.connect();
    await op.push();
    expect(await op.useLatest()).toBe(true);
    expect([op.state, op.file?.name]).toEqual(['fileReady', 'latest.dgb']);

    const t2 = new MockTransport();
    const op2 = session(t2, fastClock(), nothing);
    await op2.connect();
    await op2.push();
    expect(await op2.useLatest()).toBe(false);
    expect([op2.state, op2.alert, op2.latestMissing]).toEqual(['ready', ANNOUNCE.A22latest, true]);

    const bad = SITE[LATEST_RELEASE].slice();
    bad[bad.length - 1] ^= 1;
    const op3 = session(new MockTransport(), fastClock(), async () => bad);
    await op3.connect();
    await op3.push();
    expect(await op3.useLatest()).toBe(false);
    expect(op3.state).toBe('fileRefused');
  }, 60_000);
});
