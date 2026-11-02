import { describe, expect, test } from 'vitest';
import { RULE_FIX } from '../copy.ts';
import { Define } from './define.svelte.ts';
import { connect, setLink, setLinkKind } from './model.ts';

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
});
