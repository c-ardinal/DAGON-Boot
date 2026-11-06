// What ビルドする and the first write of a node show, from serve's jobs.
import { jobReason, type Job } from './define/cli.svelte.ts';
import { NODE_WRITE, type L } from './machine.ts';

/** The jobs of one ビルドする: build every node, sign them, make the package 書込 offers first. */
export interface BuildJobs {
  build: Job | null;
  sign: Job | null;
  release: Job | null;
}

export type BuildState = 'running' | 'done' | 'failed' | null;

/**
 * Where ビルドする stands. Only the made package is done (a built app with
 * nothing signed gives 書込 nothing to write); `startFailed`: serve would not
 * take one of the jobs. A cancelled job ends it without a notice: the user did it.
 */
export function buildStateOf(j: BuildJobs, startFailed: boolean): BuildState {
  if (startFailed) return 'failed';
  const jobs = [j.build, j.sign, j.release].filter((x) => x !== null);
  if (jobs.length === 0) return null;
  if (jobs.some((x) => x.state === 'failed')) return 'failed';
  if (jobs.some((x) => x.state === 'cancelled')) return null;
  return j.release?.state === 'succeeded' ? 'done' : 'running';
}

/** `dagon build`, and the boot core built while signing, name the node at the start of each build. */
const BUILDING = /^(\S+): building /;

const lastBuilding = (lines: string[], reach: readonly string[]): string | null => {
  for (let i = lines.length - 1; i >= 0; i--) {
    const id = BUILDING.exec(lines[i])?.[1];
    if (id) return reach.includes(id) ? id : null;
  }
  return null;
};

/** The node being built now, for its card; null when nothing runs. */
export function buildingNode(j: BuildJobs, reach: readonly string[]): string | null {
  const running = [j.sign, j.build].find((x) => x?.state === 'running');
  return running ? lastBuilding(running.lines, reach) : null;
}

export interface BuildFailure {
  /** The node it stopped at; null when no node's build failed. */
  node: string | null;
  reason: string;
  /** Where the compiler points, when it does. */
  file: string | null;
  line: number | null;
  /** Nodes built before it stopped. */
  others: number;
}

/** cargo's first error and the place under it (dagon build stops at the first node that fails). */
const CARGO_ERROR = /^error(?:\[[^\]]+\])?: (?!could not compile)(.+)$/;
const CARGO_PLACE = /^\s*-->\s*(.+):(\d+):\d+\s*$/;

/** What the failure dialog says about the failed step. */
export function buildFailure(j: BuildJobs, reach: readonly string[]): BuildFailure {
  const failed = [j.build, j.sign, j.release].find((x) => x?.state === 'failed') ?? null;
  const plain = { node: null, reason: jobReason(failed), file: null, line: null, others: 0 };
  if (failed && failed === j.sign) {
    const named = /^(\S+): (.*)$/s.exec(plain.reason ?? '');
    return named && reach.includes(named[1]) ? { ...plain, node: named[1], reason: named[2] } : plain;
  }
  if (!failed || failed !== j.build) return plain;
  const lines = failed.lines;
  const at = lines.findIndex((l, i) => CARGO_ERROR.test(l) && CARGO_PLACE.test(lines[i + 1] ?? ''));
  const node = lastBuilding(lines, reach);
  if (at < 0 || !node) return plain;
  const [, path, line] = CARGO_PLACE.exec(lines[at + 1])!;
  const shown = path.replace(/\\/g, '/');
  const built = new Set(lines.flatMap((l) => BUILDING.exec(l)?.[1] ?? []));
  built.delete(node);
  return { node, reason: `${shown}:${line}  ${CARGO_ERROR.exec(lines[at])![1]}`, file: shown.split('/').pop()!, line: Number(line), others: built.size };
}

/**
 * Nodes whose first write can be started from their card: writing can reach
 * them, and their DAGON does not answer (nothing answers before connecting).
 */
export function firstWriteNodes(
  nodes: readonly { id: string; notYet?: string | null }[],
  live: Readonly<Record<string, { responding: boolean | null } | undefined>>,
  connected: boolean,
  state: L,
): string[] {
  if (connected && !NODE_WRITE.has(state)) return [];
  return nodes.filter((n) => !n.notYet && (!connected || live[n.id]?.responding !== true)).map((n) => n.id);
}
