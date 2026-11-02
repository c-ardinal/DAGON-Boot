import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import type { LinkClock } from '../serial/hostLink.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { versionText } from '../wasm/dagon.ts';
import { bandOf } from './band.ts';
import { ANNOUNCE, BAND, BAND_FOR, NODE } from './copy.ts';
import { Define } from './define/define.svelte.ts';
import { topoDiff } from './define/diff.ts';
import { fromYaml, type Topo } from './define/model.ts';
import { Operate } from './operate.svelte.ts';
import { DESK_DEMO, DESK_DEMO_YAML } from './sample.ts';

const repo = (rel: string) => new URL(`../../../${rel}`, import.meta.url);
/** dagon-sim's desk release 1.1.0: hub A/B, seg A/B and watch images for desk-demo, test key. */
const RELEASE = new Uint8Array(readFileSync(repo('crates/dagon-sim/fixtures/desk-1.1.0.dgb')));
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

async function sentTo(opts: MockOptions = {}) {
  const t = new MockTransport(opts);
  const op = new Operate({
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
  expect(op.state).toBe('L12');
  return { t, op };
}

const v = (raw: number) => versionText(raw);

describe('update from the page with dagon-host apply against the mock hub', () => {
  test('all three: children in update order, the hub last, reconnect after its reboot, each confirmed on 1.1.0', async () => {
    const { t, op } = await sentTo();
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    expect([op.state, op.file?.verdict, op.targets]).toEqual(['L18', 'pass', ['hub', 'seg', 'watch']]);
    expect(op.status).toBe(ANNOUNCE.A10ok(3));

    await op.update();
    expect(t.reboots).toEqual(['i2c:66', 'uart', 'hub']);
    expect(op.state).toBe('L23');
    expect(op.status).toBe(ANNOUNCE.A14('1.1.0'));
    expect(Object.values(op.progress).map((p) => p.stage)).toEqual(['done', 'done', 'done']);
    expect(op.overall).toBe(100);
    for (const k of ['hub', 'i2c:66', 'uart'] as const) {
      expect([v(t.helloOf(k).activeVersion), t.helloOf(k).trial], k).toEqual(['1.1.0', false]);
    }
  }, 60_000);

  test('the hub alone: with the nodes silent, "update the answering one first" updates only the hub', async () => {
    const { t, op } = await sentTo({ quiet: ['i2c:66', 'uart'] });
    await op.round();
    await op.round();
    expect(op.state).toBe('L13');
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    expect([op.state, op.targets]).toEqual(['L18', ['hub']]);

    await op.update();
    expect(op.state).toBe('L23');
    expect(t.reboots).toEqual(['hub']);
    expect(v(t.helloOf('hub').activeVersion)).toBe('1.1.0');
    expect(v(t.helloOf('i2c:66').activeVersion)).toBe('1.0.0');
  }, 60_000);

  test('a tampered release is refused before anything is sent', async () => {
    const { t, op } = await sentTo();
    const bad = RELEASE.slice();
    bad[bad.length - 1] ^= 1;
    op.takeFile('tampered.dgb', bad);
    expect([op.state, op.file?.verdict, op.refusal]).toEqual(['L16', 'source', 'source']);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M18.sentence);
    await op.update();
    expect([op.state, t.reboots]).toEqual(['L16', []]);
  }, 20_000);

  test('a release built for another configuration is refused (topology mismatch)', async () => {
    const { t, op } = await sentTo();
    op.config = { ...DESK_DEMO, yaml: SIM_PAIR_YAML };
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    expect([op.state, op.refusal]).toEqual(['L17', 'topology']);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M20.sentence);
    expect(t.reboots).toEqual([]);
  }, 20_000);

  test('L17: the differences are shown, and 定義 takes the 構成 of the file so the same file then passes', async () => {
    const { op } = await sentTo();
    const d = new Define();
    d.load(fromYaml(DESK_DEMO_YAML.replace('addr: 0x42', 'addr: 0x43')));
    op.useConfig(d.config);
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    expect([op.state, op.refusal]).toEqual(['L17', 'topology']);
    const file = op.fileTopology as unknown as Topo;
    const rows = topoDiff(d.doc.topo, file, d.doc.names);
    expect(rows).toEqual([expect.objectContaining({ here: expect.stringContaining('0x43'), file: expect.stringContaining('0x42'), nodes: ['seg'] })]);

    d.importRelease(file);
    expect(d.errors).toBe(0);
    op.useConfig(d.config);
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    expect([op.state, op.file?.verdict]).toEqual(['L18', 'pass']);
    d.undo();
    expect(d.doc.topo.links?.[0].addr).toBe(0x43);
  }, 20_000);

  test('not a release at all', async () => {
    const { op } = await sentTo();
    op.takeFile('notes.txt', new TextEncoder().encode('hello'));
    expect([op.state, op.refusal]).toEqual(['L16', 'format']);
    expect(bandOf(op.state, op.ctx).sentence).toBe(BAND.M19.sentence);
  }, 20_000);

  test('hub not back within 45 s: chosen again by the user, then dagon-host confirms it', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity });
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    const run = op.update();
    await new Promise((r) => setTimeout(r, 0));
    await run;
    expect(op.state).toBe('L21');
    expect(op.alert).toBe(ANNOUNCE.A21timeout);
    expect(t.helloOf('hub').trial).toBe(true);

    t.replug();
    await op.reconnectHub();
    expect(op.state).toBe('L23');
    expect([v(t.helloOf('hub').activeVersion), t.helloOf('hub').trial]).toEqual(['1.1.0', false]);
  }, 60_000);

  test('hub not back within 45 s and back on its previous version when chosen again: stopped there, not done', async () => {
    const { t, op } = await sentTo({ hubAway: Infinity, fallBack: ['hub'] });
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    await op.update();
    expect(op.state).toBe('L21');
    t.replug();
    await op.reconnectHub();
    expect(v(t.helloOf('hub').activeVersion)).toBe('1.0.0');
    expect([op.state, op.failed, op.progress.hub.stage]).toEqual(['L22', 'hub', 'previous']);
    expect(op.status).toBe(ANNOUNCE.A13previous('メイン'));
  }, 60_000);

  test('a USB disconnect while the hub reboots is not taken for a lost connection', async () => {
    const { op } = await sentTo({ hubAway: 5 });
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    const run = op.update();
    while (op.state !== 'L20') await new Promise((r) => setTimeout(r, 1));
    op.lose();
    await run;
    expect(op.state).toBe('L23');
  }, 60_000);

  test('a node that does not answer stops the run there; later nodes are not written', async () => {
    const { t, op } = await sentTo();
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    t.setQuiet('i2c:66', true);
    await op.update();
    expect([op.state, op.failed]).toEqual(['L22', 'seg']);
    expect(op.progress).toMatchObject({ seg: { stage: 'silent' }, watch: { stage: 'unwritten' }, hub: { stage: 'unwritten' } });
    expect(op.status).toBe(ANNOUNCE.A13('7セグ'));
    expect(bandOf(op.state, op.ctx, { stuck: '7セグ', unwritten: 2 })).toMatchObject(BAND_FOR.M29('7セグ'));
    expect(t.reboots).toEqual([]);
  }, 60_000);

  test.each([
    [0x07, 'E_FLASH', '× 書き込めない'],
    [0x09, 'E_SIGNATURE', '× 配布元を確認できない'],
  ])('a node that refuses (status %i) is shown with its reason, not as not answering', async (status, reason, chip) => {
    const { t, op } = await sentTo({ refuse: { 'i2c:66': status } });
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.failed]).toEqual(['L22', 'seg']);
    expect(op.progress.seg).toMatchObject({ stage: 'refused', reason });
    expect(NODE.refused[reason]).toBe(chip);
    expect(op.progress).toMatchObject({ watch: { stage: 'unwritten' }, hub: { stage: 'unwritten' } });
    expect(t.reboots).toEqual([]);
  }, 60_000);

  test('the hub came back on its previous version: stopped there, marked previous', async () => {
    const { t, op } = await sentTo({ fallBack: ['hub'] });
    op.takeFile('desk-1.1.0.dgb', RELEASE);
    await op.update();
    expect([op.state, op.failed, op.progress.hub.stage]).toEqual(['L22', 'hub', 'previous']);
    expect(op.status).toBe(ANNOUNCE.A13previous('メイン'));
    expect(v(t.helloOf('hub').activeVersion)).toBe('1.0.0');
  }, 60_000);
});
