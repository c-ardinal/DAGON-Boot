import { describe, expect, test } from 'vitest';
import { Cli, CONSOLE_VERSION, SERVE_API, type CliDeps, parseLayout } from './cli.svelte.ts';
import pkg from '../../../package.json';
import { Define } from './define.svelte.ts';
import { SAVE } from '../copy.ts';

const PASS = '0123456789abcdef0123456789abcdef';
const HELLO = { cli: '0.1.0', api: 1, workspace: 'my-fleet', features: ['build', 'sign', 'release', 'gen'], key: true, key_id: '3f9a21c7aabbccdd' };

interface Call {
  url: string;
  method: string;
  auth: string;
  body: unknown;
}

function fake(opts: { status?: number; hash?: string } = {}) {
  const calls: Call[] = [];
  const store = new Map<string, string>();
  const sent: string[] = [];
  let sock: { onopen?: () => void; onmessage?: (m: { data: string }) => void; onclose?: () => void; send: (s: string) => void } | null = null;
  let replaced = '';
  let layout = '';
  const deps: CliDeps = {
    fetch: (async (url: string, init: RequestInit) => {
      calls.push({ url, method: init.method ?? 'GET', auth: (init.headers as Record<string, string>).Authorization, body: init.body });
      if (opts.status) return new Response('', { status: opts.status });
      if (url === '/v1/hello') return Response.json(HELLO);
      if (url.startsWith('/v1/release/')) return new Response(Uint8Array.of(1, 2, 3));
      if (url === '/v1/kept' && init.method === 'POST') return Response.json({ id: 'v1.1.0' });
      if (url === '/v1/kept') return Response.json({ kept: [{ id: 'v1.0.0', versions: { hub: '1.0.0' }, bytes: 3 }] });
      if (url.startsWith('/v1/kept/')) return new Response(Uint8Array.of(1, 2, 3));
      if (url === '/v1/layout') {
        if (init.method === 'PUT') {
          layout = String(init.body);
          return Response.json({ saved: true });
        }
        return layout ? new Response(layout) : Response.json({ format: 1 });
      }
      if (url === '/v1/project/init') return Response.json({ open: true });
      if (url === '/v1/project/open') {
        const path = JSON.parse(String(init.body)).path as string;
        const status = { empty: 404, badkey: 422, busy: 409, '..': 403, yamlonly: 428 }[path];
        if (status) return Response.json({ error: status === 409 ? 'a job is running; switch projects when it ends' : 'no' }, { status });
        return Response.json({ open: true, path: `/home/u/${path}`, name: path });
      }
      if (url === '/v1/project/new') {
        const path = JSON.parse(String(init.body)).path as string;
        if (path.endsWith('/proj')) return Response.json({ error: 'the folder is not empty' }, { status: 409 });
        return Response.json({ open: true, path, name: path.split('/').pop() });
      }
      if (init.method === 'POST' && ['/v1/gen', '/v1/build', '/v1/sign', '/v1/release', '/v1/flash'].includes(url)) return Response.json({ job: calls.length }, { status: 202 });
      return Response.json({});
    }) as unknown as typeof fetch,
    socket: () => {
      sock = { send: (s: string) => sent.push(s) };
      queueMicrotask(() => sock?.onopen?.());
      return sock as unknown as WebSocket;
    },
    storage: { getItem: (k) => store.get(k) ?? null, setItem: (k, v) => void store.set(k, v) },
    location: { hash: opts.hash ?? `#${PASS}`, origin: 'http://127.0.0.1:7000', host: '127.0.0.1:7000', protocol: 'http:', pathname: '/', search: '' },
    history: { replaceState: (_s: unknown, _t: string, url?: string | URL | null) => void (replaced = String(url)) },
    now: () => 1000,
  };
  return { deps, calls, store, sent, sock: () => sock!, replaced: () => replaced };
}

describe('the layout file', () => {
  test('takes names and finite places, of its format or none named', () => {
    expect(parseLayout({ format: 1, names: { a: 'A' }, positions: { a: { x: 1, y: -2.5 } } })).toMatchObject({ layout: { format: 1, names: { a: 'A' }, positions: { a: { x: 1, y: -2.5 } } } });
    expect(parseLayout({ names: { a: 'A' } })).toMatchObject({ layout: { names: { a: 'A' } } });
  });

  test.each([
    [{ format: 2 }, 'format 2'],
    [[], 'not an object'],
    [{ names: { a: 1 } }, 'names'],
    [{ positions: { a: { x: 'x', y: 0 } } }, 'positions'],
    [{ positions: { a: { x: NaN, y: 0 } } }, 'positions'],
    [{ positions: { a: { x: Infinity, y: 0 } } }, 'positions'],
    [{ positions: { a: {} } }, 'positions'],
  ])('refuses %j and says why', (v, why) => {
    expect(parseLayout(v)).toEqual({ refused: why });
  });
});

describe('dagon serve link', () => {
  test('a job stopped by the lock table is not started (no request)', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    const n = f.calls.length;
    cli.guard = () => true;
    expect(await cli.build(['seg'])).toBeNull();
    expect(await cli.flash('seg')).toBeNull();
    expect(f.calls.length).toBe(n);
  });

  test('the passphrase moves from the URL fragment to sessionStorage, rides as Bearer, and opens the event stream', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    await Promise.resolve();
    expect(cli.state).toBe('ready');
    expect(f.store.get('dagon-serve-pass')).toBe(PASS);
    expect(f.replaced()).toBe('/');
    expect(f.calls[0]).toMatchObject({ url: '/v1/hello', auth: `Bearer ${PASS}` });
    expect(f.sent).toEqual([PASS]);
    expect([cli.has('gen'), cli.has('flash')]).toEqual([true, false]);
    expect(JSON.stringify(cli.hello)).not.toContain('secret');
  });

  test('the definitions are asked with the passphrase; a serve without them gives none', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    expect(await cli.definitions()).toBe('{}');
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/definitions', method: 'GET', auth: `Bearer ${PASS}` });
    const old = new Cli(fake({ status: 404 }).deps);
    expect(await old.definitions()).toBeNull();
    expect(old.defsFailed).toBe(false);
    // The open project's definitions do not load: said, not replaced by the builtin set silently.
    const broken = new Cli(fake({ status: 500 }).deps);
    expect(await broken.definitions()).toBeNull();
    expect(broken.defsFailed).toBe(true);
    expect(cli.defsFailed).toBe(false);
  });

  test('a public page (no passphrase) stays K0; a wrong passphrase is K0 with its notice; a dead CLI is K3', async () => {
    const none = new Cli(fake({ hash: '' }).deps);
    await none.connect();
    expect(none.state).toBe('none');
    const denied = new Cli(fake({ status: 401 }).deps);
    await denied.connect();
    expect(denied.state).toBe('denied');
    const dead = fake();
    dead.deps.fetch = (async () => {
      throw new TypeError('connection refused');
    }) as unknown as typeof fetch;
    const stopped = new Cli(dead.deps);
    await stopped.connect();
    expect(stopped.state).toBe('stopped');
  });

  test('a job follows its events: state, progress, log, question, result; the topology goes as YAML', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    expect(await cli.saveTopology('dagon: 1\n')).toBe('saved');
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/topology', method: 'PUT', body: 'dagon: 1\n' });
    const job = (await cli.gen(['hub', 'seg']))!;
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/gen', body: JSON.stringify({ nodes: ['hub', 'seg'] }) });
    cli.event({ type: 'job.state', job: job.id, state: 'running' });
    cli.event({ type: 'job.progress', job: job.id, stage: 'gen', percent: 40 });
    cli.event({ type: 'job.log', job: job.id, lines: ['nodes/hub/src/main.rs', 'nodes/seg/src/main.rs'] });
    cli.event({ type: 'job.question', job: job.id, id: 'q1', text: 'BOOT を押しながら USB をつないでください。' });
    expect(cli.jobs[0]).toMatchObject({ state: 'question', percent: 40, question: { id: 'q1' } });
    expect(cli.jobs[0].lines).toHaveLength(2);
    await cli.answer(cli.jobs[0], 'つないだ');
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: `/v1/jobs/${job.id}/answer`, body: JSON.stringify({ answer: 'つないだ' }) });
    cli.event({ type: 'job.result', job: job.id, data: { files: 6 } });
    cli.event({ type: 'job.state', job: job.id, state: 'succeeded' });
    expect(cli.jobs[0]).toMatchObject({ state: 'succeeded', result: { files: 6 }, ended: 1000 });
  });

  test('the hub hand-over: device.need closes the page side, then tells serve it was released; device.returned gives it back', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    const seen: boolean[] = [];
    cli.onDevice = (need) => void seen.push(need);
    await cli.connect();
    cli.event({ type: 'device.need', job: 1 });
    await new Promise((r) => setTimeout(r, 0));
    expect(cli.holdsHub).toBe(true);
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/device', method: 'POST', body: JSON.stringify({ state: 'released' }) });
    cli.event({ type: 'device.returned', job: 1 });
    expect([cli.holdsHub, seen]).toEqual([false, [true, false]]);
  });

  test('the release bytes come with the passphrase', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    expect(await cli.releaseBytes('r1')).toEqual(Uint8Array.of(1, 2, 3));
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/release/r1', auth: `Bearer ${PASS}` });
  });

  test('projects: open and new follow serve (404, 422, 409, 403), and hello is read again after a switch', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    const hellos = () => f.calls.filter((c) => c.url === '/v1/hello').length;
    expect(await cli.openProject('sample')).toBe('opened');
    expect(hellos()).toBe(2);
    expect(await cli.openProject('empty')).toBe('missing');
    expect(await cli.openProject('badkey')).toBe('key');
    expect(await cli.openProject('busy')).toBe('busy');
    expect(await cli.openProject('..')).toBe('refused');
    expect(hellos()).toBe(2);
    expect(await cli.newProject('/home/u/proj', false)).toBe('exists');
    expect(await cli.newProject('/home/u/fresh', true)).toBe('opened');
    expect(f.calls[f.calls.length - 2]).toMatchObject({ url: '/v1/project/new', body: JSON.stringify({ path: '/home/u/fresh', from_sample: true }) });
    await cli.fs('/home/u/a b');
    expect(f.calls[f.calls.length - 1].url).toBe('/v1/fs?path=%2Fhome%2Fu%2Fa%20b');
  });

  test('書き込む is offered only for the MCU families serve\'s flash writes', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    expect(cli.canFlash('rp2040')).toBe(false);
    cli.hello = { ...cli.hello!, features: [...cli.hello!.features, 'flash'], flash_mcus: ['rp2040', 'stm32f401re'] };
    expect(['rp2040', 'stm32f401re', 'esp32s3'].map((m) => cli.canFlash(m))).toEqual([true, true, false]);
    cli.hello = { ...cli.hello!, flash_mcus: undefined };
    expect(cli.canFlash('rp2040')).toBe(false);
  });

  test('a folder with only a 構成 is its own answer (428), and init opens it with hello read again', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    expect(await cli.openProject('yamlonly')).toBe('yaml');
    expect(await cli.initProject('/home/u/yamlonly')).toBe('opened');
    expect(f.calls[f.calls.length - 2]).toMatchObject({ url: '/v1/project/init', method: 'POST', body: JSON.stringify({ path: '/home/u/yamlonly' }) });
    expect(f.calls[f.calls.length - 1].url).toBe('/v1/hello');
  });

  test('the layout (呼び名, positions) is read and written whole through serve, only with the layout feature', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    expect(await cli.loadLayout()).toBeNull();
    expect(f.calls.some((c) => c.url === '/v1/layout')).toBe(false);
    cli.hello = { ...cli.hello!, features: [...cli.hello!.features, 'layout'] };
    expect(await cli.loadLayout()).toBeNull();
    const l = { names: { seg: '7セグ' }, positions: { seg: { x: 10, y: 20 } } };
    expect(await cli.saveLayout(l)).toBe(true);
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/layout', method: 'PUT', body: JSON.stringify(l) });
    expect(await cli.loadLayout()).toEqual({ format: 1, ...l });
  });

  test('the latest release and a boot core come only through serve; none is null', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    expect(await cli.latestRelease()).toEqual(Uint8Array.of(1, 2, 3));
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/release/latest', auth: `Bearer ${PASS}` });
    // No `bootcore` feature in hello: nothing is asked.
    const n = f.calls.length;
    expect(await cli.bootcore('seg')).toBeNull();
    expect(f.calls.length).toBe(n);
  });

  test('kept packages are listed, read and kept through serve only, when serve offers them', async () => {
    const f = fake();
    const cli = new Cli(f.deps);
    await cli.connect();
    const n = f.calls.length;
    expect(await cli.keptList()).toEqual([]);
    expect(await cli.keep(Uint8Array.of(9))).toBeNull();
    expect(f.calls.length).toBe(n);
    cli.hello = { ...cli.hello!, features: [...cli.hello!.features, 'kept'] };
    expect(await cli.keptList()).toEqual([{ id: 'v1.0.0', versions: { hub: '1.0.0' }, bytes: 3 }]);
    expect(await cli.keptBytes('v1.0.0')).toEqual(Uint8Array.of(1, 2, 3));
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/kept/v1.0.0', auth: `Bearer ${PASS}` });
    expect(await cli.keep(Uint8Array.of(9))).toBe('v1.1.0');
    expect(f.calls[f.calls.length - 1]).toMatchObject({ url: '/v1/kept', method: 'POST', body: Uint8Array.of(9) });
  });

  test('a save serve refuses (422, 409) or cannot take keeps the edits on the page, marked unsaved, with the reason', async () => {
    const f = fake();
    const answers: (number | 'down')[] = [422, 409, 200, 'down'];
    const base = f.deps.fetch!;
    f.deps.fetch = (async (url: string, init: RequestInit) => {
      if (url === '/v1/topology' && init.method === 'PUT') {
        const status = answers.shift()!;
        if (status === 'down') throw new TypeError('connection refused');
        return status === 200 ? Response.json({ updatable: true }) : Response.json({ error: 'no' }, { status });
      }
      return base(url, init);
    }) as unknown as typeof fetch;
    const cli = new Cli(f.deps);
    await cli.connect();
    const d = new Define();
    d.placeSample();
    const yaml = d.exportYaml().text;
    const save = async () => d.saved(d.revision, await cli.saveTopology(d.exportYaml().text));

    await save();
    expect([d.saveFailure, d.unsaved, d.exportYaml().text]).toEqual(['invalid', true, yaml]);
    await save();
    expect([d.saveFailure, d.unsaved]).toEqual(['busy', true]);
    await save();
    expect([d.saveFailure, d.unsaved]).toEqual([null, false]);

    d.undo();
    await save();
    expect([d.saveFailure, d.unsaved, d.doc.topo.nodes]).toEqual(['failed', true, []]);
  });

  test('dagon demo refuses an uplink it cannot simulate: not saved, and the page says why', async () => {
    const f = fake();
    const base = f.deps.fetch!;
    f.deps.fetch = (async (url: string, init: RequestInit) => {
      if (url === '/v1/topology' && init.method === 'PUT') {
        return Response.json({ report: { issues: [] }, demo_uplink: { kind: 'ble', host: 'browser' } }, { status: 422 });
      }
      return base(url, init);
    }) as unknown as typeof fetch;
    const cli = new Cli(f.deps);
    await cli.connect();
    const d = new Define();
    d.placeSample();
    d.saved(d.revision, await cli.saveTopology(d.exportYaml().text));
    expect([d.saveFailure, d.unsaved]).toEqual(['demoLink', true]);
    expect(SAVE.demoLink.sentence).toContain('USB');
  });
});

describe('versions', () => {
  test('the Console is the cli component and speaks serve API 1', () => {
    expect(CONSOLE_VERSION).toBe(pkg.version);
    expect(SERVE_API).toBe(1);
  });

  test('a dagon serving another API number, or none, does not match', async () => {
    const { deps } = fake();
    const cli = new Cli(deps);
    await cli.connect();
    expect(cli.apiMatches).toBe(true);
    cli.hello = { ...cli.hello!, api: 2 };
    expect(cli.apiMatches).toBe(false);
    cli.hello = { ...cli.hello!, api: undefined };
    expect(cli.apiMatches).toBe(false);
  });
});
