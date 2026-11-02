import { describe, expect, test } from 'vitest';
import { RULE_FIX } from '../copy.ts';
import { Define } from './define.svelte.ts';
import { connect, placeMcu, setLink, setLinkKind, setNode } from './model.ts';
import { problemChip } from './problems.ts';
import { LINK } from '../copy.ts';

describe('define session', () => {
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
    expect([p.short, p.select, p.sentence]).toEqual(['つながっていないノード', 'node:nucleo', RULE_FIX['V-05']({ to: 'nucleo', board: 'nucleo-f401re', id: 'nucleo', many: false })]);
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
    expect(name).toBe('desk-demo.yaml');
    const e = new Define();
    expect(e.importYaml(text)).toBe(true);
    expect(e.config.yaml).toBe(d.config.yaml);
    expect(e.importYaml('nodes: [')).toBe(false);
    expect(e.unreadable).toBe(true);
  });

  test('運用 can be entered only with a 構成 that has no problems and is updatable (M68 / M69)', () => {
    const d = new Define();
    expect(d.operable).toBe('empty');
    d.placeSample();
    expect(d.operable).toBe('ok');
    d.apply(setLink(d.doc, 0, 'addr', 0x39));
    expect(d.operable).toBe('problems');
    d.undo();
    // A second 親機 over CAN: a shape updating does not reach yet (V-23 only).
    const r = placeMcu(d.doc, 'rp2040');
    d.apply(connect(setNode(r.doc, r.id, 'role', 'coordinator'), r.id, 'hub'));
    expect(new Set(d.problems.filter((p) => p.severity === 'error').map((p) => p.rule))).toEqual(new Set(['V-23']));
    expect(d.operable).toBe('unsupported');
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
    expect([n.product, n.mcu]).toEqual(['自作', 'RP2040']);
    expect(problemChip(d.problems.find((p) => p.rule === 'B-01')!)).toEqual({ mark: '▲', kind: 'warning' });
  });

  test('a CAN link bundle says its kind, not 「0 本」', () => {
    expect(LINK.bundle('CAN', 0, 0)).toBe('CAN');
    expect(LINK.bundle('I2C', 5, 3)).toBe('I2C · 5 本（うち共有 3）');
  });
});
