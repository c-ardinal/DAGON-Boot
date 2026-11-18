// Development stand-in for serve's agent routes (site/src/content/docs/reference/agent-api.md):
// `?agent` links on the page only, `?agent=linked`
// starts linked, `&agentdemo` refuses linking (403). The
// answers are fixed sentences, never an agent's.
import type { AgentApi, AgentStatus } from './agent.svelte.ts';

export const MOCK_TEXTS = (): string[] => [...SENDS, ...NEVER, ...Object.values(ANSWERS)];

const SENDS = ['構成と定義の事実（ノード・リンク・部品・設定値）', '選択したノードやリンク', '確認の結果（問題・◇ 制限・書込の記録）'];
const NEVER = ['キー', 'コマンドとその出力の全文', 'ファイルのパス'];
const ANSWERS: Record<string, string> = {
  problem: 'このノードに問題はありません。\n\n確認の結果と書込の記録のどちらにもこのノードの問題は存在しません。',
  next: 'PC接続ノードと接続し、パッケージファイルを選択してください。\n\n構成に問題はなく、このパソコンに接続したPC接続ノードとその接続先のノードへの書込が可能です。',
  propose: 'ウォッチのUARTの速度を230400にする案です。\n\n速度の向上で、書込の時間が短縮します。',
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
          { id: 'api', name: 'APIキー', kind: 'messages-http', available: false },
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
  return async (route, { body } = {}) => {
    if (route === 'GET /v1/agent') return json(status());
    if (route === 'POST /v1/agent/link' || route === 'DELETE /v1/agent/link') {
      const linking = route === 'POST /v1/agent/link';
      if (q.has('agentdemo')) return json({ error: 'demo', code: 'DEMO' }, 403);
      const b = (body ?? {}) as { agent?: string; way?: string; consent?: unknown };
      if (linking && (b.consent !== true || !b.agent || !b.way)) return json({ error: 'agent and way must name a defined agent and way' }, 400);
      consent = linking ? { agent: b.agent!, way: b.way!, at: now() } : null;
      return json(status());
    }
    if (route === 'POST /v1/agent/ask') {
      if (!consent) return json({ error: 'not linked', code: 'AGENT_NOT_LINKED' }, 409);
      await wait(1200);
      const kind = String((body as { kind?: string }).kind ?? 'free');
      const proposal = kind === 'propose' ? { yaml_diff: '   - kind: uart\n     iface: pio-uart\n-    baud: 115200\n+    baud: 230400', topology: yaml().replace('baud: 115200', 'baud: 230400') } : null;
      return json({ answer: ANSWERS[kind] ?? 'その質問には構成の事実からの回答は不可です。', proposal });
    }
    return json({ error: 'not found' }, 404);
  };
}
