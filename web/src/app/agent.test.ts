import { describe, expect, test } from 'vitest';
import { Agent, splitAnswer, type AgentApi } from './agent.svelte.ts';
import { agentMock } from './agentMock.ts';
import { AGENT } from './copy.ts';

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
const now = () => Promise.resolve();

describe('DAGON and its agent (docs/agent-api.md)', () => {
  test('linking records the consent once; unlinking withdraws it and clears the conversation', async () => {
    const a = new Agent(agentMock(new URLSearchParams('agent'), () => '', now));
    await a.refresh();
    expect(a.linked).toBe(false);
    expect(await a.setLink({ agent: 'claude', way: 'claude-code' })).toBeNull();
    expect(a.linked).toBe(true);
    expect(a.status?.consent).toMatchObject({ agent: 'claude', way: 'claude-code' });
    await a.ask('次は？', 'next');
    expect(a.messages.map((m) => m.role)).toEqual(['user', 'dagon']);
    expect(await a.setLink(null)).toBeNull();
    expect([a.linked, a.messages]).toEqual([false, []]);
  });

  test('a question goes out at once, with the selection and the last turns, and no confirmation', async () => {
    const sent: unknown[] = [];
    const api: AgentApi = async (_m, path, body) =>
      path === 'agent' ? json({ linked: true }) : (sent.push({ path, body }), json({ answer: '一文目です。\n\n続きです。', proposal: null }));
    const a = new Agent(api);
    await a.refresh();
    await a.ask('これは何が問題？', 'problem', { node: 'seg' });
    await a.ask('次は？', 'next');
    expect(sent[0]).toEqual({ path: 'agent/ask', body: { question: 'これは何が問題？', kind: 'problem', selection: { node: 'seg' }, history: [] } });
    expect((sent[1] as { body: { history: unknown[] } }).body.history).toEqual([
      { role: 'user', text: 'これは何が問題？' },
      { role: 'dagon', text: '一文目です。\n\n続きです。' },
    ]);
    expect(a.messages[1]).toMatchObject({ text: '一文目です。', more: '続きです。' });
    expect(a.thinking).toBe(false);
  });

  test('not linked: a question sends nothing and says how to link (serve refuses it too)', async () => {
    const sent: string[] = [];
    const a = new Agent(async (_m, path) => (sent.push(path), json({ linked: false })));
    await a.refresh();
    await a.ask('次は？', 'next');
    expect(sent).toEqual(['agent']);
    expect(a.messages[a.messages.length - 1]).toEqual({ role: 'dagon', text: AGENT.errors.AGENT_NOT_LINKED.text, failed: false });
  });

  test('stopped by the lock table: neither a question nor a link goes out', async () => {
    const sent: string[] = [];
    const a = new Agent(async (_m, path) => (sent.push(path), json({ linked: true })));
    await a.refresh();
    a.guard = () => true;
    await a.ask('次は？', 'next');
    await a.setLink({ agent: 'claude', way: 'api' });
    expect([sent, a.messages]).toEqual([['agent'], []]);
  });

  test('refusals by code: not linked, no key and busy are information; timeout and failure are failures', async () => {
    for (const [status, code, failed] of [[409, 'AGENT_NOT_LINKED', false], [409, 'AGENT_UNAVAILABLE', false], [429, 'AGENT_BUSY', false], [504, 'AGENT_TIMEOUT', true], [502, 'AGENT_FAILED', true]] as const) {
      const a = new Agent(async (_m, path) => (path === 'agent' ? json({ linked: true }) : json({ error: 'x', code }, status)));
      await a.refresh();
      await a.ask('次は？', 'next');
      expect(a.messages[a.messages.length - 1], code).toEqual({ role: 'dagon', text: AGENT.errors[code].text, failed });
    }
  });

  test('dagon demo refuses linking: said calmly', async () => {
    const a = new Agent(async () => json({ error: 'demo' }, 403));
    expect(await a.setLink({ agent: 'claude', way: 'api' })).toEqual(AGENT.errors.DEMO);
  });

  test('linking sends the chosen agent and way with consent true (changing the way links again)', async () => {
    const sent: unknown[] = [];
    const a = new Agent(async (m, path, body) => (sent.push({ m, path, body }), json({ linked: true })));
    await a.setLink({ agent: 'claude', way: 'api' });
    expect(sent).toEqual([{ m: 'POST', path: 'agent/link', body: { agent: 'claude', way: 'api', consent: true } }]);
  });

  test('the bubble holds the first part of an answer, at most 110 characters', () => {
    expect(splitAnswer('短い答えです。')).toEqual({ text: '短い答えです。' });
    const long = 'あ'.repeat(130);
    const s = splitAnswer(`${long}\n\n残り`);
    expect(s.text).toHaveLength(110);
    expect(s.more).toBe(`${long}\n\n残り`);
  });

  test('no UI text of DAGON names the provider: it comes from serve, in the linking step only', () => {
    const strings = JSON.stringify(AGENT);
    expect(strings).not.toMatch(/Claude|Anthropic/);
  });
});
