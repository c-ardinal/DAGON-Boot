import { readdirSync, readFileSync } from 'node:fs';
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

/** The string literals ('..', "..", `..`) written in `expr`, each branch of a conditional included. */
function literalsOf(expr: string): string[] {
  const out: string[] = [];
  for (let i = 0; i < expr.length; i++) {
    const q = expr[i];
    if (q !== "'" && q !== '"' && q !== '`') continue;
    let j = i + 1;
    for (let depth = 0; j < expr.length && (expr[j] !== q || depth > 0); j++) {
      if (expr[j] === '\\') j++;
      else if (q === '`' && expr.startsWith('${', j)) (depth++, j++);
      else if (q === '`' && expr[j] === '}' && depth > 0) depth--;
    }
    out.push(expr.slice(i + 1, j));
    i = j;
  }
  return out;
}

const unquote = (s: string | undefined): string | null => (s !== undefined && /^'(GET|POST|PUT|DELETE)'$/.test(s.trim()) ? s.trim().slice(1, -1) : null);

/** Calls of api/call/json('METHOD', path, ..), start('kind', path, ..), projectCall(path, ..) and fetch('/v1/path', { method }), with their top-level arguments. */
function callsIn(src: string): { name: string; args: string[] }[] {
  const out: { name: string; args: string[] }[] = [];
  for (const m of src.matchAll(/\b(api|call|json|start|projectCall|fetch)(?:<[^>(]*>)?\(/g)) {
    const args: string[] = [];
    let depth = 0;
    let from = m.index! + m[0].length;
    for (let i = from; i < src.length; i++) {
      const c = src[i];
      if (c === "'" || c === '"' || c === '`') {
        const lit = literalsOf(src.slice(i))[0];
        i += lit.length + 1;
      } else if ('([{'.includes(c)) depth++;
      else if (')]}'.includes(c) && depth-- === 0) {
        args.push(src.slice(from, i));
        break;
      } else if (c === ',' && depth === 0) (args.push(src.slice(from, i)), (from = i + 1));
    }
    // A definition or a type has `name: type` parameters; only calls are read.
    if (!/^\s*\w+\s*:/.test(args[0] ?? '') && !(m[1] === 'start' && !/^\s*'/.test(args[0] ?? ''))) out.push({ name: m[1], args });
  }
  return out;
}

describe('every API path the Console calls is in the server route table', () => {
  const routes = [...text('crates/dagon-cli/src/serve/api.rs').matchAll(/\("(GET|POST|PUT|DELETE)",\s*\[([^\]]*)\]\)/g)].map((m) => ({
    method: m[1],
    // A quoted segment is literal, a bare name is a parameter.
    segs: m[2].split(',').map((s) => s.trim()).map((s) => (s.startsWith('"') ? s.slice(1, -1) : null)),
  }));

  const sources = ['app/agent.svelte.ts', 'app/define/cli.svelte.ts'];
  const calls: { file: string; method: string; path: string }[] = [];
  const unreadable: string[] = [];
  for (const file of sources) {
    const src = web(file);
    // const path = `agent/key/...`; used later as api('PUT', path)
    const vars = new Map([...src.matchAll(/const (\w+) = `([^`]*)`/g)].map((m) => [m[1], m[2]]));
    // A parameter that carries a path on to api()/call()/json(): the callers of its function are read instead.
    const forwarded = new Set([...src.matchAll(/\bstart\(\s*\w+\s*:\s*[^,]+,\s*(\w+)\s*:|\bprojectCall\(\s*(\w+)\s*:/g)].map((m) => m[1] ?? m[2]));
    for (const call of callsIn(src)) {
      if (call.name === 'fetch') {
        // fetch('/v1/topology', { method: 'PUT', .. }); the wrapper's own `/v1/${path}` and a forwarded `...args` are not calls.
        const expr = call.args[0].trim();
        const method = /method:\s*'(GET|POST|PUT|DELETE)'/.exec(call.args[1] ?? '')?.[1] ?? (call.args[1] === undefined ? 'GET' : null);
        if (expr.startsWith('...') || expr === '`/v1/${path}`') continue;
        const lits = literalsOf(expr).filter((l) => l.startsWith('/v1/'));
        if (!lits.length || method === null) unreadable.push(`${file}: fetch(${call.args.join(', ')})`);
        else for (const l of lits) calls.push({ file, method, path: l.slice('/v1/'.length) });
        continue;
      }
      const exprs = call.name === 'start' || call.name === 'projectCall' ? [call.args[call.name === 'start' ? 1 : 0]] : [call.args[1]];
      const method = call.name === 'start' || call.name === 'projectCall' ? 'POST' : unquote(call.args[0]);
      if (method === null || exprs[0] === undefined) continue;
      const expr = exprs[0].trim();
      const lits = literalsOf(expr);
      const path = lits.length ? lits : /^\w+$/.test(expr) && vars.has(expr) ? [vars.get(expr)!] : /^\w+$/.test(expr) && forwarded.has(expr) ? [] : null;
      if (path === null) unreadable.push(`${file}: ${call.name}(${call.args.join(', ')})`);
      else for (const p of path) calls.push({ file, method, path: p });
    }
  }

  const matches = (c: { method: string; path: string }): boolean => {
    const parts = c.path.split('?')[0].split('/');
    return routes.some((r) => r.method === c.method && r.segs.length === parts.length && r.segs.every((s, i) => (s === null ? true : s === parts[i])) && parts.every((p, i) => !p.includes('${') || r.segs[i] === null));
  };

  test('the table and the calls are both found', () => {
    expect(routes.length).toBeGreaterThan(20);
    expect(calls.length).toBeGreaterThan(20);
    expect(calls.map((c) => `${c.method} ${c.path.split('?')[0]}`)).toContain('GET fs');
  });

  test('no call is skipped because its path cannot be read', () => {
    expect(unreadable).toEqual([]);
  });

  test('no other source calls the API', () => {
    const files = (dir: URL): URL[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? files(new URL(`${e.name}/`, dir)) : [new URL(e.name, dir)]));
    const callers = files(new URL('../', import.meta.url))
      .filter((f) => /\.(ts|svelte)$/.test(f.pathname) && !/\.test\.ts$|agentMock|wasm\/pkg/.test(f.pathname) && !sources.some((s) => f.pathname.endsWith(`/${s}`)))
      .filter((f) => /fetch\(\s*[`'"]\/v1\/|\b(?:api|call|json)(?:<[^>(]*>)?\(\s*'(?:GET|POST|PUT|DELETE)'/.test(new TextDecoder().decode(readFileSync(f))));
    expect(callers.map((f) => f.pathname.split('/src/')[1])).toEqual([]);
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
    const constraints = (topo_parse(FULL) as { constraints: Record<string, unknown>[] }).constraints;
    expect(constraints.flatMap((c) => Object.keys(c)).sort()).toEqual(['lockstep', 'min_bootcore', 'requires']);
  });
});
