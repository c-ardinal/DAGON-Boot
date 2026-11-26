import { describe, expect, test } from 'vitest';
import { RULE_FIX } from '../copy.ts';
import { canonical, Define } from './define.svelte.ts';
import { connect, emptyDoc, type Doc, fromYaml, LinkKind, linkView, placeMcu, setLink, setLinkKind, setNode } from './model.ts';
import { setName } from './model.ts';
import { SAMPLE_YAML } from '../sample.ts';
import { problemChip } from './problems.ts';
import { LINK, LINK_WHY } from '../copy.ts';

describe('define session', () => {
  test('a part dropped under a card is placed and linked under it in one edit; elsewhere it stands alone', () => {
    const d = new Define();
    d.placeAt({ board: 'waveshare-rp2040-touch-lcd-1.28' }, 'Waveshare RP2040-Touch-LCD-1.28', null);
    d.placeAt({ board: 'nucleo-f401re' }, 'NUCLEO-F401RE', 'hub');
    expect(d.doc.topo.links?.map((l) => linkView(d.doc.topo, l)).map((l) => [l.from, l.to])).toEqual([['hub', 'nucleo']]);
    d.undo();
    expect(d.doc.topo.nodes.map((n) => n.id)).toEqual(['hub']);
    d.placeAt({ mcu: 'stm32f401re' }, 'ボード', null);
    expect(d.doc.topo.links ?? []).toEqual([]);
  });

  test('deleting asks first: nothing goes until the confirmation says so, and then it is one undoable edit', () => {
    const d = new Define();
    d.load(fromYaml(SAMPLE_YAML));
    const nodes = d.doc.topo.nodes.length;
    d.selected = 'node:seg';
    d.requestRemove();
    expect(d.askRemove).toBe('node:seg');
    expect(d.doc.topo.nodes.length).toBe(nodes);
    d.answerRemove(false);
    expect([d.askRemove, d.doc.topo.nodes.length, d.canUndo]).toEqual([null, nodes, false]);
    d.requestRemove('link:watch');
    d.answerRemove(true);
    expect(d.doc.topo.links?.some((l) => l.ends.some((e) => e.node === 'watch'))).toBe(false);
    expect(d.doc.topo.nodes.length).toBe(nodes);
    d.undo();
    expect(d.doc.topo.links?.some((l) => l.ends.some((e) => e.node === 'watch'))).toBe(true);
    // A stopped edit asks nothing.
    d.guard = () => true;
    d.requestRemove('node:seg');
    expect(d.askRemove).toBe(null);
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
    // Enter / double-click in the parts window, the table's 配置, a drop, a port drag, Delete, import, undo.
    d.place('nucleo-f401re', 'NUCLEO-F401RE');
    d.placeMcu('stm32f401re', 'ボード');
    d.placeAt({ board: 'nucleo-f401re' }, 'NUCLEO-F401RE', 'hub');
    d.link('seg', 'watch');
    d.selected = 'node:seg';
    d.removeSelected();
    expect(d.importRelease({ ...d.doc.topo, nodes: d.doc.topo.nodes.slice(0, 1) })).toBe(false);
    expect(d.importYaml(SAMPLE_YAML)).toBe(false);
    d.undo();
    expect(JSON.stringify(d.doc)).toBe(before);
    // Eight edits asked: each path above is one, the port drag between two linked nodes (a ring) included.
    expect(asked).toBe(8);
  });

  test('a port drag links a node under another, and a node already linked to further nodes (a star, a ring); only a second link of one pair is refused', () => {
    const d = new Define();
    d.placeSample();
    expect(d.linkRefusal('hub', 'seg')).toBe(LINK_WHY.pair);
    expect(d.linkRefusal('seg', 'hub')).toBe(LINK_WHY.pair);
    d.placeAt({ mcu: 'stm32f401re' }, 'ボード', null);
    const id = d.doc.topo.nodes[d.doc.topo.nodes.length - 1].id;
    expect(d.linkRefusal('seg', id)).toBeNull();
    d.link(id, 'seg');
    const views = () => (d.doc.topo.links ?? []).map((l) => linkView(d.doc.topo, l));
    expect(views().some((l) => l.from === 'seg' && l.to === id)).toBe(true);
    // A ring: the two nodes under the coordinator, joined to each other.
    expect(d.linkRefusal('seg', 'watch')).toBeNull();
    d.link('watch', 'seg');
    expect(views().some((l) => l.from === 'seg' && l.to === 'watch')).toBe(true);
    expect(d.linkRefusal('watch', 'seg')).toBe(LINK_WHY.pair);
    // Each link has its own key: the link a node stands under, and the further one by both its nodes.
    expect(d.config.links.map((l) => l.key)).toEqual(['link:seg', 'link:watch', `link:${id}`, 'link:watch~seg']);
    // The ring's link is selected and deleted by itself; the link watch stands under stays.
    d.removeSelected('link:watch~seg');
    expect(d.config.links.map((l) => l.key)).toEqual(['link:seg', 'link:watch', `link:${id}`]);
    // A link drawn from a node to the coordinator is the coordinator's link to that node, and the node then stands under it.
    expect(d.linkRefusal(id, 'hub')).toBeNull();
    d.link('hub', id);
    expect(d.config.links.map((l) => [l.key, l.from])).toEqual([['link:seg', 'hub'], ['link:watch', 'hub'], [`link:${id}~seg`, 'seg'], [`link:${id}`, 'hub']]);
    // Updating reaches every node over its link from the coordinator.
    expect(d.config.reach).toEqual(['hub', 'seg', 'watch', id]);
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

  test('a placed node that is not linked is a problem whose 修正 selects it; linking clears it', () => {
    const d = new Define();
    d.place('waveshare-rp2040-touch-lcd-1.28', 'Waveshare RP2040-Touch-LCD-1.28');
    d.place('nucleo-f401re', 'NUCLEO-F401RE');
    expect(d.selected).toBe('node:nucleo');
    const p = d.problems.find((x) => x.rule === 'V-05')!;
    expect([p.short, p.select, p.sentence]).toEqual(['未接続のノード', 'node:nucleo', RULE_FIX['V-05']({ to: 'nucleo', board: 'nucleo-f401re', id: 'nucleo', many: false, max: 32 })]);
    d.selected = null;
    d.fix(p);
    expect(d.selected).toBe('node:nucleo');
    d.apply(connect(d.doc, 'nucleo'));
    expect(d.errors).toBe(0);
  });

  test('an address clash points at the link and its addr field', () => {
    const d = new Define();
    d.placeSample();
    d.apply(setLink(setLinkKind(d.doc, 1, LinkKind.I2c), 1, 'addr', 0x42));
    const p = d.problems.find((x) => x.rule === 'V-18')!;
    expect([p.severity, p.select, p.field]).toEqual(['error', 'link:watch', 'addr']);
  });

  test('Delete removes the selected node (undoable); export and import round-trip; a broken file is refused with its notice', () => {
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

  test('運用 can be entered with a 構成 that has no problems, parts updating skips included', () => {
    const d = new Define();
    expect(d.operable).toBe('empty');
    d.placeSample();
    expect(d.operable).toBe('ok');
    d.apply(setLink(d.doc, 0, 'addr', 0x39));
    expect(d.operable).toBe('problems');
    d.undo();
    // A second coordinator over CAN: valid, a part updating skips (V-23, information).
    const r = placeMcu(d.doc, 'rp2040');
    d.apply(connect(setNode(r.doc, r.id, 'role', 'coordinator'), r.id, 'hub'));
    expect(d.errors).toBe(0);
    expect(d.operable).toBe('ok');
    expect(d.skipped).toBe(1);
    expect(problemChip(d.problems.find((p) => p.rule === 'V-23')!)).toEqual({ mark: '◇', kind: 'unsupported' });
  });

  test('"リスタート" empties the 構成 and its history; load opens a 構成 without undo past it', () => {
    const d = new Define();
    d.placeSample();
    d.reset();
    expect([d.doc.topo.nodes.length, d.canUndo]).toEqual([0, false]);
  });

  test('a node chosen by MCU reads the name of the MCU and raises no board caution', () => {
    const d = new Define();
    d.placeSample();
    const r = placeMcu(d.doc, 'rp2040');
    d.apply(connect(r.doc, r.id));
    const n = d.config.nodes.find((x) => x.id === r.id)!;
    expect([n.product, n.mcu]).toEqual(['ボード定義なし', 'RP2040']);
    expect(d.problems.some((p) => p.rule === 'B-01' || p.rule === 'B-02')).toBe(false);
  });

  test('a saved session that still holds page-only board data loads and ignores it', () => {
    const fresh = placeMcu(emptyDoc(), 'rp2040').doc;
    const d = new Define();
    d.load({ ...fresh, boards: { hub: { flashBytes: 1 << 20 } } } as Doc);
    expect(Object.keys(d.doc)).not.toContain('boards');
    expect(d.problems.some((p) => p.rule === 'B-01' || p.rule === 'B-02')).toBe(false);
    expect(canonical(d.doc)).toBe(canonical(fresh));
  });

  test('a coordinator whose MCU cannot be one is V-25, with its own name and fix sentence', () => {
    const d = new Define();
    const espHub = SAMPLE_YAML.replace(
      '    mcu: rp2040\n    board: waveshare-rp2040-touch-lcd-1.28\n    role: coordinator\n    port: self-boot\n    flash_map: rp2040-4m',
      '    mcu: esp32s3\n    role: coordinator\n    port: vendor-stage2',
    );
    expect(d.importYaml(espHub)).toBe(true);
    const p = d.problems.find((x) => x.rule === 'V-25')!;
    expect([p.short, p.sentence]).toEqual(['コーディネータに不適合のマイコン', 'hubの代替として、コーディネータに適合するマイコンのノードをコーディネータにしてください。']);
  });

  test('by MCU, only an MCU whose definition has [coordinator] becomes the coordinator', () => {
    expect(placeMcu(emptyDoc(), 'esp32s3').doc.topo.nodes[0].role).toBe('node');
    expect(placeMcu(emptyDoc(), 'rp2040').doc.topo.nodes[0].role).toBe('coordinator');
  });

  test('a CAN link bundle says its kind, not 「0本」', () => {
    expect(LINK.bundle('CAN', 0, 0)).toBe('CAN');
    expect(LINK.bundle('I2C', 5, 3)).toBe('I2C · 5本（うち共有3）');
  });

  test('an empty file or one that is not a 構成 mapping is refused with its notice, not a crash', () => {
    const d = new Define();
    for (const text of ['', '42', '[a, b]', 'name: x']) {
      expect(d.importYaml(text), JSON.stringify(text)).toBe(false);
      expect(d.unreadable).toBe(true);
    }
  });
});

describe('names read after the project opened', () => {
  test('go under the ones here, are no edit, and undoing does not drop them', () => {
    const d = new Define();
    d.load(fromYaml(SAMPLE_YAML));
    const edits = d.revision;
    d.apply(setName(d.doc, 'seg', 'A'));
    const before = d.revision;
    d.adoptNames({ seg: 'old', hub: 'saved' });
    expect(d.doc.names).toEqual({ seg: 'A', hub: 'saved' });
    expect(d.revision).toBe(before);
    expect(before).toBeGreaterThan(edits);
    d.undo();
    expect(d.doc.names).toEqual({ seg: 'old', hub: 'saved' });
    d.redo();
    expect(d.doc.names).toEqual({ seg: 'A', hub: 'saved' });
  });
});
