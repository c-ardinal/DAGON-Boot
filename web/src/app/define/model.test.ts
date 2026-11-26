import { DIFF, RULE_FIX } from '../copy.ts';
import { describe, expect, test } from 'vitest';
import { supportedBoards, topo_check, topo_compile } from '../../wasm/dagon.ts';
import { rom_i2c_addrs, schema_version, topo_parse } from '../../wasm/pkg/dagon_wasm.js';
import { SAMPLE_CONFIG, SAMPLE_YAML } from '../sample.ts';
import { canPlace, LinkKind, mcus, ownFlash, maxNodes, connect, emptyDoc, freeAddrs, fromYaml, hopAvailable, linkKinds, linkView, SCHEMA_VERSION, place, placeMcu, removeNode, setLink, setLinkKind, setNode, setUplink, toConfig, topoWithNames, toYaml, withDefaults, type Topo } from './model.ts';
import { topoDiff } from './diff.ts';
import { problemChip } from './problems.ts';
import { problems } from './problems.ts';

const check = (yaml: string) => JSON.parse(topo_check(yaml)) as { ok: boolean; errors: number; warnings: number; issues: { rule: string; path: string }[] };

describe('define document', () => {
  test('the Console draws up to the format\'s node limit, and its sentence names it', () => {
    expect(maxNodes()).toBe(32);
    expect(RULE_FIX['V-02']({ to: '', board: '', id: '', many: true, max: maxNodes() })).toContain('32');
  });

  test('a new 構成 is written with the topology YAML number dagon-topology reads', () => {
    expect(SCHEMA_VERSION).toBe(schema_version());
    expect(emptyDoc().topo.dagon).toBe(schema_version());
  });

  test('each link end is written on one line, the way the sample writes it', () => {
    const out = toYaml(fromYaml(SAMPLE_YAML).topo);
    expect(out).toContain('      - { node: seg, port: i2c3, addr: 0x42, node_pins: { sda: PB4, scl: PA8 } }');
  });

  test('sample reads and writes back to the same canonical topology', () => {
    const doc = fromYaml(SAMPLE_YAML);
    const out = toYaml(doc.topo);
    expect(check(out).ok).toBe(true);
    expect(topo_compile(out)).toEqual(topo_compile(SAMPLE_YAML));
    expect(out).toContain('addr: 0x42');
  });

  test('the graph view of sample matches the sample the operate mode draws', () => {
    const names = Object.fromEntries(SAMPLE_CONFIG.nodes.map((n) => [n.id, n.name]));
    const c = toConfig(fromYaml(SAMPLE_YAML, names), supportedBoards());
    expect(c.nodes.map((n) => [n.id, n.name, n.role, n.via, n.reset, n.boot])).toEqual(
      SAMPLE_CONFIG.nodes.map((n) => [n.id, n.name, n.role, n.via, n.reset, n.boot]),
    );
    expect(c.order).toEqual(SAMPLE_CONFIG.order);
    expect(c.links.map((l) => [l.from, l.to, l.kind, l.addr, l.wires.length])).toEqual(SAMPLE_CONFIG.links.map((l) => [l.from, l.to, l.kind, l.addr, l.wires.length]));
  });

  test('placing the three sample boards and connecting them checks clean (wiring left as a warning)', () => {
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
    // The same board again: a second one of it; a board with no link of its own is a second coordinator.
    expect(canPlace(doc, 'nucleo-f401re')).toBe(true);
    const two = place(doc, 'waveshare-rp2040-touch-lcd-1.28');
    expect(two.doc.topo.nodes.find((n) => n.id === two.id)?.role).toBe('coordinator');
    expect(two.doc.topo.update.order[two.doc.topo.update.order.length - 1]).toBe('hub');
    expect(check(toYaml(two.doc.topo)).errors).toBe(0);
  });

  test("the coordinator's new I2C bus and PIO UART link take its board's DAGON bus pins", () => {
    let doc = emptyDoc();
    for (const b of ['waveshare-rp2040-touch-lcd-1.28', 'nucleo-f401re', 'm5stopwatch']) doc = place(doc, b).doc;
    doc = connect(connect(doc, 'nucleo'), 'm5stopwatch');
    // The values the page held before it read them from the definition.
    expect(doc.topo.buses).toEqual([{ id: 'i2c-main', kind: 'i2c', controller: 'i2c0', sda: 16, scl: 17, speed_hz: 100_000, pullups: 'external' }]);
    expect(linkView(doc.topo, doc.topo.links!.find((l) => l.kind === LinkKind.Uart)!).pins).toEqual({ tx: 26, rx: 27 });
    const yaml = toYaml(doc.topo);
    expect(yaml.indexOf('controller')).toBeLessThan(yaml.indexOf('sda: 16'));
  });

  test('renaming a node carries the id into links, recovery, order and names', () => {
    let doc = fromYaml(SAMPLE_YAML, { seg: '7セグ' });
    doc = setNode(doc, 'seg', 'id', 'seven');
    expect(doc.topo.links!.some((l) => l.ends.some((e) => e.node === 'seven'))).toBe(true);
    expect(doc.topo.recovery!.seven).toBeDefined();
    expect(doc.topo.update.order).toEqual(['seven', 'watch', 'hub']);
    expect(doc.names).toEqual({ seven: '7セグ' });
    expect(check(toYaml(doc.topo)).ok).toBe(true);
  });

  test('a duplicate address is reported by the check, and free addresses skip the ROM loader', () => {
    let doc = fromYaml(SAMPLE_YAML);
    doc = setLinkKind(doc, 1, LinkKind.I2c);
    doc = setLink(doc, 1, 'addr', 0x42);
    expect(check(toYaml(doc.topo)).issues.map((i) => i.rule)).toContain('V-18');
    // The F401's ROM loader (its definition's [rom_i2c]) is the only one defined.
    expect([...rom_i2c_addrs()]).toEqual([0x39]);
    expect(freeAddrs(doc.topo)).not.toContain(0x39);
  });

  test('removing a node removes its links, recovery and order entries', () => {
    const doc = removeNode(fromYaml(SAMPLE_YAML), 'watch');
    expect(doc.topo.nodes.map((n) => n.id)).toEqual(['hub', 'seg']);
    expect(doc.topo.links!.map((l) => linkView(doc.topo, l).to)).toEqual(['seg']);
    expect(doc.topo.update.order).toEqual(['seg', 'hub']);
    expect(check(toYaml(doc.topo)).ok).toBe(true);
  });

  test('chosen by MCU: no board entry, no B-01/B-02, the own flash of the MCU from its definition', () => {
    let doc = placeMcu(emptyDoc(), 'rp2040').doc;
    doc = placeMcu(doc, 'stm32f401re').doc;
    expect(doc.topo.nodes.map((n) => [n.id, n.role, n.board])).toEqual([
      ['hub', 'coordinator', undefined],
      ['stm32f401re', 'node', undefined],
    ]);
    doc = connect(doc, 'stm32f401re');
    expect(Object.keys(doc)).not.toContain('boards');
    expect(problems(doc).filter((p) => p.rule === 'B-01' || p.rule === 'B-02')).toEqual([]);
    expect(ownFlash(doc, 'hub')).toBe(mcus().find((m) => m.id === 'rp2040')!.flashBytes);
    expect(ownFlash(doc, 'stm32f401re')).toBe(mcus().find((m) => m.id === 'stm32f401re')!.flashBytes);
  });

  test('a node that names a board has no own flash line (the board definition holds it)', () => {
    expect(ownFlash(fromYaml(SAMPLE_YAML), 'hub')).toBeNull();
  });

  test('a ring: a link between two nodes under the coordinator has its own key, is edited by itself, and is listed as its own difference', () => {
    const sample = fromYaml(SAMPLE_YAML);
    const ring = connect(sample, 'watch', 'seg');
    expect(ring.topo.links).toHaveLength(3);
    const config = toConfig(ring, supportedBoards());
    expect(config.links.map((l) => [l.key, l.from, l.to])).toEqual([['link:seg', 'hub', 'seg'], ['link:watch', 'hub', 'watch'], ['link:watch~seg', 'seg', 'watch']]);
    // Each node stands under its link from the coordinator, and updating reaches it over that link.
    expect(config.nodes.map((n) => n.depth)).toEqual([0, 1, 1]);
    expect(config.reach).toEqual(['hub', 'seg', 'watch']);
    expect(check(config.yaml).errors).toBe(0);
    expect(problems(ring).filter((p) => p.severity === 'error')).toEqual([]);
    // What the check says of the ring's link selects that link, not the link watch stands under.
    expect(problems(ring).filter((p) => p.rule === 'V-23').map((p) => p.select)).toEqual(['link:watch~seg']);
    // The same pair again changes nothing, from either end.
    expect(connect(ring, 'seg', 'watch')).toBe(ring);
    expect(connect(ring, 'watch', 'seg')).toBe(ring);
    // Editing the ring's link leaves the link watch stands under as it was.
    const edited = setLink(ring, 2, 'baud', 9600);
    expect(edited.topo.links!.map((l) => l.baud)).toEqual([undefined, 115200, 9600]);
    // Each link drawn carries its own speed for its label.
    expect(toConfig(edited, supportedBoards()).links.map((l) => l.baud)).toEqual([undefined, 115200, 9600]);
    expect(topoDiff(sample.topo, ring.topo, {})).toEqual([{ what: 'seg → watch', here: DIFF.none, file: expect.stringContaining('UART'), nodes: ['watch'] }]);
    // The file written and read back is the same ring.
    expect(fromYaml(toYaml(ring.topo)).topo).toEqual(ring.topo);
  });

  test('any shape: a second coordinator over CAN and a node two links from the coordinator are drawn in rows and told as skipped by updating (V-23)', () => {
    let doc = fromYaml(SAMPLE_YAML);
    doc = placeMcu(doc, 'rp2040').doc;
    const hub2 = doc.topo.nodes[doc.topo.nodes.length - 1].id;
    doc = setNode(doc, hub2, 'role', 'coordinator');
    doc = connect(doc, hub2, 'hub');
    // Two RP2040s: no CAN port on either, so the first kind both define a port for.
    expect(linkView(doc.topo, doc.topo.links![doc.topo.links!.length - 1])).toMatchObject({ from: 'hub', to: hub2, kind: LinkKind.UsbCdc });
    const v23 = problems(doc).filter((p) => p.rule === 'V-23');
    expect(v23.length).toBeGreaterThan(0);
    // Information, never an error: nothing tells the 構成 is wrong.
    expect(v23.every((p) => p.severity === 'info')).toBe(true);
    expect(problems(doc).filter((p) => p.severity === 'error')).toEqual([]);
    expect(v23[0].sentence).toContain('スキップします');
    expect(v23[0].sentence).not.toContain('してください');
    // The CAN hop to it is not available either, told as skipped, not as a thing to fix.
    const v24 = problems(doc).find((p) => p.rule === 'V-24' && p.select === `link:${hub2}`)!;
    expect(v24.sentence).toContain('スキップ');
    expect(v24.sentence).not.toContain('してください');
    const c = toConfig(doc, supportedBoards());
    // Linked from the first coordinator, the second one sits below it, skipped as a second coordinator's tree.
    expect(c.nodes.find((n) => n.id === hub2)).toMatchObject({ depth: 1, skip: 'second-hub' });
    expect(c.nodes.filter((n) => n.skip).map((n) => n.id)).toEqual([hub2]);
    expect(topo_compile(toYaml(doc.topo))).toEqual(topo_compile(SAMPLE_YAML));

    let deep = removeNode(fromYaml(SAMPLE_YAML), 'watch');
    deep = place(deep, 'm5stopwatch').doc;
    deep = connect(deep, 'm5stopwatch', 'seg');
    // No [coordinator] on the F401: the first UART0 pins the coordinator's definitions allow together (its MCU's pairs on its board's header).
    expect(deep.topo.links!.map((l) => linkView(deep.topo, l)).find((l) => l.from === 'seg')).toMatchObject({ iface: 'hw-uart0', pins: { tx: 16, rx: 17 } });
    const cfg = toConfig(deep, supportedBoards());
    expect(cfg.nodes.map((n) => [n.id, n.depth, n.skip])).toEqual([['hub', 0, undefined], ['seg', 1, undefined], ['m5stopwatch', 2, 'deep']]);
    // Any depth: a node at the other end of a link from that node is placed and connected too.
    const deeper = connect(place(deep, 'nucleo-f401re').doc, 'nucleo', 'm5stopwatch');
    expect(problems(deeper).filter((p) => p.severity === 'error')).toEqual([]);
    const dc = toConfig(deeper, supportedBoards());
    expect(dc.nodes.find((n) => n.id === 'nucleo')).toMatchObject({ depth: 3, skip: 'deep' });
    expect(problems(deep).map((p) => p.rule)).toContain('V-23');
    // Valid as placed: that node's link carries its board's way into its loader, so nothing is an error.
    expect(problems(deep).filter((p) => p.severity === 'error')).toEqual([]);
    expect(topo_check(toYaml(deep.topo))).toContain('V-23');
  });

  test('the coordinator uplink: set, written to the YAML (same compiled topology), and drawn as its L0', () => {
    const doc = setUplink(fromYaml(SAMPLE_YAML), 'hub', { kind: LinkKind.WifiTcp, host: 'browser' });
    const yaml = toYaml(doc.topo);
    expect(yaml).toContain('uplink');
    expect(fromYaml(yaml).topo.nodes.find((n) => n.id === 'hub')?.uplink).toEqual({ kind: LinkKind.WifiTcp, host: 'browser' });
    expect(topo_compile(yaml)).toEqual(topo_compile(SAMPLE_YAML));
    const hubOf = (c: ReturnType<typeof toConfig>) => c.nodes.find((n) => n.id === 'hub')!;
    expect(hubOf(toConfig(doc, supportedBoards())).uplink).toEqual({ kind: LinkKind.WifiTcp, host: 'browser' });
    expect(hubOf(toConfig(fromYaml(SAMPLE_YAML), supportedBoards())).uplink).toBeUndefined();
    expect(setUplink(doc, 'hub', null).topo.nodes.find((n) => n.id === 'hub')?.uplink).toBeUndefined();
  });

  test('hops the firmware does not run are known, and reported as 使用不可の接続 (◇)', () => {
    expect(linkKinds('rp2040')).toEqual({ up: [LinkKind.UsbCdc], down: [LinkKind.I2c, LinkKind.Uart] });
    const doc = fromYaml(SAMPLE_YAML);
    const [hub, seg] = ['hub', 'seg'].map((id) => doc.topo.nodes.find((n) => n.id === id));
    expect(hopAvailable(LinkKind.I2c, hub, seg)).toBe(true);
    expect(hopAvailable(LinkKind.Ble, hub, seg)).toBe(false);
    expect(hopAvailable(LinkKind.UsbCdc, undefined, hub)).toBe(true);
    expect(hopAvailable(LinkKind.WifiTcp, undefined, hub)).toBe(false);
    const yaml = toYaml(setUplink(doc, 'hub', { kind: LinkKind.Ble, host: 'browser' }).topo);
    const r = check(yaml);
    expect(r.issues.map((i) => i.rule)).toEqual(['V-24']);
    expect(problemChip({ rule: 'V-24', severity: 'warning' })).toEqual({ mark: '◇', kind: 'unsupported' });
    const ps = problems(setUplink(doc, 'hub', { kind: LinkKind.Ble, host: 'browser' }));
    expect(ps.some((p) => p.rule === 'V-24')).toBe(true);
  });

  test('for generated 構成 made by placing and connecting, reach is the cut in 構成 order and every other node says why', () => {
    // a small LCG over the editor's own operations; 60 shapes.
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
      for (const n of c.nodes) expect(c.reach.includes(n.id), `${n.id} ${n.skip}`).toBe(!n.skip);
      // The cut keeps 構成 order, so it is the nodes' order with the skipped ones left out.
      expect(c.reach).toEqual(c.nodes.filter((n) => !n.skip).map((n) => n.id));
      expect(c.reach).toContain('hub');
      // The compiled table (dagon-wasm's topo_compile) lists its nodes in reach's order. Layout
      // (dagon-format topology): node_count at byte 6, nodes of 40 bytes from 0x20, target_tag at +16.
      const bin = topo_compile(c.yaml);
      const tags = Array.from({ length: bin[6] }, (_, i) =>
        new TextDecoder().decode(bin.slice(0x20 + 40 * i + 16, 0x20 + 40 * i + 32)).replace(/\0+$/, ''),
      );
      expect(tags).toEqual(c.reach.map((id) => doc.topo.nodes.find((n) => n.id === id)!.tag));
      compiled++;
    }
    expect(compiled).toBeGreaterThan(10);
  });
});

describe('kinds by code, files by name', () => {
  const files = import.meta.glob<string>('../../../../topology/*.yaml', { query: '?raw', import: 'default', eager: true });
  const withUplink = SAMPLE_YAML.replace('  - id: hub\n', '  - id: hub\n    uplink: { kind: wifi-tcp, host: browser }\n');
  const unknownKind = SAMPLE_YAML.replace('  - kind: uart', '  - kind: spi');
  test('the page keeps codes and writes each file back with its kinds by name, every key in its place', () => {
    expect(Object.keys(files).length).toBeGreaterThan(1);
    for (const y of [...Object.values(files), withUplink, unknownKind]) {
      const doc = fromYaml(y);
      expect(doc.topo.links!.every((l) => typeof l.kind === 'number' || l.kind === 'spi')).toBe(true);
      // The writer is given the file as read, so the YAML it writes is the one it wrote when kinds were names.
      expect(JSON.stringify(topoWithNames(doc.topo))).toBe(JSON.stringify(withDefaults(topo_parse(y) as Topo)));
    }
    expect(fromYaml(withUplink).topo.nodes.find((n) => n.id === 'hub')?.uplink?.kind).toBe(LinkKind.WifiTcp);
    expect(toYaml(fromYaml(unknownKind).topo)).toContain('- kind: spi');
  });
});
