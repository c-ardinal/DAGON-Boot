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
  /** A folder with a 構成 but no dagon.toml: the band asks whether to make it there. */
  asking = $state<string | null>(null);

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

  /** "開く" on a project folder, or one with only a 構成 (from the list, or a recent one). */
  async open(path: string): Promise<void> {
    this.asking = null;
    const r = await this.deps.cli.openProject(path);
    if (r === 'yaml') {
      this.failed = null;
      this.asking = path;
      return;
    }
    await this.result(r, false);
  }

  /** The answer to "このフォルダに dagon.toml を作りますか。": yes makes it and opens the project. */
  async init(yes: boolean): Promise<void> {
    const path = this.asking;
    this.asking = null;
    if (!yes || path === null) return;
    await this.result(await this.deps.cli.initProject(path), true);
  }

  private async result(r: ProjectResult, making: boolean): Promise<void> {
    this.making = making;
    this.failed = r === 'opened' || r === 'yaml' ? null : r;
    if (r === 'opened') await this.deps.opened();
  }

  clear(): void {
    this.asking = null;
    this.choice = null;
    this.list = null;
    this.failed = null;
    this.loading = false;
  }
}
