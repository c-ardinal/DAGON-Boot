// DAGON's agent on the page: linking and unlinking, asking about the selection, and taking in
// a proposed 構成.
import { untrack } from 'svelte';
import { AGENT } from '../copy.ts';
import { consentProblem, type Agent, type AgentChoice, type AskKind, type Selection } from '../agent.svelte.ts';
import type { Define } from '../define/define.svelte.ts';
import { fromYaml } from '../define/model.ts';
import type { LockGate } from './lockGate.svelte.ts';
import type { Feedback } from './feedback.svelte.ts';

export interface AgentLinkDeps {
  agent: Agent;
  d: Define;
  gate: LockGate;
  feedback: Feedback;
  /** The demo page's simulator: no link, no talk. */
  simulated: boolean;
  /** The key of what is selected in the mode shown ("node:<id>" or "link:<id>"), or null. */
  selected: () => string | null;
}

export class AgentLink {
  /** The linking dialog is open. */
  dialogOpen = $state(false);
  /** Requests to open DAGON's bubble: each change opens it. */
  openRequest = $state(0);
  /** The DAGON bubble is open: Escape closes it first, wherever focus is. */
  bubbleOpen = $state(false);
  /** The last proposal not yet taken in, or -1. */
  readonly lastProposal = $derived(this.deps.agent.messages.map((m) => !!m.proposal && !m.applied).lastIndexOf(true));

  /** Call it where an effect may be made (a component's script). */
  constructor(private readonly deps: AgentLinkDeps) {
    // A consent of a version this dagon does not read: a failure the user must notice.
    $effect(() => {
      const p = consentProblem(deps.agent.status);
      const why = deps.agent.status?.consent_error ?? '';
      if (p) untrack(() => (deps.feedback.notice = { word: p.label ?? p.text, error: true, detail: `${p.text}（${why}）` }));
    });
  }

  async openLink(at?: Element | null): Promise<void> {
    const { agent, gate } = this.deps;
    if (this.deps.simulated || gate.stopped('link', at)) return;
    if (!agent.status) await agent.refresh();
    if (!agent.status) return gate.showWhy(AGENT.errors.OLD_CLI.text, at);
    this.dialogOpen = true;
  }

  async agree(choice: AgentChoice): Promise<void> {
    const { agent, gate, feedback } = this.deps;
    this.dialogOpen = false;
    const p = await agent.setLink(choice);
    // An empty text: the lock table stopped it and has already said why.
    if (p) return void (p.text && gate.showWhy(p.text, null, p.failed));
    feedback.note(AGENT.linked(agent.status?.provider.name ?? '', agent.status?.consent?.at ?? ''));
    this.openRequest++;
  }

  async unlink(): Promise<void> {
    const { agent, gate, feedback } = this.deps;
    const p = await agent.setLink(null);
    if (p) return void (p.text && gate.showWhy(p.text, null, p.failed));
    feedback.note(AGENT.unlinked);
    if (feedback.drawerTab === 'talk') feedback.drawerTab = null;
  }

  /** What is selected, as the agent's question names it. */
  private selection(): Selection | undefined {
    const k = this.deps.selected();
    if (!k) return undefined;
    if (k.startsWith('node:')) return { node: k.slice(5) };
    const to = k.slice(5);
    const from = this.deps.d.config.links.find((l) => l.to === to)?.from;
    return from ? { link: { ends: [from, to] } } : undefined;
  }

  ask(text: string, kind: AskKind, at: Element): void {
    if (this.deps.gate.stopped('ask', at)) return;
    void this.deps.agent.ask(text, kind, this.selection());
  }

  askWhy(at: Element, close: () => void): void {
    void this.deps.agent.askWhy(() => this.deps.gate.stopped('ask', at), close, this.selection());
  }

  /** 『反映する』: the proposal goes in as one 構成 edit (undoable) and is saved like any edit. */
  applyProposal(i: number, at: Element): void {
    const { agent, d, gate, feedback } = this.deps;
    if (gate.stopped('apply', at)) return;
    const p = agent.messages[i]?.proposal;
    if (!p) return;
    try {
      if (!d.importRelease(fromYaml(p.topology, d.doc.names).topo)) return;
    } catch {
      return gate.showWhy(AGENT.errors.AGENT_FAILED.text, at, true);
    }
    agent.applied(i);
    feedback.note(AGENT.applied);
  }
}
