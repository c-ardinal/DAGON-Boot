import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { afterEach, describe, expect, test, vi } from 'vitest';
import { Agent, KeyDraft, type AgentApi, type AgentStatus } from './agent.svelte.ts';
import { AGENT, CLI_LINK } from './copy.ts';
import CliPanel from '../screens/define/CliPanel.svelte';

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
const KEY = 'sk-test-0123456789';

const status = (key: boolean): AgentStatus => ({
  agents: [
    {
      id: 'claude',
      name: 'Claude',
      provider: 'Anthropic',
      ways: [
        { id: 'claude-code', name: 'Claude Code', kind: 'command', available: true, key: null },
        { id: 'api', name: 'API キー', kind: 'messages-http', available: key, key },
      ],
    },
  ],
  default: { agent: 'claude', way: 'claude-code' },
  linked: false,
  consent: null,
  available: true,
  provider: { id: 'claude', name: 'Claude (Anthropic)' },
  errors: [],
  sends: [],
  never_sends: [],
});

/** serve as the page sees it: the key goes in, only 登録済み / 未登録 comes back. */
function serve(fail?: { status: number; body: unknown }) {
  const sent: { method: string; path: string; body?: unknown }[] = [];
  let stored = false;
  const api: AgentApi = async (method, path, body) => {
    sent.push({ method, path, body });
    if (path === 'agent') return json(status(stored));
    if (fail) return json(fail.body, fail.status);
    stored = method === 'PUT';
    return json(status(stored));
  };
  return { api, sent };
}

afterEach(() => vi.unstubAllGlobals());

describe('the API key in the Console (docs/agent-api.md, PUT/DELETE /v1/agent/key)', () => {
  test('saving sends the key to serve once and clears the field; the status says 登録済み', async () => {
    const s = serve();
    const a = new Agent(s.api);
    await a.refresh();
    const d = new KeyDraft(a, { agent: 'claude', way: 'api' });
    d.value = KEY;
    const saving = d.save();
    expect(d.value).toBe('');
    await saving;
    expect(d.problem).toBeNull();
    expect(s.sent[s.sent.length - 1]).toEqual({ method: 'PUT', path: 'agent/key/claude/api', body: { key: KEY } });
    expect(a.status?.agents[0].ways[1].key).toBe(true);
    expect(JSON.stringify(a.status)).not.toContain(KEY);
    await d.remove();
    expect(s.sent[s.sent.length - 1]).toMatchObject({ method: 'DELETE', path: 'agent/key/claude/api' });
    expect(a.status?.agents[0].ways[1].key).toBe(false);
  });

  test('an empty field sends nothing', async () => {
    const s = serve();
    const d = new KeyDraft(new Agent(s.api), { agent: 'claude', way: 'api' });
    d.value = '   ';
    await d.save();
    expect(s.sent).toEqual([]);
  });

  test('a store that cannot be used: the field is cleared anyway and serve’s reason is shown', async () => {
    const why = 'the Secret Service cannot be used (no session bus); install and start one';
    const s = serve({ status: 503, body: { error: why, code: 'AGENT_KEY_STORE' } });
    const d = new KeyDraft(new Agent(s.api), { agent: 'claude', way: 'api' });
    d.value = KEY;
    await d.save();
    expect(d.value).toBe('');
    expect(d.problem).toEqual({ ...AGENT.errors.AGENT_KEY_STORE, text: why });
    const refused = new KeyDraft(new Agent(serve({ status: 400, body: { error: 'the key is empty' } }).api), { agent: 'claude', way: 'api' });
    refused.value = KEY;
    await refused.save();
    expect(refused.problem).toEqual(AGENT.errors.KEY_REFUSED);
  });

  test('no browser storage is touched while a key is saved', async () => {
    const touched: string[] = [];
    const spy = (name: string) => ({ getItem: () => (touched.push(name), null), setItem: () => touched.push(name), removeItem: () => touched.push(name) });
    vi.stubGlobal('localStorage', spy('localStorage'));
    vi.stubGlobal('sessionStorage', spy('sessionStorage'));
    const d = new KeyDraft(new Agent(serve().api), { agent: 'claude', way: 'api' });
    d.value = KEY;
    await d.save();
    expect(touched).toEqual([]);
    for (const f of ['./agent.svelte.ts', '../screens/define/CliPanel.svelte']) {
      expect(new TextDecoder().decode(readFileSync(new URL(f, import.meta.url)))).not.toMatch(/localStorage|sessionStorage|indexedDB|document\.cookie/);
    }
  });

  test('the panel: a masked field, 登録済み / 未登録, and a red delete that says why on hover', async () => {
    const a = new Agent(serve().api);
    a.status = status(true);
    const cli = { state: 'ready', hello: { workspace: 'w', key: false, key_id: null, features: [] }, has: () => false, jobs: [] };
    const body = render(CliPanel, { props: { cli, d: { config: { nodes: [], reach: [] }, problems: [] }, autosave: {}, panel: 'link', onuse: () => undefined, agent: a } as never }).body;
    expect(body).toContain(CLI_LINK.keys.set);
    expect(body).toMatch(/<input[^>]*type="password"[^>]*>/);
    expect(body).toMatch(/<input[^>]*autocomplete="off"[^>]*>/);
    expect(body).not.toContain(KEY);
    expect(body).toMatch(new RegExp(`<button[^>]*class="secondary destructive[^"]*"[^>]*title="${CLI_LINK.keys.removeWhy}"[^>]*>${CLI_LINK.keys.remove}</button>`));
    a.status = status(false);
    const unset = render(CliPanel, { props: { cli, d: { config: { nodes: [], reach: [] }, problems: [] }, autosave: {}, panel: 'link', onuse: () => undefined, agent: a } as never }).body;
    expect(unset).toContain(CLI_LINK.keys.unset);
    expect(unset).not.toContain(CLI_LINK.keys.remove);
  });
});
