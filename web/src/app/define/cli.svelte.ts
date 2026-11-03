// The link to `dagon serve` (ui-design §8, dev-flow §5): the passphrase from
// the URL fragment kept in sessionStorage, API calls with it as a Bearer
// token, and the event WebSocket for job progress, logs, results, questions
// and the hub hand-over. Keys never come here: hello reports only whether
// a key exists and its id. Paths are never sent (serve takes them from
// dagon.toml) except where to open or make a project, chosen from serve's own
// GET /v1/fs list (§8.2).

export type CliState = 'none' | 'connecting' | 'ready' | 'stopped' | 'denied';

export interface Hello {
  cli: string;
  workspace: string | null;
  features: string[];
  api_key: boolean;
  key: boolean;
  key_id: string | null;
  /** The developer's public key (64 hex digits) the page verifies releases with. */
  release_key?: string;
  /** MCU families POST /v1/flash can write (rp2040, stm32f401re). */
  flash_mcus?: string[];
  /** `dagon demo`: a simulated desk, never a real hub (its speed, and the 子機 set to fail). */
  sim?: { speed: number; fail: string | null; uplink?: string; max_payload?: number; reconnect?: 'reopen' | 'stays' } | null;
}

/** A project serve knows: its folder and its name. */
export interface ProjectRef {
  path: string;
  name: string;
}

/** GET /v1/project: whether a project is open, and which. */
export interface ProjectInfo {
  open: boolean;
  path?: string;
  name?: string;
  /** Recent projects, when serve keeps them (up to 5, newest first). */
  recent?: ProjectRef[];
}

/** One folder of GET /v1/fs; `project` when it holds a dagon.toml. */
export interface FsFolder {
  name: string;
  project: boolean;
  /** A 構成 YAML but no dagon.toml: opening it asks to make dagon.toml there. */
  yaml?: boolean;
}

/** GET /v1/fs: the folders of `path` (under home only; key folders left out). */
export interface FsList {
  path: string;
  /** null at the home folder. */
  parent: string | null;
  folders: FsFolder[];
}

/**
 * How opening or making a project went (POST /v1/project/open, /new):
 * `missing` no dagon.toml (404), `key` its key is unusable (422), `busy` a
 * job runs (409), `exists` the folder is not empty (409 on new), `refused`
 * outside home or a key folder (403), `failed` anything else.
 */
/** `yaml`: the folder has a 構成 but no dagon.toml (serve's 428); POST /v1/project/init makes it. */
export type ProjectResult = 'opened' | 'missing' | 'key' | 'busy' | 'exists' | 'refused' | 'yaml' | 'failed';

/** GET/PUT /v1/layout: what the YAML does not hold, in a file beside it. */
export interface Layout {
  /** 呼び名 by node id. */
  names?: Record<string, string>;
  /** Editor positions by node id. */
  positions?: Record<string, { x: number; y: number }>;
}

/** How saving the 構成 to the project's YAML went. */
export type SaveResult = 'saved' | 'invalid' | 'demoLink' | 'busy' | 'failed';

export type JobKind = 'build' | 'sign' | 'release' | 'gen' | 'flash' | 'conform' | 'ai_topology' | string;
export type JobState = 'queued' | 'running' | 'question' | 'succeeded' | 'failed' | 'cancelled' | 'unknown';

export interface Job {
  id: number;
  kind: JobKind;
  state: JobState;
  /** What it was started for (node ids, or the release name). */
  target: string;
  stage: string;
  percent: number | null;
  lines: string[];
  result: Record<string, unknown> | null;
  question: { id: string; text: string } | null;
  started: number;
  ended: number | null;
}

/** Log lines kept per job (SC-D7: the latest 2000). */
const MAX_LINES = 2000;
const PASS_KEY = 'dagon-serve-pass';
const PASS = /^[0-9a-f]{32}$/;

export interface CliDeps {
  fetch?: typeof fetch;
  socket?: (url: string) => WebSocket;
  storage?: Pick<Storage, 'getItem' | 'setItem'>;
  location?: Pick<Location, 'hash' | 'origin' | 'host' | 'protocol' | 'pathname' | 'search'>;
  history?: Pick<History, 'replaceState'>;
  now?: () => number;
}

export class Cli {
  state = $state<CliState>('none');
  hello = $state<Hello | null>(null);
  jobs = $state<Job[]>([]);
  /** CLI asked for the hub's port (device.need) and has not returned it yet. */
  holdsHub = $state(false);

  private pass: string | null = null;
  private ws: WebSocket | null = null;
  private readonly d: Required<CliDeps>;
  /** Called when the CLI needs the hub (the page closes Web Serial first) and when it returns it. */
  onDevice: (need: boolean) => void | Promise<void> = () => undefined;

  constructor(deps: CliDeps = {}) {
    this.d = {
      fetch: deps.fetch ?? ((...a) => fetch(...a)),
      socket: deps.socket ?? ((u) => new WebSocket(u)),
      // Outside a browser (tests rendering the page) there is no serve to reach: an empty store keeps K0.
      storage: deps.storage ?? globalThis.sessionStorage ?? { getItem: () => null, setItem: () => undefined },
      location: deps.location ?? globalThis.location,
      history: deps.history ?? globalThis.history,
      now: deps.now ?? (() => Date.now()),
    };
  }

  /** Whether serve's flash can write a node of MCU family `mcu`. */
  canFlash(mcu: string): boolean {
    return this.has('flash') && (this.hello?.flash_mcus ?? []).includes(mcu);
  }

  /** Whether a feature of /v1/hello is there (gen, flash, build, ...). */
  has(feature: string): boolean {
    return this.state === 'ready' && (this.hello?.features.includes(feature) ?? false);
  }

  /**
   * Takes the passphrase from the URL fragment into sessionStorage (and out
   * of the URL), then asks /v1/hello and opens the event stream. A page not
   * served by `dagon serve` (no passphrase) stays K0.
   */
  async connect(): Promise<void> {
    const loc = this.d.location;
    if (!loc) return void (this.state = 'none');
    const frag = (loc.hash ?? '').replace(/^#/, '');
    if (PASS.test(frag)) {
      this.d.storage.setItem(PASS_KEY, frag);
      this.d.history.replaceState(null, '', `${loc.pathname}${loc.search}`);
    }
    this.pass = this.d.storage.getItem(PASS_KEY);
    if (!this.pass) return void (this.state = 'none');
    this.state = 'connecting';
    try {
      const r = await this.call('GET', 'hello');
      if (r.status === 401) return void (this.state = 'denied');
      this.hello = (await r.json()) as Hello;
    } catch {
      return void (this.state = 'stopped');
    }
    this.state = 'ready';
    this.openEvents();
  }

  /** The simulated desk of `dagon demo`, when this serve has one. */
  get sim(): Hello['sim'] {
    return this.state === 'ready' && this.hello?.sim ? this.hello.sim : null;
  }

  /** The uplink the demo simulates now (its 構成's): frame size and reboot behaviour. */
  simUplink(): { maxPayload: number; reconnect: 'reopen' | 'stays' } {
    const s = this.hello?.sim;
    return { maxPayload: s?.max_payload ?? 1088, reconnect: s?.reconnect ?? 'reopen' };
  }

  /** POST /v1/sim/write: bytes for the simulated hub. */
  async simWrite(bytes: Uint8Array): Promise<void> {
    const r = await this.d.fetch('/v1/sim/write', {
      method: 'POST',
      headers: { Authorization: `Bearer ${this.pass}`, 'Content-Type': 'application/octet-stream' },
      body: bytes.slice(),
    });
    if (!r.ok) throw new Error(String(r.status));
  }

  /** GET /v1/sim/read: what the simulated hub answered within `waitMs`. */
  async simRead(waitMs: number): Promise<Uint8Array> {
    const r = await this.call('GET', `sim/read?wait=${Math.round(waitMs)}`);
    if (!r.ok) throw new Error(String(r.status));
    return new Uint8Array(await r.arrayBuffer());
  }

  private call(method: string, path: string, body?: unknown): Promise<Response> {
    const headers: Record<string, string> = { Authorization: `Bearer ${this.pass}` };
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    return this.d.fetch(`/v1/${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  }

  private async json<T>(method: string, path: string, body?: unknown): Promise<T> {
    const r = await this.call(method, path, body);
    if (!r.ok) throw new Error(String(r.status));
    return (await r.json()) as T;
  }

  private openEvents(): void {
    const loc = this.d.location;
    const ws = this.d.socket(`${loc.protocol === 'https:' ? 'wss' : 'ws'}://${loc.host}/v1/events`);
    this.ws = ws;
    ws.onopen = () => ws.send(this.pass ?? '');
    ws.onmessage = (m) => this.event(JSON.parse(String(m.data)) as Record<string, unknown>);
    ws.onclose = () => {
      if (this.ws === ws) this.state = 'stopped';
    };
  }

  /** One serve event (ui-design §8.1's table). */
  event(e: Record<string, unknown>): void {
    const job = this.jobs.find((j) => j.id === e.job);
    switch (e.type) {
      case 'job.state':
        if (job) {
          job.state = e.state as JobState;
          if (['succeeded', 'failed', 'cancelled'].includes(job.state)) job.ended = this.d.now();
        }
        return;
      case 'job.progress':
        if (job) {
          job.stage = String(e.stage ?? '');
          job.percent = typeof e.percent === 'number' ? e.percent : null;
        }
        return;
      case 'job.log':
        if (job) job.lines = [...job.lines, ...((e.lines as string[]) ?? [])].slice(-MAX_LINES);
        return;
      case 'job.result':
        if (job) job.result = (e.data as Record<string, unknown>) ?? null;
        return;
      case 'job.question':
        if (job) {
          job.state = 'question';
          job.question = { id: String(e.id), text: String(e.text ?? '') };
        }
        return;
      case 'device.need':
        this.holdsHub = true;
        void Promise.resolve(this.onDevice(true)).then(() => this.call('POST', 'device', { state: 'released' }));
        return;
      case 'device.returned':
        this.holdsHub = false;
        void this.onDevice(false);
        return;
    }
  }

  private async start(kind: JobKind, path: string, body: unknown, target: string): Promise<Job | null> {
    const r = await this.json<{ job: number }>('POST', path, body);
    const job: Job = { id: r.job, kind, state: 'queued', target, stage: '', percent: null, lines: [], result: null, question: null, started: this.d.now(), ended: null };
    this.jobs = [job, ...this.jobs];
    return this.jobs[0];
  }

  /** GET /v1/topology: the project's 構成 YAML, or null when it has none yet. */
  async loadTopology(): Promise<string | null> {
    if (this.state !== 'ready') return null;
    const r = await this.call('GET', 'topology');
    return r.ok ? await r.text() : null;
  }

  /**
   * PUT /v1/topology: the 構成 YAML into the project's topology.yaml. The CLI
   * checks it too: `invalid` when it refuses the 構成 (422), `busy` while a
   * job holds the project (409), `failed` for anything else or no answer.
   * Shapes not updatable yet (V-23 only) are saved.
   */
  async saveTopology(yaml: string): Promise<SaveResult> {
    if (this.state !== 'ready') return 'failed';
    try {
      const r = await this.d.fetch('/v1/topology', { method: 'PUT', headers: { Authorization: `Bearer ${this.pass}`, 'Content-Type': 'application/yaml' }, body: yaml });
      if (r.ok) {
        // In dagon demo the 構成's uplink picks the simulated medium: hello says which now.
        if (this.hello?.sim) await this.refreshHello();
        return 'saved';
      }
      if (r.status === 422) {
        // dagon demo: an uplink it cannot simulate is not saved (the link shown is the one in use).
        const body = (await r.json().catch(() => null)) as { demo_uplink?: unknown } | null;
        return body?.demo_uplink ? 'demoLink' : 'invalid';
      }
      return r.status === 409 ? 'busy' : 'failed';
    } catch {
      return 'failed';
    }
  }

  gen(nodes: string[]): Promise<Job | null> {
    return this.start('gen', 'gen', { nodes }, nodes.join(' · '));
  }

  build(nodes: string[]): Promise<Job | null> {
    return this.start('build', 'build', { nodes }, nodes.join(' · '));
  }

  sign(nodes: string[]): Promise<Job | null> {
    return this.start('sign', 'sign', { nodes }, nodes.join(' · '));
  }

  release(version: string, name: string): Promise<Job | null> {
    return this.start('release', 'release', { version, name }, name);
  }

  flash(node: string): Promise<Job | null> {
    return this.start('flash', 'flash', { node }, node);
  }

  async answer(job: Job, text: string): Promise<void> {
    if (!job.question) return;
    await this.json('POST', `jobs/${job.id}/answer`, { answer: text });
    job.question = null;
    job.state = 'running';
  }

  async cancel(job: Job): Promise<void> {
    await this.call('POST', `jobs/${job.id}/cancel`);
  }

  /** GET /v1/release/{id}: the .dgb bytes. */
  async releaseBytes(id: string): Promise<Uint8Array> {
    const r = await this.call('GET', `release/${encodeURIComponent(id)}`);
    if (!r.ok) throw new Error(String(r.status));
    return new Uint8Array(await r.arrayBuffer());
  }

  /** GET /v1/release/latest: the project's latest .dgb ("最新のリリースを使う"); null when there is none. */
  async latestRelease(): Promise<Uint8Array | null> {
    if (this.state !== 'ready') return null;
    const r = await this.call('GET', 'release/latest');
    return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
  }

  /** GET /v1/bootcore/{node}: the node's signed boot core for the ROM path (feature `bootcore`); null when there is none. */
  async bootcore(node: string): Promise<Uint8Array | null> {
    if (!this.has('bootcore')) return null;
    const r = await this.call('GET', `bootcore/${encodeURIComponent(node)}`);
    return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
  }

  /** GET /v1/project (feature `project`): the open project, if any (SC-S0). */
  project(): Promise<ProjectInfo> {
    return this.json<ProjectInfo>('GET', 'project');
  }

  /** GET /v1/fs (feature `project`): the folders of `path` under the home folder; serve normalises and confines it. */
  fs(path?: string): Promise<FsList> {
    return this.json<FsList>('GET', path === undefined ? 'fs' : `fs?path=${encodeURIComponent(path)}`);
  }

  /** POST /v1/project/open {path}: a folder with dagon.toml; serve switches to its keys. */
  openProject(path: string): Promise<ProjectResult> {
    return this.projectCall('project/open', { path }, false);
  }

  /** POST /v1/project/new {path, from_sample}: `dagon new` in the new folder `path` (its name is the project's). */
  newProject(path: string, sample: boolean): Promise<ProjectResult> {
    return this.projectCall('project/new', { path, from_sample: sample }, true);
  }

  /** POST /v1/project/init {path}: dagon.toml for the folder's 構成 (after the page asked); serve switches to it. */
  initProject(path: string): Promise<ProjectResult> {
    return this.projectCall('project/init', { path }, true);
  }

  /** GET /v1/layout (feature `layout`): the project's 呼び名 and positions; null when there are none yet. */
  async loadLayout(): Promise<Layout | null> {
    if (!this.has('layout')) return null;
    const r = await this.call('GET', 'layout');
    return r.ok ? ((await r.json()) as Layout) : null;
  }

  /** PUT /v1/layout: the whole layout file; false when serve did not take it. */
  async saveLayout(layout: Layout): Promise<boolean> {
    if (!this.has('layout')) return false;
    const r = await this.call('PUT', 'layout', layout).catch(() => null);
    return r?.ok ?? false;
  }

  private async projectCall(path: string, body: unknown, making: boolean): Promise<ProjectResult> {
    let r: Response;
    try {
      r = await this.call('POST', path, body);
    } catch {
      return 'failed';
    }
    if (r.ok) {
      // The project's keys replace the last one's: hello says which (release_key, key_id, workspace).
      await this.refreshHello();
      return 'opened';
    }
    if (r.status === 404) return 'missing';
    if (r.status === 428) return 'yaml';
    if (r.status === 422) return 'key';
    if (r.status === 403) return 'refused';
    if (r.status !== 409) return 'failed';
    // 409 is a running job, or (making) a folder that is not empty.
    const msg = String(((await r.json().catch(() => ({}))) as { error?: string }).error ?? '');
    return making && !/job/.test(msg) ? 'exists' : 'busy';
  }

  /** GET /v1/hello again (after switching projects). */
  async refreshHello(): Promise<void> {
    const r = await this.call('GET', 'hello').catch(() => null);
    if (r?.ok) this.hello = (await r.json()) as Hello;
  }

  /** "最初からやり直す": the jobs this page shows are its records too (serve's files stay). */
  clearJobs(): void {
    this.jobs = [];
  }

  /** GET/PUT /v1/nodes/{id}/board (dev-flow §5.1), when serve offers it. */
  saveBoard(id: string, fields: { flash_bytes?: number }): Promise<unknown> {
    return this.json('PUT', `nodes/${encodeURIComponent(id)}/board`, fields);
  }
}
