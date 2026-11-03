import { describe, expect, test } from 'vitest';
import { topo_compile } from '../../wasm/dagon.ts';
import { DESK_DEMO_YAML } from '../sample.ts';
import { keepComments } from './comments.ts';
import { Define } from './define.svelte.ts';
import { fromYaml, setLink, toYaml } from './model.ts';

describe('hand-written YAML comments across a save (C-383)', () => {
  test('comments above a line and at its end come back on the same line, at the new indentation', () => {
    const source = '# head\nnodes:\n  # the hub\n  - id: hub  # main\n    mcu: rp2040\n';
    const generated = 'nodes:\n    - id: hub\n      mcu: rp2040\n';
    expect(keepComments(source, generated)).toBe('# head\nnodes:\n    # the hub\n    - id: hub  # main\n      mcu: rp2040\n');
  });

  test('a line that changed takes its comment with it; the others keep theirs', () => {
    const source = 'a: 1  # one\n# about b\nb: 2\nc: 3  # three\n';
    expect(keepComments(source, 'a: 1\nb: 9\nc: 3\n')).toBe('a: 1  # one\nb: 9\nc: 3  # three\n');
  });

  test('repeated lines keep their own comments, a # inside quotes is not a comment, comments after the last line stay last', () => {
    const source = 'x:\n  n: 3  # first\ny:\n  n: 3  # second\nz: "a #b"\n# end\n';
    expect(keepComments(source, 'x:\n  n: 3\ny:\n  n: 3\nz: "a #b"\n')).toBe(source);
  });

  test('without comments the generated text is unchanged', () => {
    expect(keepComments('a: 1\n', 'a: 2\n')).toBe('a: 2\n');
  });

  const commented = `# Desk demo, hand-written note.\n${DESK_DEMO_YAML.replace('    addr: 0x42', '    # the 7-segment board\n    addr: 0x42').replace('  order: [seg, watch, hub]', '  order: [seg, watch, hub]  # hub last')}`;

  test('desk-demo opened and saved untouched: the text as written, comment and all', () => {
    const d = new Define();
    d.load(fromYaml(DESK_DEMO_YAML), DESK_DEMO_YAML);
    expect(d.exportYaml().text).toBe(DESK_DEMO_YAML);
    expect(DESK_DEMO_YAML).toMatch(/^# Desk demo/);
  });

  test('desk-demo edited and saved: the comments of the untouched lines survive and the compiled 構成 is the same as without them', () => {
    const d = new Define();
    d.load(fromYaml(commented), commented);
    d.apply(setLink(d.doc, 1, 'baud', 460800));
    const text = d.exportYaml().text;
    expect(text).toMatch(/^# Desk demo, hand-written note\.\n# Desk demo: RP2040 hub/);
    expect(text).toMatch(/ {4}# the 7-segment board\n {4}addr: 0x42/);
    expect(text).toContain('order: [seg, watch, hub]  # hub last');
    expect(text).toContain('baud: 460800');
    expect(topo_compile(text)).toEqual(topo_compile(toYaml(d.doc.topo)));
    expect(fromYaml(text).topo).toEqual(d.doc.topo);
  });

  test('the comments of the lines that changed go with them', () => {
    const d = new Define();
    d.load(fromYaml(commented), commented);
    d.apply(setLink(d.doc, 0, 'addr', 0x43));
    const text = d.exportYaml().text;
    expect(text).not.toContain('# the 7-segment board');
    expect(text).toContain('addr: 0x43');
  });

  test('desk-demo saved untouched compiles exactly as before', () => {
    const d = new Define();
    d.load(fromYaml(DESK_DEMO_YAML), DESK_DEMO_YAML);
    expect(topo_compile(d.exportYaml().text)).toEqual(topo_compile(DESK_DEMO_YAML));
  });
});
