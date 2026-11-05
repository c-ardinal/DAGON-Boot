// Development stand-in for serve's agent routes (docs/agent-api.md), until
// the CLI side is merged: `?agent` links on the page only, `?agent=linked`
// starts linked, `&agentdemo` refuses linking as `dagon demo` does. The
// answers are fixed sentences, never an agent's.
import type { AgentApi, AgentStatus } from './agent.svelte.ts';

export const MOCK_TEXTS = (): string[] => [...SENDS, ...NEVER, ...Object.values(ANSWERS)];

const SENDS = ['構成と定義の事実（ノード・リンク・部品・設定値）', '選んだノードやリンク', '確かめた結果（問題・◇ 未対応・書き込みの記録）'];
const NEVER = ['鍵', 'コマンドとその出力の全文', 'ファイルのパス'];
const ANSWERS: Record<string, string> = {
  problem: 'このノードに問題はありません。\n\n確かめた結果と書き込みの記録のどちらにも、このノードの問題は出ていません。',
  next: '親機とつないで、パッケージファイルを選んでください。\n\n構成に問題はなく、このパソコンにつながる親機とその直下の子機へ書き込めます。',
  propose: 'ウォッチの UART の速さを 230400 にする案です。\n\n速さを上げると、書き込みの時間が短くなります。',
  failure: '失敗した記録はありません。',
};

/** `yaml`: the page's 構成 now, which the proposal changes. */
export function agentMock(q: URLSearchParams, yaml: () => string, wait = (ms: number) => new Promise((r) => setTimeout(r, ms))): AgentApi {
  const now = () => new Date().toISOString().slice(0, 19) + 'Z';
  let consent: AgentStatus['consent'] = q.get('agent') === 'linked' ? { agent: 'claude', way: 'claude-code', at: now() } : null;
  const status = (): AgentStatus => ({
    agents: [
      {
        id: 'claude',
        name: 'Claude',
        provider: 'Anthropic',
        ways: [
          { id: 'claude-code', name: 'Claude Code', kind: 'command', available: true },
          { id: 'api', name: 'API キー', kind: 'messages-http', available: false },
        ],
      },
    ],
    default: { agent: 'claude', way: 'claude-code' },
    linked: consent !== null,
    consent,
    available: consent?.way !== 'api',
    provider: { id: 'claude', name: 'Claude (Anthropic)' },
    errors: [],
    sends: SENDS,
    never_sends: NEVER,
  });
  const json = (body: unknown, code = 200) => new Response(JSON.stringify(body), { status: code, headers: { 'Content-Type': 'application/json' } });
  return async (method, path, body) => {
    if (path === 'agent') return json(status());
    if (path === 'agent/link') {
      if (q.has('agentdemo')) return json({ error: 'demo', code: 'DEMO' }, 403);
      const b = (body ?? {}) as { agent?: string; way?: string; consent?: unknown };
      if (method === 'POST' && (b.consent !== true || !b.agent || !b.way)) return json({ error: 'agent and way must name a defined agent and way' }, 400);
      consent = method === 'POST' ? { agent: b.agent!, way: b.way!, at: now() } : null;
      return json(status());
    }
    if (path === 'agent/ask') {
      if (!consent) return json({ error: 'not linked', code: 'AGENT_NOT_LINKED' }, 409);
      await wait(1200);
      const kind = String((body as { kind?: string }).kind ?? 'free');
      const proposal = kind === 'propose' ? { yaml_diff: '   - from: hub\n     to: watch\n     kind: uart\n-    baud: 115200\n+    baud: 230400', topology: yaml().replace('baud: 115200', 'baud: 230400') } : null;
      return json({ answer: ANSWERS[kind] ?? 'その質問には、構成の事実からは答えられません。', proposal });
    }
    return json({ error: 'not found' }, 404);
  };
}
