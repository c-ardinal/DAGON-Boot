import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { topo_parse } from '../wasm/pkg/dagon_wasm.js';
import { AGENT } from './copy.ts';
import { fromYaml, toYaml, withDefaults, type Topo } from './define/model.ts';
import { SAMPLE_YAML } from './sample.ts';

// Each test reads the other side of a contract from its source, so there is no third list to keep.
const text = (rel: string): string => new TextDecoder().decode(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
const web = (rel: string): string => text(`web/src/${rel}`);

describe('every agent error code the server defines has a Console text', () => {
  const codes = [...text('crates/dagon-cli/src/agents/mod.rs').matchAll(/=\s*"(AGENT_[A-Z_]+)"/g)].map((m) => m[1]);

  test('the server lists its codes', () => {
    expect(codes.length).toBeGreaterThanOrEqual(8);
  });

  test.each(codes)('%s', (code) => {
    const e = (AGENT.errors as Record<string, { text: string } | undefined>)[code];
    expect(e?.text, `${code} has no text in copy.ts`).toBeTruthy();
  });
});

describe('every API path the Console calls is in the server route table', () => {
  const routes = [...text('crates/dagon-cli/src/serve/api.rs').matchAll(/\("(GET|POST|PUT|DELETE)",\s*\[([^\]]*)\]\)/g)].map((m) => ({
    method: m[1],
    // A quoted segment is literal, a bare name is a parameter.
    segs: m[2].split(',').map((s) => s.trim()).map((s) => (s.startsWith('"') ? s.slice(1, -1) : null)),
  }));

  const sources = ['app/agent.svelte.ts', 'app/define/cli.svelte.ts'];
  const calls: { file: string; method: string; path: string }[] = [];
  for (const file of sources) {
    const src = web(file);
    // const path = `agent/key/...`; used later as api('PUT', path)
    const vars = new Map([...src.matchAll(/const (\w+) = `([^`]*)`/g)].map((m) => [m[1], m[2]]));
    for (const m of src.matchAll(/\b(?:api|call|json)(?:<[^>(]*>)?\(\s*'(GET|POST|PUT|DELETE)'\s*,\s*(?:(['`])([^'`]*)\2|(\w+))/g)) {
      const path = m[3] ?? vars.get(m[4]);
      if (path !== undefined) calls.push({ file, method: m[1], path });
    }
    for (const m of src.matchAll(/this\.start\(\s*'\w+'\s*,\s*'([^']+)'|projectCall\(\s*'([^']+)'/g)) calls.push({ file, method: 'POST', path: m[1] ?? m[2] });
  }

  const matches = (c: { method: string; path: string }): boolean => {
    const parts = c.path.split('?')[0].split('/');
    return routes.some((r) => r.method === c.method && r.segs.length === parts.length && r.segs.every((s, i) => (s === null ? true : s === parts[i])) && parts.every((p, i) => !p.includes('${') || r.segs[i] === null));
  };

  test('the table and the calls are both found', () => {
    expect(routes.length).toBeGreaterThan(20);
    expect(calls.length).toBeGreaterThan(20);
  });

  test.each(calls.map((c) => [`${c.method} ${c.path}`, c] as const))('%s', (_n, c) => {
    expect(matches(c), `${c.file} calls ${c.method} ${c.path}, which serve/api.rs does not route`).toBe(true);
  });
});

const FULL = `dagon: 2
name: x
nodes:
  - id: hub
    mcu: rp2040
    board: some-board
    role: coordinator
    port: usb0
    flash_map: map
    max_attempts: 3
    listen_window_ms: 500
    tag: h
    uplink: { kind: ble, host: browser }
  - { id: n1, mcu: stm32, role: node, port: p, tag: t }
buses:
  - { id: i2c-main, kind: i2c, controller: I2C0, sda: 4, scl: 5, speed_hz: 100000, pullups: external }
links:
  - kind: i2c
    bus: i2c-main
    ends:
      - { node: hub, port: a }
      - { node: n1, port: b, addr: 0x42, node_pins: { sda: PB4, scl: PA8 }, nrst: 3, boot: 2 }
    control: { kind: full, rom_entry: i2c }
  - kind: uart
    iface: pio
    baud: 115200
    ends:
      - { node: hub, port: a, pins: { tx: 1, rx: 2 } }
      - { node: n1, port: b }
    control: { kind: cooperative }
recovery:
  n1: [retransfer, rom]
update:
  order: [n1, hub]
constraints:
  - lockstep: [hub, n1]
  - requires: { node: n1, needs: hub, min: 1.0.0 }
  - min_bootcore: { node: n1, min: 0.1.0 }
`;

describe('the YAML the Console writes reads back as the same topology', () => {
  test.each([
    ['a document with every field and each constraint kind', FULL],
    ['the sample', SAMPLE_YAML],
  ])('%s', (_n, yaml) => {
    const read = withDefaults(topo_parse(yaml) as Topo);
    const back = topo_parse(toYaml(fromYaml(yaml).topo)) as Topo;
    expect(withDefaults(back)).toEqual(read);
  });

  test('the full document has every constraint kind to lose', () => {
    expect(Object.keys((topo_parse(FULL) as Topo).constraints!.reduce((a, c) => ({ ...a, ...c }), {})).sort()).toEqual(['lockstep', 'min_bootcore', 'requires']);
  });
});
