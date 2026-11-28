// The link to `dagon serve`: the passphrase from
// the URL fragment kept in sessionStorage, API calls with it as a Bearer
// token, and the event WebSocket for job progress, logs, results, questions
// and the coordinator hand-over. Keys never come here: hello reports only whether
// a key exists and its id. Paths are never sent (serve takes them from
// dagon.toml) except where to open or make a project, chosen from serve's own
// GET /v1/fs list.

import type { Kept } from '../kept.ts';
import { request, type RequestOptions, type Route } from '../../request.ts';
import pkg from '../../../package.json';

export type CliState = 'none' | 'connecting' | 'ready' | 'stopped' | 'denied';

/** The `dagon serve` API number this Console speaks (GET /v1/hello `api`; site/src/content/docs/explanation/versioning.md). */
export const SERVE_API = 1;
/** This Console's own version: the cli component's (web/package.json). */
export const CONSOLE_VERSION: string = pkg.version;

export interface Hello {
  cli: string;
  /** Serve API number; missing from a dagon older than numbered APIs. */
  api?: number;
  /** Every format number the dagon reads and writes (`wire`, `image-header`, ...). */
  formats?: Record<string, number>;
  workspace: string | null;
  features: string[];
  key: boolean;
  key_id: string | null;
  /** The developer's public key (64 hex digits) the page verifies releases with. */
  release_key?: string;
  /** Nodes with a [build.*] in dagon.toml: the others have no skeleton yet. */
  build_nodes?: string[];
  /** MCU families POST /v1/flash can write (rp2040, stm32f401re). */
  flash_mcus?: string[];
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
  /** The file's format number. */
  format?: number;
  /** 表示名 by node id. */
  names?: Record<string, string>;
  /** Editor positions by node id. */
  positions?: Record<string, { x: number; y: number }>;
}

/** The layout format this page reads and writes (GET /v1/hello `formats.layout`). */
export const LAYOUT_FORMAT = 1;

/** A layout file as read: its shape checked, or why it is refused. Coordinates are finite numbers or the file is not taken. */
export function parseLayout(v: unknown): { layout: Layout } | { refused: string } {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return { refused: 'not an object' };
  const { format, names, positions } = v as Record<string, unknown>;
  if (format !== undefined && format !== LAYOUT_FORMAT) return { refused: `format ${String(format)}` };
  const isMap = (x: unknown): x is Record<string, unknown> => typeof x === 'object' && x !== null && !Array.isArray(x);
  if (names !== undefined && !(isMap(names) && Object.values(names).every((n) => typeof n === 'string'))) return { refused: 'names' };
  if (positions !== undefined) {
    const point = (p: unknown) => isMap(p) && Number.isFinite(p.x) && Number.isFinite(p.y);
    if (!(isMap(positions) && Object.values(positions).every(point))) return { refused: 'positions' };
  }
  return { layout: { format: LAYOUT_FORMAT, names: names as Layout['names'], positions: positions as Layout['positions'] } };
}

/** How saving the 構成 to the project's YAML went. */
export type SaveResult = 'saved' | 'invalid' | 'busy' | 'failed';

export type JobKind = 'build' | 'sign' | 'release' | 'gen' | 'flash' | 'conform' | 'ai_topology' | 'ai_port' | string;

/** POST /v1/ai/port: the board the agent ports DAGON to (`dagon ai port`). */
export interface PortRequest {
  mcu: string;
  board: string;
  /** `S`: DAGON's own boot stages; `V`: on the vendor's second stage. */
  kind: 'S' | 'V';
  /** What the user says of the board (its parts, its pins); left out when empty. */
  description?: string;
}

/** An id as serve takes it for an MCU or a board (`dagon ai port`: letters, digits, `-`, `_`, `.`, up to 32, no `.` first). */
export const PORT_ID = /^(?!\.)[A-Za-z0-9._-]{1,32}$/;
/** The longest board description serve takes, in UTF-8 bytes. */
export const PORT_DESCRIPTION_MAX = 32 * 1024;
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

/** Why a job stopped: serve's error in its result, else its last log line. */
export const jobReason = (j: Job | null | undefined): string => {
  const e = (j?.result as { error?: unknown } | null | undefined)?.error;
  return e !== undefined && e !== null ? String(e) : (j?.lines[j.lines.length - 1] ?? '');
};

/** Log lines kept per job (作業: the latest 2000). */
const MAX_LINES = 2000;
const PASS_KEY = 'dagon-serve-pass';
const PASS = /^[0-9a-f]{32}$/;

/** What goes with a route: its `:name` values, its query, and a body sent as JSON. */
export type Call = Pick<RequestOptions, 'params' | 'query'> & { body?: unknown };

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
  /** Events about a job the page has not yet been told the number of. */
  private early: Record<string, unknown>[] = [];
  /** Why the project's layout file was not taken, or null. */
  layoutRefused = $state<string | null>(null);
  /** The open project's definitions did not load (GET /v1/definitions 500): the builtin set is not used in their place. */
  defsFailed = $state(false);
  jobs = $state<Job[]>([]);
  /** CLI asked for the coordinator's port (device.need) and has not returned it yet. */
  holdsHub = $state(false);

  private pass: string | null = null;
  private ws: WebSocket | null = null;
  private readonly d: Required<Omit<CliDeps, 'fetch'>> & Pick<CliDeps, 'fetch'>;
  /**
   * The lock table's check, set by the page: true when the operation is stopped now
   * (the page has shown why). Every job asks it in `start`.
   */
  guard: (kind: 'gen' | 'build' | 'release' | 'flash') => boolean = () => false;
  /** Called when the CLI needs the coordinator (the page closes Web Serial first) and when it returns it. */
  onDevice: (need: boolean) => void | Promise<void> = () => undefined;

  constructor(deps: CliDeps = {}) {
    this.d = {
      fetch: deps.fetch,
      socket: deps.socket ?? ((u) => new WebSocket(u)),
      // Outside a browser (tests rendering the page) there is no serve to reach: an empty store keeps the state `none`.
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

  /** Whether the connected dagon serves the API this Console speaks. */
  get apiMatches(): boolean {
    return this.hello?.api === SERVE_API;
  }

  /** Whether a feature of /v1/hello is there (gen, flash, build, ...). */
  has(feature: string): boolean {
    return this.state === 'ready' && (this.hello?.features.includes(feature) ?? false);
  }

  /**
   * Takes the passphrase from the URL fragment into sessionStorage (and out
   * of the URL), then asks /v1/hello and opens the event stream. A page not
   * served by `dagon serve` (no passphrase) stays `none`.
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
      const r = await this.call('GET /v1/hello');
      if (r.status === 401) return void (this.state = 'denied');
      this.hello = (await r.json()) as Hello;
    } catch {
      return void (this.state = 'stopped');
    }
    this.state = 'ready';
    this.openEvents();
  }

  /** A /v1/ call with the passphrase (the DAGON agent routes: /v1/agent, /v1/agent/link, /v1/agent/ask). */
  api(route: Route, o: Call = {}): Promise<Response> {
    return this.call(route, o);
  }

  private call(route: Route, { body, ...where }: Call = {}): Promise<Response> {
    return body === undefined ? this.send(route, where) : this.send(route, { ...where, body: JSON.stringify(body) }, 'application/json');
  }

  /** `route` with the passphrase; `type` says what a body is. */
  private send(route: Route, o: RequestOptions, type?: string): Promise<Response> {
    const headers: Record<string, string> = { Authorization: `Bearer ${this.pass}` };
    if (type !== undefined) headers['Content-Type'] = type;
    return request(route, { ...o, headers, via: this.d.fetch });
  }

  private async json<T>(route: Route, o: Call = {}): Promise<T> {
    const r = await this.call(route, o);
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

  /** One serve event. */
  event(e: Record<string, unknown>): void {
    const job = this.jobs.find((j) => j.id === e.job);
    // serve starts a job before the page has its answer: what it says meanwhile waits for the job.
    if (!job && typeof e.job === 'number' && String(e.type).startsWith('job.')) {
      this.early.push(e);
      this.early.splice(0, this.early.length - MAX_LINES);
      return;
    }
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
        void Promise.resolve(this.onDevice(true)).then(() => this.call('POST /v1/device', { body: { state: 'released' } }));
        return;
      case 'device.returned':
        this.holdsHub = false;
        void this.onDevice(false);
        return;
    }
  }

  private async start(kind: JobKind, route: Route, body: unknown, target: string): Promise<Job | null> {
    const lock = ({ gen: 'gen', build: 'build', sign: 'build', release: 'release', flash: 'flash' } as const)[kind as 'gen'];
    if (lock && this.guard(lock)) return null;
    const r = await this.json<{ job: number }>(route, { body });
    return this.track(kind, r.job, target);
  }

  /** Lists the job serve has taken as `id`, with what serve said of it before its number was known here. */
  private track(kind: JobKind, id: number, target: string): Job {
    const job: Job = { id, kind, state: 'queued', target, stage: '', percent: null, lines: [], result: null, question: null, started: this.d.now(), ended: null };
    this.jobs = [job, ...this.jobs];
    const said = this.early.filter((e) => e.job === id);
    this.early = this.early.filter((e) => e.job !== id);
    said.forEach((e) => this.event(e));
    return this.jobs[0];
  }

  /** GET /v1/topology: the project's 構成 YAML, or null when it has none yet. */
  async loadTopology(): Promise<string | null> {
    if (this.state !== 'ready') return null;
    const r = await this.call('GET /v1/topology');
    // serve says 404 for a project with no 構成 yet; anything else is a file not read, not an empty one.
    if (r.status === 404) return null;
    if (!r.ok) throw new Error(`GET topology: ${r.status}`);
    return r.text();
  }

  /**
   * PUT /v1/topology: the 構成 YAML into the project's topology.yaml. The CLI
   * checks it too: `invalid` when it refuses the 構成 (422), `busy` while a
   * job holds the project (409), `failed` for anything else or no answer.
   * Parts updating skips (V-23, information only) are saved.
   */
  async saveTopology(yaml: string): Promise<SaveResult> {
    if (this.state !== 'ready') return 'failed';
    try {
      const r = await this.send('PUT /v1/topology', { body: yaml }, 'application/yaml');
      if (r.ok) return 'saved';
      if (r.status === 422) return 'invalid';
      return r.status === 409 ? 'busy' : 'failed';
    } catch {
      return 'failed';
    }
  }

  gen(nodes: string[]): Promise<Job | null> {
    return this.start('gen', 'POST /v1/gen', { nodes }, nodes.join(' · '));
  }

  build(nodes: string[]): Promise<Job | null> {
    return this.start('build', 'POST /v1/build', { nodes }, nodes.join(' · '));
  }

  sign(nodes: string[]): Promise<Job | null> {
    return this.start('sign', 'POST /v1/sign', { nodes }, nodes.join(' · '));
  }

  /** POST /v1/release: the signed images into a package; without a version each keeps its crate's, without a name serve names it by the newest. */
  release(version?: string, name?: string): Promise<Job | null> {
    return this.start('release', 'POST /v1/release', { ...(version ? { version } : {}), ...(name ? { name } : {}) }, name ?? '');
  }

  flash(node: string): Promise<Job | null> {
    return this.start('flash', 'POST /v1/flash', { node }, node);
  }

  /**
   * POST /v1/ai/port: the agent ports DAGON to a board in the project's DAGON
   * clone. The agent edits files and runs commands there without asking, so
   * serve takes it only with `accept`, which the caller sends once the user
   * has agreed to that. `refused` is serve's reason when it does not take it.
   */
  async aiPort(req: PortRequest): Promise<Job | { refused: string }> {
    const description = req.description?.trim();
    const body = { mcu: req.mcu, board: req.board, kind: req.kind, ...(description ? { description } : {}), accept: true };
    const r = await this.call('POST /v1/ai/port', { body });
    if (!r.ok) {
      const said = (await r.json().catch(() => null)) as { error?: unknown } | null;
      return { refused: typeof said?.error === 'string' ? said.error : String(r.status) };
    }
    return this.track('ai_port', ((await r.json()) as { job: number }).job, `${req.mcu} · ${req.board}`);
  }

  async answer(job: Job, text: string): Promise<void> {
    if (!job.question) return;
    await this.json('POST /v1/jobs/:id/answer', { params: { id: job.id }, body: { answer: text } });
    job.question = null;
    job.state = 'running';
  }

  async cancel(job: Job): Promise<void> {
    await this.call('POST /v1/jobs/:id/cancel', { params: { id: job.id } });
  }

  /** GET /v1/release/{id}: the .dgb bytes. */
  async releaseBytes(id: string): Promise<Uint8Array> {
    const r = await this.call('GET /v1/release/:id', { params: { id } });
    if (!r.ok) throw new Error(String(r.status));
    return new Uint8Array(await r.arrayBuffer());
  }

  /** GET /v1/release/latest: the project's latest .dgb ("最新リリースの使用"); null when there is none. */
  async latestRelease(): Promise<Uint8Array | null> {
    if (this.state !== 'ready') return null;
    const r = await this.call('GET /v1/release/latest');
    return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
  }

  /** GET /v1/kept (feature `kept`): the packages kept in the project, newest version first. */
  async keptList(): Promise<Kept[]> {
    if (!this.has('kept')) return [];
    const r = await this.call('GET /v1/kept');
    return r.ok ? (((await r.json()) as { kept?: Kept[] }).kept ?? []) : [];
  }

  /** GET /v1/kept/{id}: a kept package's bytes; null when it is not there. */
  async keptBytes(id: string): Promise<Uint8Array | null> {
    if (!this.has('kept')) return null;
    const r = await this.call('GET /v1/kept/:id', { params: { id } });
    return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
  }

  /** POST /v1/kept: keeps a package about to be written; its id, or null when it was not kept. */
  async keep(bytes: Uint8Array): Promise<string | null> {
    if (!this.has('kept')) return null;
    try {
      const r = await this.send('POST /v1/kept', { body: bytes as BodyInit }, 'application/octet-stream');
      return r.ok ? (((await r.json()) as { id?: string }).id ?? null) : null;
    } catch {
      return null;
    }
  }

  /** GET /v1/bootcore/{node}: the node's signed boot core for the ROM path (feature `bootcore`); null when there is none. */
  async bootcore(node: string): Promise<Uint8Array | null> {
    if (!this.has('bootcore')) return null;
    const r = await this.call('GET /v1/bootcore/:node', { params: { node } });
    return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
  }

  /** GET /v1/stage0/{node}: stage0's bytes for the ROM path's full rewrite of an MCU with a stage0 (feature `bootcore`); null when there are none. */
  async stage0(node: string): Promise<Uint8Array | null> {
    if (!this.has('bootcore')) return null;
    const r = await this.call('GET /v1/stage0/:node', { params: { node } });
    return r.ok ? new Uint8Array(await r.arrayBuffer()) : null;
  }

  /** GET /v1/project (feature `project`): the open project, if any. */
  project(): Promise<ProjectInfo> {
    return this.json<ProjectInfo>('GET /v1/project');
  }

  /** GET /v1/fs (feature `project`): the folders of `path` under the home folder; serve normalises and confines it. */
  fs(path?: string): Promise<FsList> {
    return this.json<FsList>('GET /v1/fs', path === undefined ? {} : { query: { path } });
  }

  /** POST /v1/project/open {path}: a folder with dagon.toml; serve switches to its keys. */
  openProject(path: string): Promise<ProjectResult> {
    return this.projectCall('POST /v1/project/open', { path }, false);
  }

  /** POST /v1/project/new {path, from_sample}: `dagon new` in the new folder `path` (its name is the project's). */
  newProject(path: string, sample: boolean): Promise<ProjectResult> {
    return this.projectCall('POST /v1/project/new', { path, from_sample: sample }, true);
  }

  /** POST /v1/project/init {path}: dagon.toml for the folder's 構成 (after the page asked); serve switches to it. */
  initProject(path: string): Promise<ProjectResult> {
    return this.projectCall('POST /v1/project/init', { path }, true);
  }

  /** GET /v1/layout (feature `layout`): the project's 表示名 and positions; null when there are none yet. */
  async loadLayout(): Promise<Layout | null> {
    if (!this.has('layout')) return null;
    const r = await this.call('GET /v1/layout');
    if (!r.ok) {
      this.layoutRefused = ((await r.json().catch(() => ({}))) as { error?: string }).error ?? String(r.status);
      return null;
    }
    const read = parseLayout(await r.json());
    if ('refused' in read) {
      this.layoutRefused = read.refused;
      return null;
    }
    this.layoutRefused = null;
    return read.layout;
  }

  /** PUT /v1/layout: the whole layout file; false when serve did not take it. */
  async saveLayout(layout: Layout): Promise<boolean> {
    if (!this.has('layout')) return false;
    const r = await this.call('PUT /v1/layout', { body: layout }).catch(() => null);
    return r?.ok ?? false;
  }

  private async projectCall(route: Route, body: unknown, making: boolean): Promise<ProjectResult> {
    let r: Response;
    try {
      r = await this.call(route, { body });
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

  /** GET /v1/definitions: the project's MCU and board definitions (JSON), or null without them. */
  async definitions(): Promise<string | null> {
    const r = await this.call('GET /v1/definitions').catch(() => null);
    this.defsFailed = r?.status === 500;
    return r?.ok ? await r.text() : null;
  }

  /** GET /v1/hello again (after switching projects). */
  async refreshHello(): Promise<void> {
    const r = await this.call('GET /v1/hello').catch(() => null);
    if (r?.ok) this.hello = (await r.json()) as Hello;
  }

  /** "リスタート": the jobs this page shows are its records too (serve's files stay). */
  clearJobs(): void {
    this.jobs = [];
  }
}
