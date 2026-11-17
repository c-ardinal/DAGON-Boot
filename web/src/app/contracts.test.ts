import { readdirSync, readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { topo_parse } from '../wasm/pkg/dagon_wasm.js';
import { ROUTES, request, requestFile } from '../request.ts';
import { AGENT } from './copy.ts';
import type { Cli } from './define/cli.svelte.ts';
import { fromYaml, toYaml, withDefaults, type Topo } from './define/model.ts';
import { SAMPLE_YAML } from './sample.ts';

// Each test reads the other side of a contract from its source, so there is no third list to keep.
const text = (rel: string): string => new TextDecoder().decode(readFileSync(new URL(`../../../${rel}`, import.meta.url)));

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

interface Source {
  /** Path under web/src. */
  name: string;
  text: string;
}

/**
 * The Console's own sources: no tests, and not wasm/pkg, the WebAssembly loader wasm-bindgen
 * writes at build time (it loads the page's own .wasm and is not in the repository).
 */
function consoleSources(): Source[] {
  const src = new URL('../', import.meta.url);
  const files = (dir: URL): URL[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? files(new URL(`${e.name}/`, dir)) : [new URL(e.name, dir)]));
  return files(src)
    .map((f) => ({ name: f.pathname.slice(src.pathname.length), url: f }))
    .filter((f) => /\.(ts|js|svelte)$/.test(f.name) && !/\.test\.ts$|^wasm\/pkg\//.test(f.name))
    .map((f) => ({ name: f.name, text: new TextDecoder().decode(readFileSync(f.url)) }));
}

const REQUEST = 'request.ts';
const count = (text: string, part: string): number => text.split(part).length - 1;

/** The files that send a request themselves: any with `fetch(` other than request.ts, and request.ts unless it has exactly one. */
const strayFetches = (files: Source[]): string[] => files.filter((f) => count(f.text, 'fetch(') !== (f.name === REQUEST ? 1 : 0)).map((f) => f.name);

/** The entries of `table` that are not among the server's routes. */
const unrouted = (table: readonly string[], server: ReadonlySet<string>): string[] => table.filter((r) => !server.has(r));

describe('every request the Console sends goes through request(), by a route the server has', () => {
  // serve's route table in the form ROUTES is written in: a quoted segment is literal, a bare name is `:name`.
  const server = new Set(
    [...text('crates/dagon-cli/src/serve/api.rs').matchAll(/\("(GET|POST|PUT|DELETE)",\s*\[([^\]]*)\]\)/g)].map(
      (m) =>
        `${m[1]} /v1/${m[2]
          .split(',')
          .map((s) => s.trim())
          .map((s) => (s.startsWith('"') ? s.slice(1, -1) : `:${s}`))
          .join('/')}`,
    ),
  );
  const sources = consoleSources();

  test('the server table and the sources are both found', () => {
    expect(server.size).toBeGreaterThan(20);
    expect(server).toContain('GET /v1/release/:id');
    expect(sources.map((f) => f.name)).toEqual(expect.arrayContaining([REQUEST, 'app/define/cli.svelte.ts', 'App.svelte']));
  });

  test('fetch( is written once, in request.ts', () => {
    expect(strayFetches(sources)).toEqual([]);
  });

  test('every route of the table is one the server has', () => {
    expect(unrouted(ROUTES, server)).toEqual([]);
  });

  test('a fetch( anywhere else is found', () => {
    const one = { name: REQUEST, text: 'fetch(url, init)' };
    expect(strayFetches([one])).toEqual([]);
    expect(strayFetches([one, { name: 'app/stray.ts', text: "await fetch('/v1/hello')" }])).toEqual(['app/stray.ts']);
    expect(strayFetches([one, { name: 'app/Stray.svelte', text: 'this.d.fetch(url)' }])).toEqual(['app/Stray.svelte']);
    expect(strayFetches([{ name: REQUEST, text: 'fetch(a); fetch(b)' }])).toEqual([REQUEST]);
    expect(strayFetches([{ name: REQUEST, text: '' }])).toEqual([REQUEST]);
  });

  test.each(['GET /v1/nope', 'DELETE /v1/hello', 'GET /v1/release/:node', 'GET /v1/release', 'GET /v1/jobs/:id/answer', 'GET hello'])('%s, which the server lacks, is found', (route) => {
    expect(unrouted(['GET /v1/hello', route], server)).toEqual([route]);
  });

  test('what is not a route of the table does not compile (svelte-check fails on an unused @ts-expect-error)', () => {
    const built: string = ['GET', '/v1/hello'].join(' ');
    // @ts-expect-error a string put together at run time is no Route
    const fromText: Parameters<typeof request>[0] = built;
    // @ts-expect-error a literal outside the table is no Route
    const outside: Parameters<typeof request>[0] = 'GET /v1/nope';
    // @ts-expect-error nor is one through the Console's own call
    const called: Parameters<Cli['api']>[0] = built;
    // @ts-expect-error nor is an address wrapped in an object
    const wrapped: Parameters<typeof request>[0] = { file: '/v1/nope' };
    // @ts-expect-error an address that is not the import of a page file is no PageFile
    const address: Parameters<typeof requestFile>[0] = '/v1/nope';
    // @ts-expect-error nor is one put together at run time
    const builtAddress: Parameters<typeof requestFile>[0] = built;
    expect([fromText, outside, called, wrapped, address, builtAddress]).toHaveLength(6);
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
    const constraints = (topo_parse(FULL) as { constraints: Record<string, unknown>[] }).constraints;
    expect(constraints.flatMap((c) => Object.keys(c)).sort()).toEqual(['lockstep', 'min_bootcore', 'requires']);
  });
});
