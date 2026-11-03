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
    // The same board again: a second one of it; a board with no link of its own is a second 親機.
    expect(canPlace(doc, 'nucleo-f401re')).toBe(true);
    const two = place(doc, 'waveshare-rp2040-touch-lcd-1.28');
    expect(two.doc.topo.nodes.find((n) => n.id === two.id)?.role).toBe('coordinator');
    expect(two.doc.topo.update.order[two.doc.topo.update.order.length - 1]).toBe('hub');
    expect(check(toYaml(two.doc.topo)).errors).toBe(0);
  });

  test("the hub's new I2C bus and PIO UART link take its board's [coordinator] pins", () => {
    let doc = emptyDoc();
    for (const b of ['waveshare-rp2040-touch-lcd-1.28', 'nucleo-f401re', 'm5stopwatch']) doc = place(doc, b).doc;
    doc = connect(connect(doc, 'nucleo'), 'm5stopwatch');
    // The values the page held before it read them from the definition.
    expect(doc.topo.buses).toEqual([{ id: 'i2c-main', kind: 'i2c', controller: 'i2c0', sda: 16, scl: 17, speed_hz: 400_000, pullups: 'external' }]);
    expect(doc.topo.links!.find((l) => l.kind === 'uart')!.pins).toEqual({ tx: 26, rx: 27 });
    const yaml = toYaml(doc.topo);
    expect(yaml.indexOf('controller')).toBeLessThan(yaml.indexOf('sda: 16'));
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
    // Information, never an error: nothing tells the 構成 is wrong.
    expect(v23.every((p) => p.severity === 'info')).toBe(true);
    expect(problems(doc).filter((p) => p.severity === 'error')).toEqual([]);
    expect(v23[0].sentence).toContain('飛ばします');
    expect(v23[0].sentence).not.toContain('してください');
    // The CAN hop to it is not available yet either (V-24), told as skipped, not as a thing to fix.
    const v24 = problems(doc).find((p) => p.rule === 'V-24' && p.select === `link:${hub2}`)!;
    expect(v24.sentence).toContain('飛ばす');
    expect(v24.sentence).not.toContain('してください');
    const c = toConfig(doc, supportedBoards());
    // Linked from the first 親機, the second one sits below it, skipped as a second 親機's tree.
    expect(c.nodes.find((n) => n.id === hub2)).toMatchObject({ depth: 1, notYet: 'second-hub' });
    expect(c.nodes.filter((n) => n.notYet).map((n) => n.id)).toEqual([hub2]);
    expect(topo_compile(toYaml(doc.topo))).toEqual(topo_compile(DESK_DEMO_YAML));

    let deep = removeNode(fromYaml(DESK_DEMO_YAML), 'watch');
    deep = place(deep, 'm5stopwatch').doc;
    deep = connect(deep, 'm5stopwatch', 'seg');
    // No [coordinator] on the F401: UART0 pins of the 親機 MCU's definition (the former fixed 0/1).
    expect(deep.topo.links!.find((l) => l.from === 'seg')).toMatchObject({ iface: 'hw-uart0', pins: { tx: 0, rx: 1 } });
    const cfg = toConfig(deep, supportedBoards());
    expect(cfg.nodes.map((n) => [n.id, n.depth, n.notYet])).toEqual([['hub', 0, undefined], ['seg', 1, undefined], ['m5stopwatch', 2, 'deep']]);
    // Any depth: a node below the 孫機 is placed and connected too.
    const deeper = connect(place(deep, 'nucleo-f401re').doc, 'nucleo', 'm5stopwatch');
    expect(problems(deeper).filter((p) => p.severity === 'error')).toEqual([]);
    const dc = toConfig(deeper, supportedBoards());
    expect(dc.nodes.find((n) => n.id === 'nucleo')).toMatchObject({ depth: 3, notYet: 'deep' });
    expect(problems(deep).map((p) => p.rule)).toContain('V-23');
    // Valid as placed: the 孫機's link carries its board's way into its loader, so nothing is an error.
    expect(problems(deep).filter((p) => p.severity === 'error')).toEqual([]);
    expect(topo_check(toYaml(deep.topo))).toContain('V-23');
  });

  test('the 親機 uplink: set, written to the YAML (same compiled topology), and drawn as its L0', () => {
    const doc = setUplink(fromYaml(DESK_DEMO_YAML), 'hub', { kind: 'wifi-tcp', host: 'browser' });
    const yaml = toYaml(doc.topo);
    expect(yaml).toContain('uplink');
    expect(fromYaml(yaml).topo.nodes.find((n) => n.id === 'hub')?.uplink).toEqual({ kind: 'wifi-tcp', host: 'browser' });
    expect(topo_compile(yaml)).toEqual(topo_compile(DESK_DEMO_YAML));
    const hubOf = (c: ReturnType<typeof toConfig>) => c.nodes.find((n) => n.id === 'hub')!;
    expect(hubOf(toConfig(doc, supportedBoards())).uplink).toEqual({ kind: 'wifi-tcp', host: 'browser' });
    expect(hubOf(toConfig(fromYaml(DESK_DEMO_YAML), supportedBoards())).uplink).toBeUndefined();
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

  test('for generated 構成 made by placing and connecting, reach is the cut in 構成 order and every other node says why', () => {
    // ponytail: a small LCG over the editor's own operations; 60 shapes.
    let seed = 0x7090;
    const next = (n: number) => {
      seed = (seed * 1103515245 + 12345) % 2 ** 31;
      return seed % n;
    };
    const boards = ['waveshare-rp2040-touch-lcd-1.28', 'nucleo-f401re', 'm5stopwatch'];
    let compiled = 0;
    for (let k = 0; k < 60; k++) {
      let doc = place(emptyDoc(), boards[0]).doc;
      for (let i = 0, count = 1 + next(9); i < count; i++) {
        const r = place(doc, boards[next(boards.length)]);
        const ids = r.doc.topo.nodes.map((n) => n.id).filter((id) => id !== r.id);
        doc = r.doc.topo.nodes.find((n) => n.id === r.id)!.role === 'coordinator' && next(2) ? r.doc : connect(r.doc, r.id, ids[next(ids.length)]);
      }
      const c = toConfig(doc, supportedBoards());
      if (!check(toYaml(doc.topo)).ok) continue;
      // Total and disjoint: each node is either reached or told why.
      for (const n of c.nodes) expect(c.reach.includes(n.id), `${n.id} ${n.notYet}`).toBe(!n.notYet);
      // The cut keeps 構成 order, so it is the nodes' order with the skipped ones left out.
      expect(c.reach).toEqual(c.nodes.filter((n) => !n.notYet).map((n) => n.id));
      expect(c.reach).toContain('hub');
      expect(() => topo_compile(c.yaml)).not.toThrow();
      compiled++;
    }
    expect(compiled).toBeGreaterThan(10);
  });
});
