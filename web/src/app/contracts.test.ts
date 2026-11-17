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

const METHODS = 'GET|POST|PUT|DELETE';
const isIdent = (s: string): boolean => /^\w+$/.test(s);

/** The text of `s` when it is exactly one string literal (single, double or backtick quotes), else null. */
function wholeLiteral(s: string): string | null {
  const t = s.trim();
  const lit = /^['"`]/.test(t) ? literalsOf(t)[0] : undefined;
  return lit !== undefined && lit.length + 2 === t.length ? lit : null;
}

function methodOf(s: string | undefined): string | null {
  const lit = wholeLiteral(s ?? '');
  return lit !== null && new RegExp(`^(${METHODS})$`).test(lit) ? lit : null;
}

/** The index of the first top-level `ch` of `t` (outside quotes and brackets), or -1. */
function topLevel(t: string, ch: string): number {
  let depth = 0;
  for (let i = 0; i < t.length; i++) {
    const c = t[i];
    if (c === "'" || c === '"' || c === '`') i += literalsOf(t.slice(i))[0].length + 1;
    else if ('([{'.includes(c)) depth++;
    else if (')]}'.includes(c)) depth--;
    else if (c === ch && depth === 0 && !(ch === '?' && (t[i + 1] === '.' || t[i + 1] === '?' || t[i - 1] === '?'))) return i;
  }
  return -1;
}

/** The top-level comma-separated parts of `src` from `from` up to the bracket that closes there, and that bracket's index (-1 when none). */
function partsUntilClose(src: string, from: number): { parts: string[]; end: number } {
  const parts: string[] = [];
  let depth = 0;
  for (let i = from; i < src.length; i++) {
    const c = src[i];
    if (c === "'" || c === '"' || c === '`') i += literalsOf(src.slice(i))[0].length + 1;
    else if ('([{'.includes(c)) depth++;
    else if (')]}'.includes(c) && depth-- === 0) return (parts.push(src.slice(from, i)), { parts, end: i });
    else if (c === ',' && depth === 0) (parts.push(src.slice(from, i)), (from = i + 1));
  }
  return { parts, end: -1 };
}

/** Whether `t` spreads anything (`...`) outside its string literals. */
function hasSpread(t: string): boolean {
  for (let i = 0; i < t.length; i++) {
    if (t[i] === "'" || t[i] === '"' || t[i] === '`') i += literalsOf(t.slice(i))[0].length + 1;
    else if (t.startsWith('...', i)) return true;
  }
  return false;
}

/** The properties of `s` when it is exactly one object literal of `key: value` and shorthand `key` entries, else null (a spread, a computed key, a method or anything else). */
function propsOf(s: string): Map<string, string> | null {
  const t = s.trim();
  if (!t.startsWith('{')) return null;
  const { parts, end } = partsUntilClose(t, 1);
  if (end !== t.length - 1) return null;
  const props = new Map<string, string>();
  for (const p of parts.map((x) => x.trim()).filter((x, i) => x !== '' || i < parts.length - 1)) {
    const m = /^(\w+)\s*(?::([\s\S]*))?$/.exec(p);
    if (!m || props.has(m[1])) return null;
    props.set(m[1], m[2] ?? m[1]);
  }
  return props;
}

/** Calls of api/call/json('METHOD', path, ..), start('kind', path, ..), projectCall(path, ..) and fetch('/v1/path', { method }), with where the name starts and their top-level arguments. */
function callsIn(src: string): { name: string; at: number; args: string[] }[] {
  const out: { name: string; at: number; args: string[] }[] = [];
  for (const m of src.matchAll(/\b(api|call|json|start|projectCall|fetch)(?:<[^>(]*>)?\(/g)) {
    const args = partsUntilClose(src, m.index! + m[0].length).parts;
    const first = args[0] ?? '';
    // A definition has `name: type` parameters, and a call with no argument (r.json()) carries no path.
    if (!/^\s*\w+\s*:/.test(first) && !(args.length === 1 && first.trim() === '')) out.push({ name: m[1], at: m.index!, args });
  }
  return out;
}

interface Read {
  calls: { file: string; method: string; path: string }[];
  unreadable: string[];
}

/** What `src` (the file `file`) sends to the API: method and path of each call, and every recognized call whose method or path could not be read. */
function readCalls(file: string, src: string): Read {
  const calls: Read['calls'] = [];
  const unreadable: string[] = [];
  // const path = `agent/key/...`; used later as api('PUT', path)
  const vars = new Map([...src.matchAll(/const (\w+) = (['"`])(.*?)\2\s*;/g)].map((m) => [m[1], m[3]]));
  // Parameters that carry a method or a path on to api()/call()/json(), start() or projectCall(): the callers of those functions are read instead.
  const wrappers = [...src.matchAll(/\b(?:api|call|json)(?:<[^>(]*>)?\(\s*(\w+)\s*:[^,]+,\s*(\w+)\s*:/g)];
  const forwardedMethods = new Set(wrappers.map((m) => m[1]));
  const forwarded = new Set([...wrappers.map((m) => m[2]), ...[...src.matchAll(/\bstart\(\s*\w+\s*:\s*[^,]+,\s*(\w+)\s*:|\bprojectCall\(\s*(\w+)\s*:/g)].map((m) => m[1] ?? m[2])]);

  /** Every path `expr` can be: a literal, a named literal, a forwarded parameter (none here), or each arm of a conditional; null when any is unreadable. */
  const pathsOf = (expr: string): string[] | null => {
    const t = expr.trim();
    const lit = wholeLiteral(t);
    if (lit !== null) return [lit];
    if (isIdent(t)) return vars.has(t) ? [vars.get(t)!] : forwarded.has(t) ? [] : null;
    const q = topLevel(t, '?');
    if (q < 0) return null;
    const rest = t.slice(q + 1);
    const colon = topLevel(rest, ':');
    if (colon < 0) return null;
    const arms = [pathsOf(rest.slice(0, colon)), pathsOf(rest.slice(colon + 1))];
    return arms.some((a) => a === null) ? null : arms.flatMap((a) => a!);
  };

  /** `(...a) => fetch(...a)`: hands every argument on unchanged, so it is no call of its own. */
  const isSpreadPassthrough = (call: { at: number; args: string[] }): boolean => {
    const rest = /^\s*\.\.\.(\w+)\s*$/.exec(call.args[0]);
    return call.args.length === 1 && rest !== null && new RegExp(`\\(\\s*\\.\\.\\.${rest[1]}\\s*\\)\\s*=>\\s*$`).test(src.slice(0, call.at));
  };
  /** fetch(`/v1/${path}`, { method, .. }) inside a wrapper whose method and path parameters are forwarded: its callers are read. */
  const isWrapperFetch = (args: string[]): boolean => {
    const path = /^`\/v1\/\$\{(\w+)\}`$/.exec(args[0].trim());
    const method = args.length === 2 && !hasSpread(args[1]) ? propsOf(args[1])?.get('method')?.trim() : undefined;
    return path !== null && forwarded.has(path[1]) && method !== undefined && forwardedMethods.has(method);
  };
  /** The method and paths of fetch(path) or fetch(path, { method: 'M', .. }); null for any other form. */
  const fetchOf = (args: string[]): { method: string; paths: string[] } | null => {
    if (args.length > 2 || args.some(hasSpread)) return null;
    const method = args[1] === undefined ? 'GET' : methodOf(propsOf(args[1])?.get('method'));
    const paths = pathsOf(args[0]);
    if (method === null || paths === null || !paths.length || paths.some((p) => !p.startsWith('/v1/') || p.includes('${'))) return null;
    return { method, paths: paths.map((p) => p.slice('/v1/'.length)) };
  };

  for (const call of callsIn(src)) {
    const shown = `${file}: ${call.name}(${call.args.join(', ')})`;
    if (call.name === 'fetch') {
      if (isSpreadPassthrough(call) || isWrapperFetch(call.args)) continue;
      const read = fetchOf(call.args);
      if (read === null) unreadable.push(shown);
      else for (const path of read.paths) calls.push({ file, method: read.method, path });
      continue;
    }
    const helper = call.name === 'start' || call.name === 'projectCall';
    const method = helper ? 'POST' : (methodOf(call.args[0]) ?? (forwardedMethods.has((call.args[0] ?? '').trim()) ? 'forwarded' : null));
    const paths = pathsOf(call.args[call.name === 'start' ? 1 : helper ? 0 : 1] ?? '');
    if (method === null || paths === null) unreadable.push(shown);
    else if (method !== 'forwarded') for (const p of paths) calls.push({ file, method, path: p });
  }
  return { calls, unreadable };
}

describe('the reader of the Console calls', () => {
  const read = (src: string): Read => readCalls('x.ts', src);
  const found = (src: string): string[] => read(src).calls.map((c) => `${c.method} ${c.path}`);

  test.each([
    ['api', "this.api('GET', 'a')", ['GET a']],
    ['call', "this.call('POST', 'b')", ['POST b']],
    ['json', "this.json<T>('PUT', 'c', body)", ['PUT c']],
    ['start', "this.start('gen', 'd', { nodes }, 'x')", ['POST d']],
    ['projectCall', "this.projectCall('e', { path }, false)", ['POST e']],
    ['fetch', "this.d.fetch('/v1/f', { method: 'DELETE', headers })", ['DELETE f']],
    ['fetch without options', "this.d.fetch('/v1/g')", ['GET g']],
    ['both arms of a conditional', "this.json('GET', p === undefined ? 'h' : `h?path=${encodeURIComponent(p)}`)", ['GET h', 'GET h?path=${encodeURIComponent(p)}']],
    ['double quotes', 'this.call("GET", "i")', ['GET i']],
    ['backticks without ${}', 'this.call(`GET`, `j`)', ['GET j']],
    ['backticks with ${}', 'this.call("GET", `k/${id}`)', ['GET k/${id}']],
    ['a named template', 'const p = `l/${a}`;\nthis.api("PUT", p)', ['PUT l/${a}']],
    ['a named double-quoted string', 'const p = "m";\nthis.api("PUT", p)', ['PUT m']],
    ['a conditional fetch', "this.d.fetch(x ? '/v1/n' : \"/v1/o\", { method: `POST` })", ['POST n', 'POST o']],
  ])('%s is read', (_n, src, want) => {
    const r = read(src);
    expect(r.unreadable).toEqual([]);
    expect(found(src)).toEqual(want);
  });

  test('a wrapper passing its method and path on is not a call; its callers are', () => {
    const src = "api(method: string, path: string) { return this.call(method, path); }\nthis.api('GET', 'q')";
    expect(read(src)).toMatchObject({ unreadable: [], calls: [{ method: 'GET', path: 'q' }] });
  });

  test.each([
    ['a fetch wrapper forwarding its method and path', "call(method: string, path: string) { return this.d.fetch(`/v1/${path}`, { method, headers }); }\nthis.call('GET', 'q')", ['GET q']],
    ['a fetch handing all its arguments on', 'const f = (...a) => fetch(...a);', []],
  ])('%s is not a call; its callers are', (_n, src, want) => {
    expect(read(src).unreadable).toEqual([]);
    expect(found(src)).toEqual(want);
  });

  test.each([
    ['a method that is a call', "this.call(verb(), 'a')"],
    ['a method that is not a literal', "this.call(verb, 'a')"],
    ['a method that is not an HTTP one', 'this.call("FETCH", "a")'],
    ['a path built by a function', "this.call('GET', pathOf(1))"],
    ['a path that is a variable of unknown value', "this.call('GET', target)"],
    ['a path concatenated', "this.call('GET', 'a' + id)"],
    ['one conditional arm that is not a literal', "this.call('GET', c ? 'a' : other)"],
    ['a call with no path', "this.call('GET')"],
    ['a fetch of a variable', 'this.d.fetch(url, { method: "GET" })'],
    ['a fetch with a method that is not a literal', "this.d.fetch('/v1/a', { method: m })"],
    ['a fetch outside /v1/', "this.d.fetch('/other', { method: 'GET' })"],
    ['a fetch of spread arguments', 'this.d.fetch(...args)'],
    ['a fetch spreading arguments it was not handed', 'const f = (...a) => fetch(...b);'],
    ['a fetch of a template path', "this.d.fetch(`/v1/${path}`, { method: 'GET' })"],
    ['a fetch of a forwarded template path with a fixed method', "call(method: string, path: string) { return this.d.fetch(`/v1/${path}`, { method: 'GET' }); }"],
    ['a fetch with a method that is not a literal among other keys', "this.d.fetch('/v1/a', { headers: { a: 'b' }, method: m })"],
    ['a fetch of spread options', "this.d.fetch('/v1/a', { ...opts })"],
    ['a fetch of spread options and a method', "this.d.fetch('/v1/a', { ...opts, method: 'GET' })"],
    ['a fetch with a spread inside its options', "this.d.fetch('/v1/a', { headers: { ...h }, method: 'GET' })"],
    ['a fetch of options in a variable', "this.d.fetch('/v1/a', opts)"],
    ['a fetch with options and no method', "this.d.fetch('/v1/a', { headers })"],
    ['a fetch with the method only in a nested object', "this.d.fetch('/v1/a', { headers: { method: 'PUT' } })"],
    ['a fetch with a computed key', "this.d.fetch('/v1/a', { [k]: v, method: 'GET' })"],
    ['a fetch with a third argument', "this.d.fetch('/v1/a', { method: 'GET' }, x)"],
    ['a start with a path that is not a literal', "this.start('gen', where, {}, 'x')"],
    ['a projectCall with a path that is not a literal', 'this.projectCall(where, {}, false)'],
  ])('%s is unreadable, not skipped', (_n, src) => {
    expect(read(src).unreadable).toHaveLength(1);
  });
});

describe('every API path the Console calls is in the server route table', () => {
  const routes = [...text('crates/dagon-cli/src/serve/api.rs').matchAll(/\("(GET|POST|PUT|DELETE)",\s*\[([^\]]*)\]\)/g)].map((m) => ({
    method: m[1],
    // A quoted segment is literal, a bare name is a parameter.
    segs: m[2]
      .split(',')
      .map((s) => s.trim())
      .map((s) => (s.startsWith('"') ? s.slice(1, -1) : null)),
  }));

  const sources = ['app/agent.svelte.ts', 'app/define/cli.svelte.ts'];
  const calls: Read['calls'] = [];
  const unreadable: string[] = [];
  for (const file of sources) {
    const read = readCalls(file, web(file));
    calls.push(...read.calls);
    unreadable.push(...read.unreadable);
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

  test('no call is skipped because its method or path cannot be read', () => {
    expect(unreadable).toEqual([]);
  });

  test('no other source calls the API', () => {
    const files = (dir: URL): URL[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? files(new URL(`${e.name}/`, dir)) : [new URL(e.name, dir)]));
    const callers = files(new URL('../', import.meta.url))
      .filter((f) => /\.(ts|svelte)$/.test(f.pathname) && !/\.test\.ts$|agentMock|wasm\/pkg/.test(f.pathname) && !sources.some((s) => f.pathname.endsWith(`/${s}`)))
      .filter((f) => /fetch\(\s*[`'"]\/v1\/|\b(?:api|call|json)(?:<[^>(]*>)?\(\s*['"`](?:GET|POST|PUT|DELETE)['"`]/.test(new TextDecoder().decode(readFileSync(f))));
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
