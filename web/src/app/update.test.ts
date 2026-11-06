import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import type { LinkClock } from '../serial/hostLink.ts';
import { MockTransport, SAMPLE_HUB_HELLO, type MockOptions } from '../serial/mock.ts';
import { PHASE, versionText, type UpdateEvent } from '../wasm/dagon.ts';
import { bandOf } from './band.ts';
import { ANNOUNCE, BAND, BAND_CHIP, BAND_FOR, BUTTON_FOR, NODE } from './copy.ts';
import { Define } from './define/define.svelte.ts';
import { topoDiff } from './define/diff.ts';
import { fromYaml, type Topo } from './define/model.ts';
import { Operate, waterOf } from './operate.svelte.ts';
import { SAMPLE_CONFIG, SAMPLE_YAML } from './sample.ts';

const repo = (rel: string) => new URL(`../../../${rel}`, import.meta.url);
/** dagon-sim's sample release 1.1.0: hub A/B, seg A/B and watch images for sample, test key. */
const RELEASE = new Uint8Array(readFileSync(repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb')));
const SIM_PAIR_YAML = new TextDecoder().decode(readFileSync(repo('topology/sim-pair.yaml')));

/** dagon-host's real timeouts (45 s hub reconnect, 30 s node wait) on a clock that runs fast. */
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

async function sentTo(opts: MockOptions = {}, keep?: (bytes: Uint8Array) => Promise<boolean>) {
  const t = new MockTransport(opts);
  const op = new Operate({
    keep,
    open: async () => {
      await t.open();
      return t;
    },
    reselect: async () => {
      await t.open();
      return t;
    },
    clock: fastClock(),
  });
  await op.connect();
  await op.push();
  expect(op.state).toBe('ready');
  return { t, op };
}

const v = (raw: number) => versionText(raw);

describe('update from the page with dagon-host apply against the mock hub', () => {
  test('all three: children in update order, the hub last, reconnect after its reboot, each confirmed on 1.1.0', async () => {
    const { t, op } = await sentTo();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    expect([op.state, op.file?.verdict, op.targets]).toEqual(['fileReady', 'pass', ['hub', 'seg', 'watch']]);
    expect(op.status).toBe(ANNOUNCE.A10ok(3));

    await op.update();
    expect(t.reboots).toEqual(['i2c:66', 'uart', 'hub']);
    expect(op.state).toBe('done');
    expect(op.status).toBe(ANNOUNCE.A14('1.1.0'));
    expect(Object.values(op.progress).map((p) => p.stage)).toEqual(['done', 'done', 'done']);
    expect(op.overallAt(op.now())).toBe(100);
    for (const k of ['hub', 'i2c:66', 'uart'] as const) {
      expect([v(t.helloOf(k).activeVersion), t.helloOf(k).trial], k).toEqual(['1.1.0', false]);
    }
  }, 60_000);

  test('the hub alone: with the nodes silent, "update the answering one first" updates only the hub', async () => {
    const { t, op } = await sentTo({ quiet: ['i2c:66', 'uart'] });
    await op.round();
    await op.round();
    expect(op.state).toBe('nodesSilent');
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    expect([op.state, op.targets]).toEqual(['fileReady', ['hub']]);

    await op.update();
    expect(op.state).toBe('done');
    expect(t.reboots).toEqual(['hub']);
    expect(v(t.helloOf('hub').activeVersion)).toBe('1.1.0');
    expect(v(t.helloOf('i2c:66').activeVersion)).toBe('1.0.0');
  }, 60_000);

  test('a tampered release is refused before anything is sent', async () => {
    const { t, op } = await sentTo();
    const bad = RELEASE.slice();
    bad[bad.length - 1] ^= 1;
    op.takeFile('tampered.dgb', bad);
    expect([op.state, op.file?.verdict, op.refusal]).toEqual(['fileRefused', 'source', 'source']);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M18.sentence);
    await op.update();
    expect([op.state, t.reboots]).toEqual(['fileRefused', []]);
  }, 20_000);

  test('a release built for another configuration is refused (topology mismatch)', async () => {
    const { t, op } = await sentTo();
    op.config = { ...SAMPLE_CONFIG, yaml: SIM_PAIR_YAML };
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    expect([op.state, op.refusal]).toEqual(['otherTopology', 'topology']);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M20.sentence);
    expect(t.reboots).toEqual([]);
  }, 20_000);

  test('otherTopology: the differences are shown, and 定義 takes the 構成 of the file so the same file then passes', async () => {
    const { op } = await sentTo();
    const d = new Define();
    d.load(fromYaml(SAMPLE_YAML.replace('addr: 0x42', 'addr: 0x43')));
    op.useConfig(d.config);
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    expect([op.state, op.refusal]).toEqual(['otherTopology', 'topology']);
    const file = op.fileTopology as unknown as Topo;
    const rows = topoDiff(d.doc.topo, file, d.doc.names);
    expect(rows).toEqual([expect.objectContaining({ here: expect.stringContaining('0x43'), file: expect.stringContaining('0x42'), nodes: ['seg'] })]);

    d.importRelease(file);
    expect(d.errors).toBe(0);
    op.useConfig(d.config);
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    expect([op.state, op.file?.verdict]).toEqual(['fileReady', 'pass']);
    d.undo();
    expect(d.doc.topo.links?.[0].addr).toBe(0x43);
  }, 20_000);

  test('a second 親機, a 孫機 and a loop of 子機 are skipped with why: the reachable part updates as before', async () => {
    const { t, op } = await sentTo();
    const d = new Define();
    // hub2 and a loop x <-> y sit between hub and seg in nodes[], so a node's topology index is not its 構成 position.
    const extra = (id: string, role: string) => `  - id: ${id}\n    mcu: rp2040\n    role: ${role}\n    port: self-boot\n    tag: ${id}\n`;
    d.load(
      fromYaml(
        SAMPLE_YAML.replace('  - id: seg\n', `${extra('hub2', 'coordinator')}${extra('x', 'node')}${extra('y', 'node')}  - id: seg\n`)
          .replace('\nbuses:', `${extra('deep', 'node')}\nbuses:`)
          .replace('links:\n', 'links:\n  - from: hub\n    to: hub2\n    kind: can\n  - from: seg\n    to: deep\n    kind: uart\n    iface: hw-uart0\n    pins: { tx: 0, rx: 1 }\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n  - from: x\n    to: y\n    kind: uart\n    iface: hw-uart0\n    pins: { tx: 0, rx: 1 }\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n  - from: y\n    to: x\n    kind: uart\n    iface: hw-uart0\n    pins: { tx: 0, rx: 1 }\n    control:\n      kind: cooperative\n      rom_entry: force-download-boot\n')
          .replace('order: [seg, watch, hub]', 'order: [seg, watch, deep, x, y, hub2, hub]'),
      ),
    );
    expect(d.errors).toBe(0);
    expect(d.config.nodes.filter((n) => n.notYet).map((n) => [n.id, n.notYet])).toEqual([
      ['hub2', 'second-hub'],
      ['x', 'unreachable'],
      ['y', 'unreachable'],
      ['deep', 'deep'],
    ]);
    // One source: the page's reach is the cut's list, and every other node says why.
    expect(d.config.reach).toEqual(['hub', 'seg', 'watch']);
    expect(d.config.nodes.every((n) => d.config.reach.includes(n.id) !== Boolean(n.notYet))).toBe(true);
    op.useConfig(d.config);
    await op.round();
    expect([op.live.hub2.responding, op.live.deep.responding, op.live.x.responding]).toEqual([null, null, null]);
    expect(op.stuck).toBeUndefined();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    expect([op.state, op.file?.verdict, op.targets]).toEqual(['fileReady', 'pass', ['hub', 'seg', 'watch']]);
    expect(op.status).toBe(ANNOUNCE.A10ok(3, 4));

    await op.update();
    expect(t.reboots).toEqual(['i2c:66', 'uart', 'hub']);
    expect(op.state).toBe('done');
    expect(Object.keys(op.progress).sort()).toEqual(['hub', 'seg', 'watch']);
    expect(op.finished).toEqual({ kind: 'updated', left: 0 });
  }, 60_000);

  test('not a release at all', async () => {
    const { op } = await sentTo();
    op.takeFile('notes.txt', new TextEncoder().encode('hello'));
    expect([op.state, op.refusal]).toEqual(['fileRefused', 'format']);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M19.sentence);
  }, 20_000);

  test('hub not back within 45 s: chosen again by the user, then dagon-host confirms it', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    const run = op.update();
    await new Promise((r) => setTimeout(r, 0));
    await run;
    expect(op.state).toBe('reconnectNeeded');
    expect(op.alert).toBe(ANNOUNCE.A21timeout);
    expect(t.helloOf('hub').trial).toBe(true);

    t.replug();
    await op.reconnectHub();
    expect(op.state).toBe('done');
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.1.0', false]);
  }, 60_000);

  test('hub not back within 45 s and back on its previous version when chosen again: stopped there, not done', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity, fallBack: ['hub'] });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('reconnectNeeded');
    t.replug();
    await op.reconnectHub();
    expect(v(t.helloOf('hub').activeVersion)).toBe('1.0.0');
    expect([op.state, op.failed, op.progress.hub.stage]).toEqual(['stopped', 'hub', 'previous']);
    expect(op.status).toBe(ANNOUNCE.A13previous('メイン'));
  }, 60_000);

  test('a USB disconnect while the hub reboots is not taken for a lost connection', async () => {
    const { op } = await sentTo({ hubAway: 5 });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    const run = op.update();
    while (op.state !== 'hubRestarting') await new Promise((r) => setTimeout(r, 1));
    op.lose();
    await run;
    expect(op.state).toBe('done');
  }, 60_000);

  test('a node that does not answer stops the run there; later nodes are not written', async () => {
    const { t, op } = await sentTo();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    t.setQuiet('i2c:66', true);
    await op.update();
    expect([op.state, op.failed]).toEqual(['stopped', 'seg']);
    expect(op.progress).toMatchObject({ seg: { stage: 'silent' }, watch: { stage: 'unwritten' }, hub: { stage: 'unwritten' } });
    expect(op.status).toBe(ANNOUNCE.A13('7セグ'));
    expect(bandOf(op.state, op.ctx, { stuck: '7セグ', unwritten: 2 })).toMatchObject(BAND_FOR.M29('7セグ'));
    expect(t.reboots).toEqual([]);
  }, 60_000);

  test('a run that stops at a later node shows the earlier confirmed node on its new version', async () => {
    const { t, op } = await sentTo();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    t.setQuiet('uart', true);
    await op.update();
    expect([op.state, op.failed]).toEqual(['stopped', 'watch']);
    expect(op.progress.seg.stage).toBe('done');
    expect([v(op.live.seg?.hello?.activeVersion ?? 0), op.live.seg?.hello?.trial]).toEqual(['1.1.0', false]);
  }, 60_000);

  test.each([
    [0x07, 'E_FLASH', '× 書き込み失敗'],
    [0x09, 'E_SIGNATURE', '× 配布元不明'],
  ])('a node that refuses (status %i) is shown with its reason, not as not answering', async (status, reason, chip) => {
    const { t, op } = await sentTo({ refuse: { 'i2c:66': status } });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.failed]).toEqual(['stopped', 'seg']);
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason });
    expect(NODE.refused[reason]).toBe(chip);
    expect(op.progress).toMatchObject({ watch: { stage: 'unwritten' }, hub: { stage: 'unwritten' } });
    expect(t.reboots).toEqual([]);
  }, 60_000);

  test('before the update each target shows what it gets: the version the release gives it', async () => {
    const { op } = await sentTo();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    expect(Object.fromEntries(Object.entries(op.fileVersions).map(([id, raw]) => [id, v(raw)]))).toEqual({ hub: '1.1.0', seg: '1.1.0', watch: '1.1.0' });
  }, 20_000);

  test('node choice: an unchecked node is not written; checking it again puts it back in update order', async () => {
    const { t, op } = await sentTo();
    op.takeFile('v1.1.0.dgb', RELEASE);
    op.choose('seg', false);
    expect(op.targets).toEqual(['hub', 'watch']);
    op.choose('seg', true);
    expect(op.targets).toEqual(['hub', 'seg', 'watch']);
    op.choose('seg', false);
    op.choose('nobody', true);
    expect(op.targets).toEqual(['hub', 'watch']);

    await op.update();
    expect(op.state).toBe('done');
    expect(t.reboots).toEqual(['uart', 'hub']);
    expect(v(t.helloOf('i2c:66').activeVersion)).toBe('1.0.0');
  }, 60_000);

  test('kept: the package is kept in the project before anything is written to a node', async () => {
    const seen: [number, number][] = [];
    let t: MockTransport | null = null;
    const r = await sentTo({}, async (bytes) => {
      seen.push([bytes.length, t!.reboots.length]);
      return true;
    });
    t = r.t;
    r.op.takeFile('v1.1.0.dgb', RELEASE);
    await r.op.update();
    expect(seen).toEqual([[RELEASE.length, 0]]);
    expect([r.op.state, r.op.keepFailed]).toEqual(['done', false]);
  }, 60_000);

  test('kept: a package that could not be kept is still written, and the failure is told', async () => {
    const { op } = await sentTo({}, async () => false);
    op.takeFile('v1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.keepFailed]).toEqual(['done', true]);
  }, 60_000);

  test('boot core and app in one write: the boot core\'s confirmation does not finish the node; only the app\'s makes 100 %', async () => {
    const { op } = await sentTo();
    op.takeFile('v1.1.0.dgb', RELEASE);
    const feed = (e: Omit<UpdateEvent, 'node'>) => (op as unknown as { onEvent(e: UpdateEvent): void }).onEvent({ node: 1, ...e });
    const app = op.fileVersions.seg!;
    const was = op.live.seg!.hello!.activeVersion;
    for (const phase of [PHASE.transfer, PHASE.verify, PHASE.reboot, PHASE.confirm]) feed({ phase, percent: 100, version: app, bootCore: true });
    expect(op.percentOf('seg')).toBeLessThan(100);
    expect(op.progress.seg?.stage).not.toBe('done');
    expect(op.live.seg!.hello!.activeVersion).toBe(was);
    feed({ phase: PHASE.transfer, percent: 10, version: app, bootCore: false });
    expect(op.percentOf('seg')).toBeLessThan(100);
    for (const phase of [PHASE.verify, PHASE.reboot, PHASE.confirm]) feed({ phase, percent: 100, version: app, bootCore: false });
    expect(op.percentOf('seg')).toBe(100);
    expect(op.progress.seg?.stage).toBe('done');
    expect(v(op.live.seg!.hello!.activeVersion)).toBe('1.1.0');
  }, 20_000);

  test('node choice on the cards: an unchecked node is left out of the next write', async () => {
    const { t, op } = await sentTo();
    op.toggleSkip('seg');
    expect(op.skip.has('seg')).toBe(true);
    op.takeFile('v1.1.0.dgb', RELEASE);
    expect(op.targets).toEqual(['hub', 'watch']);
    await op.update();
    expect(t.reboots).toEqual(['uart', 'hub']);
    op.toggleSkip('seg');
    expect(op.skip.has('seg')).toBe(false);
  }, 60_000);

  test('every written node ends at 100 % and shows its new version on confirmation', async () => {
    const { op } = await sentTo();
    op.takeFile('v1.1.0.dgb', RELEASE);
    await op.update();
    for (const id of ['hub', 'seg', 'watch']) {
      expect(op.percentOf(id), id).toBe(100);
      expect(v(op.live[id]!.hello!.activeVersion), id).toBe('1.1.0');
    }
  }, 60_000);

  test('node choice: with every node unchecked nothing is written', async () => {
    const { t, op } = await sentTo();
    op.takeFile('v1.1.0.dgb', RELEASE);
    for (const id of ['hub', 'seg', 'watch']) op.choose(id, false);
    await op.update();
    expect(op.state).toBe('fileReady');
    expect(t.reboots).toEqual([]);
  }, 20_000);

  test('the hub back by itself within 45 s: the reconnect dialog state (reconnectNeeded) is never entered', async () => {
    const { op } = await sentTo({ hubAway: 3 });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    const seen = new Set<string>();
    let running = true;
    const run = op.update().finally(() => (running = false));
    while (running) {
      seen.add(op.state);
      await new Promise((r) => setTimeout(r, 0));
    }
    await run;
    expect(seen.has('hubRestarting')).toBe(true);
    expect(seen.has('reconnectNeeded')).toBe(false);
    expect([op.state, op.startedAt]).toEqual(['done', null]);
  }, 60_000);

  test('auto reconnect failed (reconnectNeeded): a device the browser reports is taken by itself when it is the hub on trial with the version just sent', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('reconnectNeeded');
    t.replug();
    const took = await op.reconnectHub(async () => {
      await t.open();
      return t;
    }, true);
    expect([took, op.state]).toEqual([true, 'done']);
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.1.0', false]);
  }, 60_000);

  test('auto reconnect failed: a reported device that is not the hub on trial is left alone, with the reason noted', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity, fallBack: ['hub'] });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('reconnectNeeded');
    t.replug();
    const took = await op.reconnectHub(async () => {
      await t.open();
      return t;
    }, true);
    expect([took, op.state]).toEqual([false, 'reconnectNeeded']);
    expect(op.reconnectNotes).toEqual([{ kind: 'version', version: t.helloOf('hub').activeVersion, trial: false }]);
    // The user can still choose it: then it is shown stopped on its previous version (stopped).
    await op.reconnectHub();
    expect([op.state, op.progress.hub.stage]).toEqual(['stopped', 'previous']);
  }, 60_000);

  test('the header 切断 releases the hub to idle while idle, and does nothing while an update runs', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity });
    await op.disconnect();
    expect([op.state, op.status]).toEqual(['idle', ANNOUNCE.A21lost]);
    await expect(t.write(new Uint8Array(1))).rejects.toThrow('port not open');

    await op.connect();
    expect(op.state).toBe('ready');
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    const run = op.update();
    while (op.state !== 'writing') await new Promise((r) => setTimeout(r, 0));
    await op.disconnect();
    expect(op.state).not.toBe('idle');
    await run;
    expect(op.state).toBe('reconnectNeeded');
    await op.disconnect();
    expect(op.state).toBe('reconnectNeeded');
  }, 60_000);

  test('reconnectNeeded: two devices reported at once (another hub and this one) do not interleave; only this hub is taken', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('reconnectNeeded');
    const other = new MockTransport();
    t.replug();
    const open = (x: MockTransport) => async () => {
      await x.open();
      return x;
    };
    const took = await Promise.all([op.reconnectHub(open(other), true), op.reconnectHub(open(t), true)]);
    expect(took).toEqual([false, true]);
    expect(op.state).toBe('done');
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.1.0', false]);
    expect([v(other.helloOf('hub').activeVersion), other.reboots]).toEqual(['1.0.0', []]);
  }, 60_000);

  test.each([
    ['another hub (other MCU)', { hello: { ...SAMPLE_HUB_HELLO, mcuId: 9 } }, { kind: 'otherHub' }],
    ['a device that does not answer', { silent: true }, { kind: 'silent' }],
  ] as const)('reconnectNeeded: the user chose %s: closed at once, still reconnectNeeded, the reason in the log', async (_, opts, note) => {
    const { op } = await sentTo({ hubAway: Infinity });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('reconnectNeeded');
    const wrong = new MockTransport(opts);
    const took = await op.reconnectHub(async () => {
      await wrong.open();
      return wrong;
    });
    expect([took, op.state, op.alert]).toEqual([false, 'reconnectNeeded', ANNOUNCE.A21timeout]);
    expect(op.reconnectNotes).toContainEqual(note);
    await expect(wrong.write(new Uint8Array(1))).rejects.toThrow('port not open');
    expect(op.device).not.toBe(wrong);
  }, 60_000);

  test('after updating only the answering nodes, the ones left out stay visible and recoverable: done moves to nodesSilent with recovery first', async () => {
    const { op } = await sentTo({ quiet: ['i2c:66', 'uart'] });
    await op.round();
    await op.round();
    expect(bandOf(op.state, op.ctx).primary?.action).toBe('updateResponding');
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('done');
    await op.round();
    expect([op.state, op.stuck?.id, op.needUpdate]).toEqual(['nodesSilent', 'seg', 0]);
    const b = bandOf(op.state, op.ctx, { stuck: op.recoverTarget?.name });
    expect([b.primary?.label, b.secondary, b.sentence]).toEqual([BUTTON_FOR.recover('7セグ'), [], BAND.M62.sentence]);
  }, 60_000);

  test('a write that goes back leaves its record: the node, both versions, the reason and what the node reports then', async () => {
    const { op } = await sentTo({ fallBack: ['uart'] });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.failed, op.progress.watch.stage]).toEqual(['stopped', 'watch', 'previous']);
    // Asked at once: the question waits for the node's STATUS, so null can only mean it did not answer.
    expect(await op.settledRollbackRecord()).toEqual({
      node: 'watch',
      failed_version: '1.1.0',
      returned_version: '1.0.0',
      reason: 'ROLLED BACK',
      node_state: { trial: false, last_reason: 3 },
    });
  }, 60_000);

  test('the record of a hub found back on its previous version when chosen again', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity, fallBack: ['hub'] });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    t.replug();
    await op.reconnectHub();
    expect(op.rollbackRecord).toMatchObject({ node: 'hub', failed_version: '1.1.0', returned_version: '1.0.0', reason: 'ROLLED BACK' });
  }, 60_000);

  test('no record when no write went back, and none once the page has moved on from one', async () => {
    const { op } = await sentTo();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.rollbackRecord).toBeNull();

    const back = await sentTo({ fallBack: ['uart'] });
    back.op.takeFile('sample-1.1.0.dgb', RELEASE);
    await back.op.update();
    expect(back.op.rollbackRecord).not.toBeNull();
    back.op.reset();
    expect(back.op.rollbackRecord).toBeNull();
  }, 60_000);

  test('a node that stops without going back leaves no record', async () => {
    const { op } = await sentTo({ refuse: { uart: 0x07 } });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.progress.watch.stage]).toEqual(['stopped', 'refused']);
    expect(op.rollbackRecord).toBeNull();
  }, 60_000);

  test('done says all nodes run the new version only when they all do', async () => {
    const { op } = await sentTo();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.finished]).toEqual(['done', { kind: 'updated', left: 0 }]);
    expect(bandOf(op.state, op.ctx, { finish: op.finished }).sentence).toBe(BAND.M66.sentence);
  }, 60_000);

  test('done after updating only the answering nodes: says how many were not updated, and to recover them', async () => {
    const { op } = await sentTo({ quiet: ['i2c:66', 'uart'] });
    await op.round();
    await op.round();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.finished]).toEqual(['done', { kind: 'updated', left: 2 }]);
    const b = bandOf(op.state, op.ctx, { finish: op.finished });
    expect(b.sentence).toBe(BAND_FOR.M66some(2).sentence);
    expect(b.sentence).not.toBe(BAND.M66.sentence);
  }, 60_000);

  test('done after 「前のバージョンに戻す」: says the node runs its previous version', async () => {
    const { op } = await sentTo();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    await op.rollback('seg');
    expect([op.state, op.finished]).toEqual(['done', { kind: 'rolledBack', name: '7セグ', version: '1.0.0' }]);
    expect(bandOf(op.state, op.ctx, { finish: op.finished }).sentence).toBe(BAND_FOR.M66back('7セグ', '1.0.0').sentence);
  }, 60_000);

  test('the CLI gives the hub back: the page opens it again by itself and goes on watching (M67)', async () => {
    const t = new MockTransport();
    const op = new Operate({
      open: async () => {
        await t.open();
        return t;
      },
      again: async () => {
        await t.open();
        return t;
      },
      clock: fastClock(),
    });
    await op.connect();
    await op.push();
    await op.handOver();
    expect(op.state).toBe('cliTurn');
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M67.sentence);
    await op.cliReturned();
    expect([op.state, op.status]).toEqual(['ready', ANNOUNCE.A05]);
    await op.round();
    expect(op.responding).toBe(3);
  }, 20_000);

  test('the CLI gives back another 親機 than the one connected: closed, lost (M34), the reason in the log', async () => {
    const t = new MockTransport();
    const other = new MockTransport({ hello: { ...SAMPLE_HUB_HELLO, mcuId: 9 } });
    const op = new Operate({
      open: async () => {
        await t.open();
        return t;
      },
      again: async () => {
        await other.open();
        return other;
      },
      clock: fastClock(),
    });
    await op.connect();
    await op.push();
    await op.handOver();
    await op.cliReturned();
    expect([op.state, op.reconnectNotes]).toEqual(['lost', [{ kind: 'otherHub' }]]);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M34.sentence);
    await expect(other.write(new Uint8Array(1))).rejects.toThrow('port not open');
    // The reason shows on lost itself, as its cause chip; connecting again clears it.
    expect(op.returnRefused).toEqual({ kind: 'otherHub' });
    expect(bandOf(op.state, op.ctx, { refused: op.returnRefused?.kind })).toMatchObject({ chip: BAND_CHIP.otherHub, sentence: BAND.M34.sentence });
    await op.connect();
    expect(op.returnRefused).toBeNull();
  }, 20_000);

  test('the version to keep a reopened hub by is cleared once the hub is confirmed', async () => {
    class Recording extends MockTransport {
      expected: (number | null)[] = [];
      expectHub(v: number | null): void {
        this.expected.push(v);
      }
    }
    const t = new Recording();
    const op = new Operate({
      open: async () => {
        await t.open();
        return t;
      },
      clock: fastClock(),
    });
    await op.connect();
    await op.push();
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    t.expected = [];
    await op.update();
    expect(op.state).toBe('done');
    const sent = t.expected.findIndex((x) => x !== null);
    expect(v(t.expected[sent]!)).toBe('1.1.0');
    expect(t.expected[t.expected.length - 1]).toBeNull();
    expect(t.expected.slice(sent + 1)).toContain(null);
  }, 60_000);

  test('the CLI gives the hub back but it cannot be opened: lost, connect again (M34)', async () => {
    const t = new MockTransport();
    const op = new Operate({
      open: async () => {
        await t.open();
        return t;
      },
      again: async () => {
        throw new DOMException('busy', 'NetworkError');
      },
      clock: fastClock(),
    });
    await op.connect();
    await op.push();
    await op.handOver();
    await op.cliReturned();
    expect(op.state).toBe('lost');
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M34.sentence);
  }, 20_000);

  test('the water fill follows each node being written and drains when it is done or stopped', () => {
    expect(
      waterOf({
        hub: { stage: 'waiting', percent: 0 },
        seg: { stage: 'writing', percent: 62 },
        watch: { stage: 'rebooting', percent: 100 },
        x: { stage: 'done', percent: 100 },
        y: { stage: 'silent', percent: 40 },
        z: { stage: 'lastResort', percent: 140 },
      }),
    ).toEqual({ seg: 62, watch: 100, z: 100 });
  });

  test('the hub came back on its previous version: stopped there, marked previous', async () => {
    const { t, op } = await sentTo({ fallBack: ['hub'] });
    op.takeFile('sample-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.failed, op.progress.hub.stage]).toEqual(['stopped', 'hub', 'previous']);
    expect(op.status).toBe(ANNOUNCE.A13previous('メイン'));
    expect(v(t.helloOf('hub').activeVersion)).toBe('1.0.0');
  }, 60_000);
});
