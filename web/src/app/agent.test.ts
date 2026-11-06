import { describe, expect, test } from 'vitest';
import { Agent, consentProblem, problemOf, splitAnswer, type AgentApi, type AgentStatus } from './agent.svelte.ts';
import { agentMock } from './agentMock.ts';
import { AGENT, BUILD } from './copy.ts';

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
const now = () => Promise.resolve();

describe('DAGON and its agent (site/src/content/docs/reference/agent-api.md)', () => {
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

  test('the record of a write that went back goes with every question while there is one, and not otherwise', async () => {
    const sent: { body: Record<string, unknown> }[] = [];
    const a = new Agent(async (_m, path, body) => (path === 'agent' ? json({ linked: true }) : (sent.push({ body: body as Record<string, unknown> }), json({ answer: 'はい。' }))));
    await a.refresh();
    await a.ask('次は？', 'next');
    const rollback = { node: 'watch', failed_version: '1.1.0', returned_version: '1.0.0', reason: 'ROLLED BACK', node_state: { trial: false, last_reason: 3 } };
    let record: typeof rollback | null = rollback;
    a.record = async () => record;
    await a.ask('失敗の理由を聞く', 'failure');
    record = null;
    await a.ask('次は？', 'next');
    expect(sent.map((s) => 'rollback' in s.body)).toEqual([false, true, false]);
    expect(sent[1].body.rollback).toEqual(rollback);
  });

  test('a question waits for the record to be complete, showing thinking meanwhile', async () => {
    const sent: unknown[] = [];
    const a = new Agent(async (_m, path, body) => (path === 'agent' ? json({ linked: true }) : (sent.push(body), json({ answer: 'はい。' }))));
    await a.refresh();
    let read = (_: null) => undefined as void;
    a.record = () => new Promise((r) => (read = r));
    const asked = a.ask('失敗の理由を聞く', 'failure');
    await now();
    expect([sent.length, a.thinking]).toEqual([0, true]);
    read(null);
    await asked;
    expect([sent.length, a.thinking]).toEqual([1, false]);
  });

  test('asking why from a modal closes it first, so its scrim does not cover the answer', async () => {
    const order: string[] = [];
    const a = new Agent(async (_m, path, body) => {
      if (path === 'agent') return json({ linked: true });
      order.push(`sent ${(body as { kind: string }).kind}`);
      return json({ answer: 'はい。' });
    });
    await a.refresh();
    await a.askWhy(
      () => false,
      () => order.push('closed'),
      { node: 'seg' },
    );
    expect(order).toEqual(['closed', 'sent failure']);
    expect(a.messages[0]).toEqual({ role: 'user', text: BUILD.askAgent });
  });

  test('asking why while the lock table stops it: the modal stays open and nothing is sent', async () => {
    const order: string[] = [];
    const a = new Agent(async (_m, path) => {
      if (path === 'agent') return json({ linked: true });
      order.push('sent');
      return json({ answer: 'はい。' });
    });
    await a.refresh();
    await a.askWhy(
      () => true,
      () => order.push('closed'),
    );
    expect(order).toEqual([]);
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

describe('AI link refusals use the agreed words', () => {
  test('too long and terms of use: short label, plain detail', () => {
    expect(AGENT.errors.AGENT_CONTEXT_TOO_LARGE).toMatchObject({ label: '× 長すぎる', text: '文字数が多すぎて送れません' });
    expect(AGENT.errors.AGENT_CONSENT_VERSION).toMatchObject({ label: '× 利用規約の確認', text: '利用規約が更新されています。連携のために確認してください。' });
  });
});

describe('refusals serve tells apart by code', () => {
  test.each([
    [409, 'AGENT_CONSENT_VERSION'],
    [413, 'AGENT_CONTEXT_TOO_LARGE'],
  ] as const)('%s %s has its own words, not the generic failure', (status, code) => {
    const p = problemOf(status, code);
    expect(p).toBe(AGENT.errors[code]);
    expect(p).not.toBe(AGENT.errors.AGENT_FAILED);
    expect(p.failed).toBe(true);
    // A short word for the notice, the sentence as its detail.
    expect(p.label).toMatch(/^× \S+$/);
    expect(p.text.length).toBeGreaterThan(p.label!.length);
  });

  test("GET /v1/agent's consent_error becomes the consent-version notice", () => {
    const base = { agents: [], default: { agent: 'claude', way: 'claude-code' }, linked: false, consent: null, available: true, provider: { id: 'anthropic', name: 'Anthropic' }, errors: [], sends: [], never_sends: [] } as AgentStatus;
    expect(consentProblem({ ...base, consent_error: 'agent.json is version 9' })).toBe(AGENT.errors.AGENT_CONSENT_VERSION);
    expect(consentProblem({ ...base, consent_error: null })).toBeNull();
    expect(consentProblem(base)).toBeNull();
    expect(consentProblem(null)).toBeNull();
  });
});
