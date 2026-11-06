import { describe, expect, test } from 'vitest';
import { RULE_FIX } from '../copy.ts';
import { Define } from './define.svelte.ts';
import { connect, emptyDoc, fromYaml, placeMcu, setLink, setLinkKind, setNode } from './model.ts';
import { SAMPLE_YAML } from '../sample.ts';
import { problemChip } from './problems.ts';
import { LINK, LINK_WHY } from '../copy.ts';

describe('define session', () => {
  test('a part dropped under a card is placed and linked under it in one edit; elsewhere it stands alone', () => {
    const d = new Define();
    d.placeAt({ board: 'waveshare-rp2040-touch-lcd-1.28' }, 'Waveshare RP2040-Touch-LCD-1.28', null);
    d.placeAt({ board: 'nucleo-f401re' }, 'NUCLEO-F401RE', 'hub');
    expect(d.doc.topo.links?.map((l) => [l.from, l.to])).toEqual([['hub', 'nucleo']]);
    d.undo();
    expect(d.doc.topo.nodes.map((n) => n.id)).toEqual(['hub']);
    d.placeAt({ mcu: 'stm32f401re' }, 'STM32F401RE（自作ボード）', null);
    expect(d.doc.topo.links ?? []).toEqual([]);
  });

  test('importing a topology identical to the 構成 is no change: no revision, nothing to save, no undo step', () => {
    const d = new Define();
    d.load(fromYaml(SAMPLE_YAML));
    const rev = d.revision;
    // A fresh object with the same content (a release made for this very 構成).
    expect(d.importRelease(JSON.parse(JSON.stringify(d.doc.topo)))).toBe(false);
    expect([d.revision, d.unsaved, d.canUndo]).toEqual([rev, false, false]);
    // The same through apply from any caller (table, proposal, import of the YAML).
    expect(d.apply(JSON.parse(JSON.stringify(d.doc)))).toBe(false);
    expect(d.importYaml(SAMPLE_YAML)).toBe(true);
    expect([d.revision, d.unsaved]).toEqual([rev, false]);
    // A real difference is still an edit.
    expect(d.importRelease({ ...d.doc.topo, name: 'other' })).toBe(true);
    expect(d.unsaved).toBe(true);
  });

  test('an empty entry the file writes is content: recovery for a node that is not there is a change, and the check reports it', () => {
    const d = new Define();
    d.load(fromYaml(SAMPLE_YAML));
    expect(d.problems.filter((p) => p.path.includes('ghost'))).toEqual([]);
    const t = d.doc.topo;
    expect(d.importRelease({ ...t, recovery: { ...t.recovery, ghost: [] } })).toBe(true);
    expect(d.unsaved).toBe(true);
    expect(d.problems.some((p) => p.path.includes('ghost'))).toBe(true);
  });

  test('an identical import while editing is stopped: no change, so nothing is asked and nothing moves', () => {
    const d = new Define();
    d.load(fromYaml(SAMPLE_YAML));
    const rev = d.revision;
    let asked = 0;
    d.guard = () => (asked++, true);
    expect(d.importRelease(JSON.parse(JSON.stringify(d.doc.topo)))).toBe(false);
    expect([asked, d.revision, d.unsaved]).toEqual([0, rev, false]);
  });

  test('editing stopped by the lock table: no path changes the 構成 (keys, double-click, drag, import, undo)', () => {
    const d = new Define();
    d.placeSample();
    const before = JSON.stringify(d.doc);
    let asked = 0;
    d.guard = () => (asked++, true);
    // Enter / double-click in the parts window, the table's 置く, a drop, a port drag, Delete, import, undo.
    d.place('nucleo-f401re', 'NUCLEO-F401RE');
    d.placeMcu('stm32f401re', 'STM32F401RE（自作ボード）');
    d.placeAt({ board: 'nucleo-f401re' }, 'NUCLEO-F401RE', 'hub');
    d.link('seg', 'watch');
    d.selected = 'node:seg';
    d.removeSelected();
    expect(d.importRelease({ ...d.doc.topo, nodes: d.doc.topo.nodes.slice(0, 1) })).toBe(false);
    expect(d.importYaml(SAMPLE_YAML)).toBe(false);
    d.undo();
    expect(JSON.stringify(d.doc)).toBe(before);
    // Seven edits asked (linking an already linked node changes nothing and needs no asking).
    expect(asked).toBe(7);
  });

  test('a port drag links a node under another; a taken node and a loop are refused with why', () => {
    const d = new Define();
    d.placeSample();
    expect(d.linkRefusal('hub', 'seg')).toBe(LINK_WHY.taken);
    d.placeAt({ mcu: 'stm32f401re' }, 'STM32F401RE（自作ボード）', null);
    const id = d.doc.topo.nodes[d.doc.topo.nodes.length - 1].id;
    expect(d.linkRefusal('seg', id)).toBeNull();
    d.link(id, 'seg');
    expect(d.doc.topo.links?.some((l) => l.from === 'seg' && l.to === id)).toBe(true);
    expect(d.linkRefusal(id, 'watch')).toBe(LINK_WHY.taken);
    expect(d.linkRefusal(id, 'seg')).toBe(LINK_WHY.taken);
    expect(d.linkRefusal(id, 'hub')).toBe(LINK_WHY.loop);
  });

  test('the sample checks clean; placing, undo and redo walk the history', () => {
    const d = new Define();
    expect(d.problems).toEqual([]);
    d.placeSample();
    expect([d.errors, d.config.nodes.length, d.config.links.length]).toEqual([0, 3, 2]);

    d.undo();
    expect(d.doc.topo.nodes).toEqual([]);
    d.redo();
    expect(d.doc.topo.nodes.length).toBe(3);
    expect(d.canRedo).toBe(false);
  });

  test('a placed node that is not linked is a problem whose 直す selects it; linking clears it', () => {
    const d = new Define();
    d.place('waveshare-rp2040-touch-lcd-1.28', 'Waveshare RP2040-Touch-LCD-1.28');
    d.place('nucleo-f401re', 'NUCLEO-F401RE');
    expect(d.selected).toBe('node:nucleo');
    const p = d.problems.find((x) => x.rule === 'V-05')!;
    expect([p.short, p.select, p.sentence]).toEqual(['つながっていないノード', 'node:nucleo', RULE_FIX['V-05']({ to: 'nucleo', board: 'nucleo-f401re', id: 'nucleo', many: false, max: 32 })]);
    d.selected = null;
    d.fix(p);
    expect(d.selected).toBe('node:nucleo');
    d.apply(connect(d.doc, 'nucleo'));
    expect(d.errors).toBe(0);
  });

  test('an address clash points at the link and its addr field', () => {
    const d = new Define();
    d.placeSample();
    d.apply(setLink(setLinkKind(d.doc, 1, 'i2c'), 1, 'addr', 0x42));
    const p = d.problems.find((x) => x.rule === 'V-18')!;
    expect([p.severity, p.select, p.field]).toEqual(['error', 'link:watch', 'addr']);
  });

  test('Delete removes the selected node (undoable); export and import round-trip; a broken file is C09', () => {
    const d = new Define();
    d.placeSample();
    d.selected = 'node:watch';
    d.removeSelected();
    expect(d.doc.topo.nodes.map((n) => n.id)).toEqual(['hub', 'seg']);
    d.undo();
    expect(d.doc.topo.nodes.length).toBe(3);

    const { name, text } = d.exportYaml();
    expect(name).toBe('sample.yaml');
    const e = new Define();
    expect(e.importYaml(text)).toBe(true);
    expect(e.config.yaml).toBe(d.config.yaml);
    expect(e.importYaml('nodes: [')).toBe(false);
    expect(e.unreadable).toBe(true);
  });

  test('運用 can be entered with a 構成 that has no problems, parts not updatable yet included (M69)', () => {
    const d = new Define();
    expect(d.operable).toBe('empty');
    d.placeSample();
    expect(d.operable).toBe('ok');
    d.apply(setLink(d.doc, 0, 'addr', 0x39));
    expect(d.operable).toBe('problems');
    d.undo();
    // A second 親機 over CAN: valid, a part updating skips (V-23, information).
    const r = placeMcu(d.doc, 'rp2040');
    d.apply(connect(setNode(r.doc, r.id, 'role', 'coordinator'), r.id, 'hub'));
    expect(d.errors).toBe(0);
    expect(d.operable).toBe('ok');
    expect(d.notYet).toBe(1);
    expect(problemChip(d.problems.find((p) => p.rule === 'V-23')!)).toEqual({ mark: '◇', kind: 'unsupported' });
  });

  test('"最初からやり直す" empties the 構成 and its history; load opens a 構成 without undo past it', () => {
    const d = new Define();
    d.placeSample();
    d.reset();
    expect([d.doc.topo.nodes.length, d.canUndo]).toEqual([0, false]);
  });

  test('a node chosen by MCU reads 「自作 · MCU 名」, and its missing board is a caution (▲), not an error', () => {
    const d = new Define();
    d.placeSample();
    const r = placeMcu(d.doc, 'rp2040');
    d.apply(connect(r.doc, r.id));
    const n = d.config.nodes.find((x) => x.id === r.id)!;
    expect([n.product, n.mcu]).toEqual(['ボード定義なし', 'RP2040']);
    expect(problemChip(d.problems.find((p) => p.rule === 'B-01')!)).toEqual({ mark: '▲', kind: 'warning' });
  });

  test('a coordinator whose MCU cannot be one is V-25, with its own name and fix sentence', () => {
    const d = new Define();
    const espHub = SAMPLE_YAML.replace(
      '    mcu: rp2040\n    board: waveshare-rp2040-touch-lcd-1.28\n    role: coordinator\n    port: self-boot\n    flash_map: rp2040-4m',
      '    mcu: esp32s3\n    role: coordinator\n    port: vendor-stage2',
    );
    expect(d.importYaml(espHub)).toBe(true);
    const p = d.problems.find((x) => x.rule === 'V-25')!;
    expect([p.short, p.sentence]).toEqual(['親機にできないマイコン', 'hubの代わりに、親機にできるマイコンのノードを親機にしてください。']);
  });

  test('by MCU, only an MCU whose definition has [coordinator] becomes the 親機', () => {
    expect(placeMcu(emptyDoc(), 'esp32s3').doc.topo.nodes[0].role).toBe('node');
    expect(placeMcu(emptyDoc(), 'rp2040').doc.topo.nodes[0].role).toBe('coordinator');
  });

  test('a CAN link bundle says its kind, not 「0 本」', () => {
    expect(LINK.bundle('CAN', 0, 0)).toBe('CAN');
    expect(LINK.bundle('I2C', 5, 3)).toBe('I2C · 5 本（うち共有 3）');
  });

  test('an empty file or one that is not a 構成 mapping is C09, not a crash', () => {
    const d = new Define();
    for (const text of ['', '42', '[a, b]', 'name: x']) {
      expect(d.importYaml(text), JSON.stringify(text)).toBe(false);
      expect(d.unreadable).toBe(true);
    }
  });
});
