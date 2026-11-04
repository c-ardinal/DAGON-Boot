// DAGON, the page's side of the agent link (docs/agent-api.md on the CLI):
// GET /v1/agent, POST/DELETE /v1/agent/link (consent asked once, when
// linking), POST /v1/agent/ask (no confirmation per question). The key never
// comes here; the provider's name is shown only in the linking step.
import { AGENT } from './copy.ts';

/** One way to reach an agent: its own login through a program, or an HTTP API with a key. */
export interface AgentWay {
  id: string;
  name: string;
  kind: 'command' | 'messages-http' | string;
  /** It can be used on this machine now (program on PATH, key set). */
  available: boolean;
}

export interface AgentDef {
  id: string;
  name: string;
  provider: string;
  ways: AgentWay[];
}

/** The agent and the way DAGON talks through. */
export interface AgentChoice {
  agent: string;
  way: string;
}

export interface AgentStatus {
  agents: AgentDef[];
  /** The way the linking step preselects. */
  default: AgentChoice;
  linked: boolean;
  consent: (AgentChoice & { at: string }) | null;
  /** The linked (or default) way can be used here. */
  available: boolean;
  /** The agent's name for the linking texts ("Claude (Anthropic)"). */
  provider: { id: string; name: string };
  /** User definition files that did not load. */
  errors: string[];
  sends: string[];
  never_sends: string[];
}

export type AskKind = 'problem' | 'next' | 'propose' | 'failure' | 'free';
export type Selection = { node: string } | { link: { from: string; to: string } };

export interface Proposal {
  /** Lines prefixed ' ', '-', '+'. */
  yaml_diff: string;
  /** The whole 構成 YAML the proposal leads to. */
  topology: string;
}

export interface Message {
  role: 'user' | 'dagon';
  text: string;
  /** The rest of an answer after its first sentence (the 会話 tab only). */
  more?: string;
  proposal?: Proposal | null;
  /** A reply that is a failure (red), not information. */
  failed?: boolean;
  /** The proposal went into the 構成. */
  applied?: boolean;
}

/** Why a call did not go through: information unless `failed`. */
export interface AgentProblem {
  text: string;
  failed: boolean;
}

/** One HTTP call to serve: the Cli in the product, a stand-in in tests and the dev mock. */
export type AgentApi = (method: string, path: string, body?: unknown) => Promise<Response>;

/** Questions sent with the conversation's last turns, at most this many. */
const HISTORY = 10;
/** The bubble shows the first sentence only, cut to this many characters. */
export const BUBBLE_MAX = 110;

export function problemOf(status: number, code: string | undefined): AgentProblem {
  const known = code ? (AGENT.errors as Record<string, AgentProblem>)[code] : undefined;
  if (known) return known;
  if (status === 403) return AGENT.errors.DEMO;
  if (status === 404) return AGENT.errors.OLD_CLI;
  return AGENT.errors.AGENT_FAILED;
}

/** The answer's first part (up to the first blank line, ≤ 110 characters) and the rest. */
export function splitAnswer(answer: string): { text: string; more?: string } {
  const at = answer.indexOf('\n\n');
  const head = (at < 0 ? answer : answer.slice(0, at)).trim();
  const rest = at < 0 ? '' : answer.slice(at + 2).trim();
  const text = head.length > BUBBLE_MAX ? `${head.slice(0, BUBBLE_MAX - 1)}…` : head;
  const more = [head.length > BUBBLE_MAX ? head : '', rest].filter(Boolean).join('\n\n');
  return more ? { text, more } : { text };
}

export class Agent {
  status = $state<AgentStatus | null>(null);
  thinking = $state(false);
  messages = $state<Message[]>([]);

  /** The lock table's check, set by the page: true when it is stopped now (the page has shown why). */
  guard: (kind: 'ask' | 'link') => boolean = () => false;

  constructor(private readonly api: AgentApi) {}

  /** Linked: DAGON is shown and questions go out without asking again. */
  get linked(): boolean {
    return this.status?.linked === true;
  }

  async refresh(): Promise<void> {
    try {
      const r = await this.api('GET', 'agent');
      this.status = r.ok ? ((await r.json()) as AgentStatus) : null;
    } catch {
      this.status = null;
    }
  }

  /** POST (with the chosen agent and way) or DELETE /v1/agent/link; null when it went through, else why not. */
  async setLink(choice: AgentChoice | null): Promise<AgentProblem | null> {
    if (this.guard('link')) return { text: '', failed: false };
    try {
      const r = choice ? await this.api('POST', 'agent/link', { ...choice, consent: true }) : await this.api('DELETE', 'agent/link');
      if (!r.ok) return problemOf(r.status, await codeOf(r));
      this.status = (await r.json()) as AgentStatus;
      if (!choice) this.messages = [];
      return null;
    } catch {
      return AGENT.errors.AGENT_FAILED;
    }
  }

  /** Sends a question at once (consent was given when linking); the answer or why not joins the conversation. */
  async ask(question: string, kind: AskKind, selection?: Selection): Promise<void> {
    if (this.guard('ask')) return;
    // Sending needs the consent given when linking: without it nothing leaves the page (serve refuses too).
    if (!this.linked) return this.say(AGENT.errors.AGENT_NOT_LINKED);
    const history = this.messages.slice(-HISTORY).map((m) => ({ role: m.role, text: [m.text, m.more].filter(Boolean).join('\n\n') }));
    this.messages = [...this.messages, { role: 'user', text: question }];
    this.thinking = true;
    try {
      const r = await this.api('POST', 'agent/ask', { question, kind, ...(selection ? { selection } : {}), history });
      if (!r.ok) return this.say(problemOf(r.status, await codeOf(r)));
      const a = (await r.json()) as { answer: string; proposal?: Proposal | null };
      this.messages = [...this.messages, { role: 'dagon', ...splitAnswer(a.answer), proposal: a.proposal ?? null }];
    } catch {
      this.say(AGENT.errors.AGENT_FAILED);
    } finally {
      this.thinking = false;
    }
  }

  private say(p: AgentProblem) {
    this.messages = [...this.messages, { role: 'dagon', text: p.text, failed: p.failed }];
  }

  /** The proposal of message `i` went into the 構成. */
  applied(i: number) {
    this.messages = this.messages.map((m, k) => (k === i ? { ...m, applied: true } : m));
  }
}

async function codeOf(r: Response): Promise<string | undefined> {
  const body = (await r.json().catch(() => ({}))) as { code?: unknown };
  return typeof body.code === 'string' ? body.code : undefined;
}
