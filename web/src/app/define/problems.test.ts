import { describe, expect, test } from 'vitest';
import { topo_check } from '../../wasm/dagon.ts';
import { SAMPLE_YAML } from '../sample.ts';
import { fromYaml, setBus, setLink, toYaml } from './model.ts';
import { problems } from './problems.ts';

const errors = (yaml: string) => (JSON.parse(topo_check(yaml)) as { errors: number }).errors;

/** A problem's sentence names values the check itself reads: following it makes the check pass. */
describe('problem sentences', () => {
  test('V-20 names the speeds every node on the bus defines, and one of them passes', () => {
    const doc = setBus(fromYaml(SAMPLE_YAML), 0, 'speed_hz', 200_000);
    const p = problems(doc).find((x) => x.rule === 'V-20')!;
    expect(p.sentence).toContain('100 kHz・400 kHz');
    expect(p.sentence).not.toContain('以下');
    expect(errors(toYaml(setBus(doc, 0, 'speed_hz', 100_000).topo))).toBe(0);
  });

  test('V-13 names the pairs the ROM loader listens on, and one of them passes', () => {
    const base = fromYaml(SAMPLE_YAML);
    const i = base.topo.links!.findIndex((l) => l.kind === 'i2c');
    const doc = setLink(setLink(base, i, 'node_pins.sda', 'PB9'), i, 'node_pins.scl', 'PB8');
    const p = problems(doc).find((x) => x.rule === 'V-13')!;
    expect(p.sentence).toContain('PB4・PA8、PB7・PB6、PB3・PB10');
    expect(p.sentence).not.toContain('PB9');
    expect(errors(toYaml(setLink(setLink(doc, i, 'node_pins.sda', 'PB4'), i, 'node_pins.scl', 'PA8').topo))).toBe(0);
  });

  test("V-07 names the pins of the pin's own node, not the 親機's by name, and one of them passes", () => {
    const base = fromYaml(SAMPLE_YAML);
    const i = base.topo.links!.findIndex((l) => l.kind === 'uart');
    const doc = setLink(base, i, 'pins.tx', 5);
    const p = problems(doc).find((x) => x.rule === 'V-07')!;
    expect(p.short).not.toContain('親機');
    expect(p.sentence).toContain('GPIO26');
    expect(errors(toYaml(setLink(doc, i, 'pins.tx', 26).topo))).toBe(0);
  });
});
