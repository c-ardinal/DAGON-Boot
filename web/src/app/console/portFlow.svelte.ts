// ボード移植 (`dagon ai port`): the modal's state, from its opening to the agent's report.
import type { Cli, Job, PortRequest } from '../define/cli.svelte.ts';

export interface PortDeps {
  cli: Cli;
  /** The text shown when serve gave no answer at all. */
  noAnswer: string;
}

export class PortFlow {
  /** The modal is open. */
  open = $state(false);
  /** The request is with serve. */
  sending = $state(false);
  /** Serve would not take the job: its reason. */
  refused = $state<string | null>(null);
  private id = $state<number | null>(null);
  readonly job: Job | null = $derived(this.id === null ? null : (this.deps.cli.jobs.find((j) => j.id === this.id) ?? null));

  constructor(private readonly deps: PortDeps) {}

  /** Opens the modal; a run still under way, or its report, stays in it. */
  show(): void {
    this.open = true;
  }

  async start(r: PortRequest): Promise<void> {
    if (this.sending) return;
    this.sending = true;
    this.refused = null;
    try {
      const took = await this.deps.cli.aiPort(r);
      if ('refused' in took) this.refused = took.refused;
      else this.id = took.id;
    } catch {
      this.refused = this.deps.noAnswer;
    } finally {
      this.sending = false;
    }
  }

  async cancel(): Promise<void> {
    if (this.job) await this.deps.cli.cancel(this.job);
  }

  /** Back to the form: the failure is put away, what the user typed stays in the modal. */
  retry(): void {
    this.refused = null;
    this.id = null;
  }

  /** Closes the modal; an ended run is put away with it, a running one goes on (作業 lists it). */
  close(): void {
    this.open = false;
    const s = this.job?.state;
    if (this.refused || (s !== 'running' && s !== 'queued' && s !== 'question')) this.retry();
  }
}
