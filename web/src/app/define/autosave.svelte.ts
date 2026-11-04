// 構成 changes go to the project's YAML by themselves (ui-design UC-D04, §5.2):
// a save follows the last edit by AUTOSAVE_MS, one at a time; 保存 saves now.
// A save that does not go through leaves the edits on the page (Define keeps
// them and says why) and the next edit tries again. Nothing leaves the
// project with edits unsaved: leaving saves first and, when that fails, asks
// (save again / discard / stay).
import type { SaveResult } from './cli.svelte.ts';
import type { Define } from './define.svelte.ts';

/** Quiet time after the last edit before the 構成 is saved. */
export const AUTOSAVE_MS = 500;

export interface Timers {
  set: (f: () => void, ms: number) => unknown;
  clear: (t: unknown) => void;
}

const REAL: Timers = {
  set: (f, ms) => setTimeout(f, ms),
  clear: (t) => clearTimeout(t as ReturnType<typeof setTimeout>),
};

export interface AutosaveDeps {
  /** PUT /v1/topology. */
  put: (yaml: string) => Promise<SaveResult>;
  /** There is a project to save to (not a development stand-in's unsaved 構成, not SC-S0). */
  enabled: () => boolean;
  /**
   * Saving is refused (the project's definitions do not load): no save is
   * scheduled or sent, whatever asks for it; leaving then asks as for a
   * failed save.
   */
  blocked?: () => boolean;
  timers?: Timers;
  delayMs?: number;
}

export class Autosave {
  /** Where the page was going when a save before leaving failed: the dialog asks. */
  asking = $state<(() => void) | null>(null);

  private timer: unknown = null;
  /** Saves run one after another (PUT /v1/topology is not ordered by itself). */
  private chain: Promise<boolean> = Promise.resolve(true);
  private readonly timers: Timers;
  private readonly delayMs: number;

  constructor(
    private readonly d: Define,
    private readonly deps: AutosaveDeps,
  ) {
    this.timers = deps.timers ?? REAL;
    this.delayMs = deps.delayMs ?? AUTOSAVE_MS;
  }

  /** Edits on the page the project's YAML does not have. */
  get pending(): boolean {
    return this.deps.enabled() && this.d.unsaved;
  }

  /** An edit happened: save after the quiet time (each edit starts it again). */
  edited(): void {
    this.cancel();
    if (!this.pending || this.isBlocked()) return;
    this.timer = this.timers.set(() => {
      this.timer = null;
      void this.now();
    }, this.delayMs);
  }

  /** Saves at once ("保存", and before leaving or operating); true when the YAML has every edit. */
  now(): Promise<boolean> {
    this.cancel();
    this.chain = this.chain.then(async () => {
      if (!this.pending) return true;
      // The one place every save goes through (timer, 保存, leaving, unload).
      if (this.isBlocked()) return false;
      const at = this.d.revision;
      this.d.saved(at, await this.deps.put(this.d.exportYaml().text));
      return !this.d.unsaved;
    });
    return this.chain;
  }

  /** The YAML has every edit (saving first when it does not): 運用 and 親機へ送る use the saved 構成. */
  ready(): Promise<boolean> {
    return this.pending ? this.now() : Promise.resolve(true);
  }

  /**
   * Runs `then` once the project's YAML has every edit, after the saves
   * already queued (「プロジェクトを作る」: gen reads that YAML, C-377);
   * false, without running it, when the save does not go through.
   */
  async whenSaved(then: () => Promise<unknown>): Promise<boolean> {
    if (!(await this.ready())) return false;
    await then();
    return true;
  }

  /** Goes on with `go` once nothing is unsaved; when the save fails the dialog asks first. */
  async leave(go: () => void): Promise<void> {
    if (await this.ready()) return go();
    this.asking = go;
  }

  /** The dialog's "もう一度保存する". */
  async saveAgain(): Promise<void> {
    if (!(await this.now())) return;
    this.proceed();
  }

  /** The dialog's "変更を捨てて進む": the edits stay out of the YAML. */
  discard(): void {
    this.cancel();
    this.proceed();
  }

  /** The dialog's "やめる". */
  stay(): void {
    this.asking = null;
  }

  private isBlocked(): boolean {
    return this.deps.blocked?.() ?? false;
  }

  cancel(): void {
    if (this.timer !== null) this.timers.clear(this.timer);
    this.timer = null;
  }

  private proceed(): void {
    const go = this.asking;
    this.asking = null;
    go?.();
  }
}
