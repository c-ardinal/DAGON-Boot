// The link to `dagon serve` (ui-design §8, dev-flow §5): the passphrase from
// the URL fragment kept in sessionStorage, API calls with it as a Bearer
// token, and the event WebSocket for job progress, logs, results, questions
// and the hub hand-over. Keys never come here: hello reports only whether
// a key exists and its id. Paths are never sent; serve takes them from
// dagon.toml.

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
}

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

  /** PUT /v1/topology: the 構成 YAML into the project's topology.yaml; the CLI checks it too (422 when not pushable). */
  async saveTopology(yaml: string): Promise<void> {
    const r = await this.d.fetch('/v1/topology', { method: 'PUT', headers: { Authorization: `Bearer ${this.pass}`, 'Content-Type': 'application/yaml' }, body: yaml });
    if (!r.ok) throw new Error(String(r.status));
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

  /** GET/PUT /v1/nodes/{id}/board (dev-flow §5.1), when serve offers it. */
  saveBoard(id: string, fields: { flash_bytes?: number }): Promise<unknown> {
    return this.json('PUT', `nodes/${encodeURIComponent(id)}/board`, fields);
  }
}
