import { describe, expect, test } from 'vitest';
import { Cli, type CliDeps } from './cli.svelte.ts';

const PASS = '0123456789abcdef0123456789abcdef';
const HELLO = { cli: '0.1.0', workspace: 'my-fleet', features: ['build', 'sign', 'release', 'gen'], api_key: false, key: true, key_id: '3f9a21c7aabbccdd' };

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
  const deps: CliDeps = {
    fetch: (async (url: string, init: RequestInit) => {
      calls.push({ url, method: init.method ?? 'GET', auth: (init.headers as Record<string, string>).Authorization, body: init.body });
      if (opts.status) return new Response('', { status: opts.status });
      if (url === '/v1/hello') return Response.json(HELLO);
      if (url.startsWith('/v1/release/')) return new Response(Uint8Array.of(1, 2, 3));
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

describe('dagon serve link', () => {
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
    await cli.saveTopology('dagon: 1\n');
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
});
