// SC-S0 始め (ui-design §7.2b): make a project, make one from the sample, or
// open one, in a folder chosen from dagon serve's own list (GET /v1/fs). The
// page sends only a path built from that list; serve confines it to the home
// folder and refuses key folders (§8.2).
import type { Cli, FsList, ProjectRef, ProjectResult } from './define/cli.svelte.ts';

export type StartChoice = 'new' | 'sample' | 'open';

/** Why the last open or make did not go through (the band's chip and sentence). */
export type StartFailure = Exclude<ProjectResult, 'opened'>;

export interface StartDeps {
  cli: Cli;
  /** serve has switched to a project: the page reads its 構成 and leaves SC-S0. */
  opened: () => Promise<void>;
  /** Without serve's project feature (development stand-ins): start an unsaved 構成 here; null where that is not allowed. */
  local: ((c: 'new' | 'sample') => void) | null;
}

const SAMPLE_NAME = 'desk-demo';

/** `name` inside folder `dir` as serve shows paths (forward slashes). */
export const inside = (dir: string, name: string): string => `${dir.replace(/\/+$/, '')}/${name}`;

export class Start {
  choice = $state<StartChoice | null>(null);
  list = $state<FsList | null>(null);
  /** Reading serve (the list, the project): the list place shows a skeleton. */
  loading = $state(true);
  /** The new project's folder name (new, sample). */
  name = $state('');
  failed = $state<StartFailure | null>(null);
  /** Whether the failure came from making (M35) or opening. */
  making = $state(false);
  recent = $state<ProjectRef[]>([]);

  constructor(private readonly deps: StartDeps) {}

  /** serve can list folders and open or make projects. */
  get online(): boolean {
    return this.deps.cli.has('project');
  }

  /** The choices can be used: through serve, or here without it where allowed. */
  usable(c: StartChoice): boolean {
    return this.online || (c !== 'open' && this.deps.local !== null);
  }

  async refresh(): Promise<void> {
    if (!this.online) return;
    try {
      this.recent = ((await this.deps.cli.project()).recent ?? []).slice(0, 5);
    } catch {
      // The recent list is a convenience; the choices still work.
    }
  }

  async choose(c: StartChoice): Promise<void> {
    if (!this.usable(c)) return;
    this.choice = c;
    this.failed = null;
    this.name = c === 'sample' ? SAMPLE_NAME : '';
    if (this.online) return this.browse(this.list?.path);
    if (c !== 'open') this.deps.local?.(c);
  }

  /** Into folder `path` (serve's path), or the home folder. */
  async browse(path?: string): Promise<void> {
    this.loading = true;
    try {
      this.list = await this.deps.cli.fs(path);
    } catch {
      this.making = this.choice !== 'open';
      this.failed = 'refused';
    } finally {
      this.loading = false;
    }
  }

  /** "新しく作る" / "見本から新しく作る": a new folder `name` in the folder on the list. */
  async make(): Promise<void> {
    const c = this.choice;
    const name = this.name.trim();
    if (!this.list || !name || (c !== 'new' && c !== 'sample')) return;
    await this.result(await this.deps.cli.newProject(inside(this.list.path, name), c === 'sample'), true);
  }

  /** "開く" on a project folder (from the list, or a recent one). */
  async open(path: string): Promise<void> {
    await this.result(await this.deps.cli.openProject(path), false);
  }

  private async result(r: ProjectResult, making: boolean): Promise<void> {
    this.making = making;
    this.failed = r === 'opened' ? null : r;
    if (r === 'opened') await this.deps.opened();
  }

  clear(): void {
    this.choice = null;
    this.list = null;
    this.failed = null;
    this.loading = false;
  }
}
