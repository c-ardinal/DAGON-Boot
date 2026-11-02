import { describe, expect, test } from 'vitest';
import { supportedBoards, topo_check, topo_compile } from '../../wasm/dagon.ts';
import { DESK_DEMO, DESK_DEMO_YAML } from '../sample.ts';
import { canPlace, connect, emptyDoc, freeAddrs, fromYaml, place, removeNode, ROM_ADDR, setLink, setLinkKind, setNode, toConfig, toYaml } from './model.ts';

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
    expect(freeAddrs(doc.topo)).not.toContain(ROM_ADDR);
  });

  test('removing a node removes its links, recovery and order entries', () => {
    const doc = removeNode(fromYaml(DESK_DEMO_YAML), 'watch');
    expect(doc.topo.nodes.map((n) => n.id)).toEqual(['hub', 'seg']);
    expect(doc.topo.links!.map((l) => l.to)).toEqual(['seg']);
    expect(doc.topo.update.order).toEqual(['seg', 'hub']);
    expect(check(toYaml(doc.topo)).ok).toBe(true);
  });
});
