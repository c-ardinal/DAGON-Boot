import { describe, expect, test } from 'vitest';
import { supportedBoards, topo_check, topo_compile } from '../../wasm/dagon.ts';
import { rom_i2c_addrs } from '../../wasm/pkg/dagon_wasm.js';
import { DESK_DEMO, DESK_DEMO_YAML } from '../sample.ts';
import { boardMissing, canPlace, connect, emptyDoc, freeAddrs, fromYaml, hopAvailable, linkKinds, place, placeMcu, removeNode, setBoard, setLink, setLinkKind, setNode, setUplink, toConfig, toYaml } from './model.ts';
import { problemChip } from './problems.ts';
import { problems } from './problems.ts';

const check = (yaml: string) => JSON.parse(topo_check(yaml)) as { ok: boolean; errors: number; warnings: number; issues: { rule: string; path: string }[] };

describe('define document', () => {
  test('desk-demo reads and writes back to the same canonical topology', () => {
    const doc = fromYaml(DESK_DEMO_YAML);
    const out = toYaml(doc.topo);
    expect(check(out).ok).toBe(true);
    expect(topo_compile(out)).toEqual(topo_compile(DESK_DEMO_YAML));
    expect(out).toContain('addr: 0x42');
  });

  test('the graph view of desk-demo matches the sample the operate mode draws', () => {
    const names = Object.fromEntries(DESK_DEMO.nodes.map((n) => [n.id, n.name]));
    const c = toConfig(fromYaml(DESK_DEMO_YAML, names), supportedBoards());
    expect(c.nodes.map((n) => [n.id, n.name, n.role, n.via, n.reset, n.boot, n.target])).toEqual(
      DESK_DEMO.nodes.map((n) => [n.id, n.name, n.role, n.via, n.reset, n.boot, n.target]),
    );
    expect(c.order).toEqual(DESK_DEMO.order);
    expect(c.links.map((l) => [l.from, l.to, l.kind, l.addr, l.wires.length])).toEqual(DESK_DEMO.links.map((l) => [l.from, l.to, l.kind, l.addr, l.wires.length]));
  });

  test('placing the three desk boards and connecting them checks clean (wiring left as a warning)', () => {
    let doc = emptyDoc();
    for (const b of ['waveshare-rp2040-touch-lcd-1.28', 'nucleo-f401re', 'm5stopwatch']) doc = place(doc, b).doc;
    expect(doc.topo.nodes.map((n) => [n.id, n.role])).toEqual([
      ['hub', 'coordinator'],
      ['nucleo', 'node'],
      ['m5stopwatch', 'node'],
    ]);
    expect(check(toYaml(doc.topo)).issues.map((i) => i.rule)).toContain('V-05');
    doc = connect(connect(doc, 'nucleo'), 'm5stopwatch');
    const r = check(toYaml(doc.topo));
    expect([r.ok, r.errors]).toEqual([true, 0]);
    expect(doc.topo.update.order).toEqual(['nucleo', 'm5stopwatch', 'hub']);
    expect(canPlace(doc, 'nucleo-f401re')).toBe(false);
  });

  test('renaming a node carries the id into links, recovery, order and names', () => {
    let doc = fromYaml(DESK_DEMO_YAML, { seg: '7セグ' });
    doc = setNode(doc, 'seg', 'id', 'seven');
    expect(doc.topo.links!.some((l) => l.to === 'seven')).toBe(true);
    expect(doc.topo.recovery!.seven).toBeDefined();
    expect(doc.topo.update.order).toEqual(['seven', 'watch', 'hub']);
    expect(doc.names).toEqual({ seven: '7セグ' });
    expect(check(toYaml(doc.topo)).ok).toBe(true);
  });

  test('a duplicate address is reported by the check, and free addresses skip the ROM loader', () => {
    let doc = fromYaml(DESK_DEMO_YAML);
    doc = setLinkKind(doc, 1, 'i2c');
    doc = setLink(doc, 1, 'addr', 0x42);
    expect(check(toYaml(doc.topo)).issues.map((i) => i.rule)).toContain('V-18');
    // The F401's ROM loader (its definition's [rom_i2c]) is the only one defined.
    expect([...rom_i2c_addrs()]).toEqual([0x39]);
    expect(freeAddrs(doc.topo)).not.toContain(0x39);
  });

  test('removing a node removes its links, recovery and order entries', () => {
    const doc = removeNode(fromYaml(DESK_DEMO_YAML), 'watch');
    expect(doc.topo.nodes.map((n) => n.id)).toEqual(['hub', 'seg']);
    expect(doc.topo.links!.map((l) => l.to)).toEqual(['seg']);
    expect(doc.topo.update.order).toEqual(['seg', 'hub']);
    expect(check(toYaml(doc.topo)).ok).toBe(true);
  });

  test('chosen by MCU: no board, the board fields asked for (B-01), a flash too small refused (B-02)', () => {
    let doc = placeMcu(emptyDoc(), 'rp2040').doc;
    doc = placeMcu(doc, 'stm32f401re').doc;
    expect(doc.topo.nodes.map((n) => [n.id, n.role, n.board])).toEqual([
      ['hub', 'coordinator', undefined],
      ['stm32f401re', 'node', undefined],
    ]);
    doc = connect(doc, 'stm32f401re');
    expect(boardMissing(doc, 'hub')).toEqual(['flash']);
    expect(problems(doc).filter((p) => p.rule === 'B-01').length).toBe(2);
    doc = setBoard(setBoard(doc, 'hub', { flashBytes: 2 << 20 }), 'stm32f401re', { flashBytes: 512 << 10 });
    const b02 = problems(doc).filter((p) => p.rule === 'B-02');
    expect(b02.map((p) => [p.select, p.sentence])).toEqual([['node:hub', 'hubは容量が 4 MiB 以上のボードにしてください。']]);
    doc = setBoard(doc, 'hub', { flashBytes: 4 << 20 });
    expect(problems(doc).filter((p) => p.severity === 'error')).toEqual([]);
  });

  test('any shape: a second 親機 over CAN and a grandchild are drawn in rows and told as not updatable yet (V-23)', () => {
    let doc = fromYaml(DESK_DEMO_YAML);
    doc = placeMcu(doc, 'rp2040').doc;
    const hub2 = doc.topo.nodes[doc.topo.nodes.length - 1].id;
    doc = setNode(doc, hub2, 'role', 'coordinator');
    doc = connect(doc, hub2, 'hub');
    expect(doc.topo.links![doc.topo.links!.length - 1]).toMatchObject({ from: 'hub', to: hub2, kind: 'can' });
    const v23 = problems(doc).filter((p) => p.rule === 'V-23');
    expect(v23.length).toBeGreaterThan(0);
    expect(v23[0].sentence).toContain('更新するには');
    const c = toConfig(doc, supportedBoards());
    expect(c.nodes.find((n) => n.id === hub2)!.depth).toBe(0);

    let deep = removeNode(fromYaml(DESK_DEMO_YAML), 'watch');
    deep = place(deep, 'm5stopwatch').doc;
    deep = connect(deep, 'm5stopwatch', 'seg');
    const cfg = toConfig(deep, supportedBoards());
    expect(cfg.nodes.map((n) => [n.id, n.depth])).toEqual([['hub', 0], ['seg', 1], ['m5stopwatch', 2]]);
    expect(problems(deep).map((p) => p.rule)).toContain('V-23');
    expect(topo_check(toYaml(deep.topo))).toContain('V-23');
  });

  test('the 親機 uplink: set, written to the YAML (same compiled topology), and drawn as its L0', () => {
    const doc = setUplink(fromYaml(DESK_DEMO_YAML), 'hub', { kind: 'wifi-tcp', host: 'browser' });
    const yaml = toYaml(doc.topo);
    expect(yaml).toContain('uplink');
    expect(fromYaml(yaml).topo.nodes.find((n) => n.id === 'hub')?.uplink).toEqual({ kind: 'wifi-tcp', host: 'browser' });
    expect(topo_compile(yaml)).toEqual(topo_compile(DESK_DEMO_YAML));
    expect(toConfig(doc, supportedBoards()).uplink).toEqual({ kind: 'wifi-tcp', host: 'browser' });
    expect(toConfig(fromYaml(DESK_DEMO_YAML), supportedBoards()).uplink).toBeUndefined();
    expect(setUplink(doc, 'hub', null).topo.nodes.find((n) => n.id === 'hub')?.uplink).toBeUndefined();
  });

  test('hops the firmware cannot do yet are known, and reported as まだ使えないつながり (◇)', () => {
    expect(linkKinds('rp2040')).toEqual({ up: ['usb-cdc'], down: ['i2c', 'uart'] });
    const doc = fromYaml(DESK_DEMO_YAML);
    const [hub, seg] = ['hub', 'seg'].map((id) => doc.topo.nodes.find((n) => n.id === id));
    expect(hopAvailable('i2c', hub, seg)).toBe(true);
    expect(hopAvailable('ble', hub, seg)).toBe(false);
    expect(hopAvailable('usb-cdc', undefined, hub)).toBe(true);
    expect(hopAvailable('wifi-tcp', undefined, hub)).toBe(false);
    const yaml = toYaml(setUplink(doc, 'hub', { kind: 'ble', host: 'browser' }).topo);
    const r = check(yaml);
    expect(r.issues.map((i) => i.rule)).toEqual(['V-24']);
    expect(problemChip({ rule: 'V-24', severity: 'warning' })).toEqual({ mark: '◇', kind: 'unsupported' });
    const ps = problems(setUplink(doc, 'hub', { kind: 'ble', host: 'browser' }));
    expect(ps.some((p) => p.rule === 'V-24')).toBe(true);
  });
});
