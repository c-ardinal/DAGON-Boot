import { afterEach, describe, expect, test, vi } from 'vitest';
import { CLI_LINK, LOCK, START, UNSUPPORTED } from './app/copy.ts';

// The public demo page (Vite mode `demo`): the Console on the simulator, with no serve behind it.
// Fresh modules (and the Svelte runtime with them) after the mode is set, as a `vite build --mode demo` sees it.
async function demoBody(): Promise<string> {
  vi.stubEnv('MODE', 'demo');
  vi.stubEnv('DEV', false);
  vi.resetModules();
  const { render } = await import('svelte/server');
  const App = (await import('./App.svelte')).default;
  return render(App).body;
}

function stubBrowser(hash = '') {
  const fetch = vi.fn((_url: string) => Promise.reject(new Error('no network in the demo')));
  const socket = vi.fn();
  // A passphrase-shaped fragment must not make the demo page call a serve.
  vi.stubGlobal('location', { search: '', hash, href: `https://example.invalid/demo/${hash}`, origin: 'https://example.invalid', host: 'example.invalid', protocol: 'https:', pathname: '/demo/' });
  vi.stubGlobal('navigator', {});
  vi.stubGlobal('fetch', fetch);
  vi.stubGlobal('WebSocket', socket);
  vi.stubGlobal('history', { replaceState: () => undefined });
  vi.stubGlobal('sessionStorage', { getItem: () => '0123456789abcdef0123456789abcdef', setItem: () => undefined });
  return { fetch, socket };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  vi.resetModules();
});

describe('the public demo page', () => {
  test('opens on SC-S0 without Web Serial: making a 構成 is usable, opening a project is the demo-stopped project control', { timeout: 30_000 }, async () => {
    stubBrowser();
    const body = await demoBody();
    expect(body).not.toContain(UNSUPPORTED.heading);
    expect(body).not.toContain(CLI_LINK.needCli);
    expect(body).toMatch(new RegExp(`<button class="button-primary"[^>]*aria-disabled="false"[^>]*>${START.new}<`));
    expect(body).toMatch(new RegExp(`<button class="button-secondary" data-control="open"[^>]*>${START.open}<`));
  });

  test('calls no dagon serve route and opens no event stream, even with a passphrase in the URL', { timeout: 30_000 }, async () => {
    const { fetch, socket } = stubBrowser('#0123456789abcdef0123456789abcdef');
    await demoBody();
    await new Promise((r) => setTimeout(r, 0));
    expect(fetch.mock.calls.map((c) => String(c[0])).filter((u) => u.includes('/v1/'))).toEqual([]);
    expect(socket).not.toHaveBeenCalled();
  });

  test('every control stopped there says the demo reason, which names DAGON and never another product', () => {
    for (const why of Object.values(LOCK.states.site.why)) {
      expect(why).toMatch(/^このデモは/);
      expect(why).not.toMatch(/Claude/i);
    }
  });
});
