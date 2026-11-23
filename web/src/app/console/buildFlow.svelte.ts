// 準備's two actions as serve's jobs: ビルドする (build every node, sign with the project's key,
// then make the release 書込 offers first) and テンプレート生成 (the nodes' skeleton crates).
import { untrack } from 'svelte';
import type { Autosave } from '../define/autosave.svelte.ts';
import type { Cli, Job } from '../define/cli.svelte.ts';
import { buildingNow, buildStateOf, type BuildingMemo, type BuildState } from '../jobFlow.ts';
import type { Control } from '../locks.ts';

type Step = 'build' | 'sign' | 'release';

export interface BuildDeps {
  cli: Cli;
  autosave: Autosave;
  /** The nodes updating reaches: what is built, signed and generated. */
  reach: () => string[];
  /** The 構成's revision now: a finished build remembers it. */
  revision: () => number;
  stopped: (c: Control) => boolean;
  /** A build finished: `n` nodes were built. */
  done: (n: number) => void;
}

export class BuildFlow {
  private ids = $state<Record<Step, number | null>>({ build: null, sign: null, release: null });
  /** Serve would not take one of the build's jobs. */
  startFailed = $state(false);
  /** The build failure modal is open. */
  failureOpen = $state(false);
  readonly jobs = $derived({ build: this.jobOf(this.ids.build), sign: this.jobOf(this.ids.sign), release: this.jobOf(this.ids.release) });
  readonly state: BuildState = $derived(buildStateOf(this.jobs, this.startFailed));
  /** The 構成 as it was when a build last finished: while it stays so, the next step is to write. */
  builtRevision = $state<number | null>(null);
  /** While building, only the node cargo builds now carries the edge light; it goes when the next is named or the job ends. */
  readonly building: readonly string[] = $derived.by(() => {
    this.memo = buildingNow(this.jobs, this.deps.reach(), this.memo);
    return this.memo.node ? [this.memo.node] : [];
  });

  /** テンプレート生成: the modal is open while set; `id` is serve's job once it has taken it. */
  skeleton = $state<{ id: number | null; failure?: string } | null>(null);
  readonly skeletonJob: Job | null = $derived(this.jobOf(this.skeleton?.id ?? null));

  private memo: BuildingMemo = { job: null, node: null };
  private announced: BuildState = null;

  /** Call it where an effect may be made (a component's script). */
  constructor(private readonly deps: BuildDeps) {
    $effect(() => {
      const s = this.state;
      // Each step's end starts the next while the state stays "running".
      const [built, signed] = [this.jobs.build?.state, this.jobs.sign?.state];
      untrack(() => {
        if (built === 'succeeded' && this.ids.sign === null) this.step('sign', () => deps.cli.sign(deps.reach()));
        if (signed === 'succeeded' && this.ids.release === null) this.step('release', () => deps.cli.release());
        if (s === this.announced) return;
        this.announced = s;
        if (s === 'done') {
          this.builtRevision = deps.revision();
          deps.done(deps.reach().length);
        }
        if (s === 'failed') this.failureOpen = true;
      });
    });
    // 中止: the modal goes with the job.
    $effect(() => {
      const s = this.skeletonJob?.state;
      if (s === 'cancelled') untrack(() => (this.skeleton = null));
      if (s === 'succeeded') untrack(() => void deps.cli.refreshHello());
    });
  }

  private jobOf(id: number | null): Job | null {
    return id === null ? null : (this.deps.cli.jobs.find((j) => j.id === id) ?? null);
  }

  async build(): Promise<void> {
    const { cli, autosave } = this.deps;
    if (this.deps.stopped('build') || !cli.has('build')) return;
    this.failureOpen = false;
    this.startFailed = false;
    this.ids = { build: null, sign: null, release: null };
    try {
      await autosave.whenSaved(async () => {
        const j = await cli.build(this.deps.reach());
        this.ids = { build: j?.id ?? null, sign: null, release: null };
      });
    } catch {
      this.startFailed = true;
    }
  }

  /** Starts the next step of the build; a step serve would not take ends it as a failure. */
  private step(step: 'sign' | 'release', start: () => Promise<{ id: number } | null>): void {
    this.ids = { ...this.ids, [step]: -1 };
    void start()
      .then((j) => {
        if (!j) this.startFailed = true;
        this.ids = { ...this.ids, [step]: j?.id ?? -1 };
      })
      .catch(() => (this.startFailed = true));
  }

  /** テンプレート生成, made from the saved YAML: after the saves queued before it. `failed` is the modal's word when serve would not take it. */
  async makeSkeleton(failed: string): Promise<void> {
    this.skeleton = { id: null };
    let job: { id: number } | null = null;
    try {
      await this.deps.autosave.whenSaved(async () => (job = await this.deps.cli.gen(this.deps.reach())));
      this.skeleton = job ? { id: (job as { id: number }).id } : null;
    } catch {
      this.skeleton = { id: null, failure: failed };
    }
  }
}
