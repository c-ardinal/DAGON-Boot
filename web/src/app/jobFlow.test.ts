import { describe, expect, test } from 'vitest';
import type { Job } from './define/cli.svelte.ts';
import { buildFailure, buildingNode, buildingNow, buildStateOf, firstWriteNodes} from './jobFlow.ts';

const job = (p: Partial<Job>): Job => ({ id: 1, kind: 'build', state: 'running', target: '', stage: '', percent: null, lines: [], result: null, question: null, started: 0, ended: null, ...p });
const none = { build: null, sign: null, release: null };
const reach = ['hub', 'seg', 'watch'];

describe('where ビルドする stands', () => {
  test('nothing started: no state', () => {
    expect(buildStateOf(none, false)).toBeNull();
  });

  test('running until the package is made; signing counts', () => {
    expect(buildStateOf({ ...none, build: job({}) }, false)).toBe('running');
    expect(buildStateOf({ ...none, build: job({ state: 'succeeded' }) }, false)).toBe('running');
    expect(buildStateOf({ ...none, build: job({ state: 'succeeded' }), sign: job({ state: 'succeeded' }) }, false)).toBe('running');
    expect(buildStateOf({ build: job({ state: 'succeeded' }), sign: job({ state: 'succeeded' }), release: job({ state: 'succeeded' }) }, false)).toBe('done');
  });

  test('a built app with nothing signed is not done: no package exists to write', () => {
    expect(buildStateOf({ ...none, build: job({ state: 'succeeded' }) }, false)).not.toBe('done');
  });

  test('any step failing, or one that could not start, is a failure', () => {
    expect(buildStateOf({ ...none, build: job({ state: 'failed' }) }, false)).toBe('failed');
    expect(buildStateOf({ build: job({ state: 'succeeded' }), sign: job({ state: 'succeeded' }), release: job({ state: 'failed' }) }, false)).toBe('failed');
    expect(buildStateOf(none, true)).toBe('failed');
  });

  test('a cancelled step ends it: the button is free again and nothing is announced', () => {
    expect(buildStateOf({ ...none, build: job({ state: 'cancelled' }) }, false)).toBeNull();
    expect(buildStateOf({ ...none, build: job({ state: 'succeeded' }), sign: job({ state: 'cancelled' }) }, false)).toBeNull();
  });
});

describe('which card says ビルド中', () => {
  test('the node cargo builds now, nothing else', () => {
    const lines = ['seg: building slot a in nodes/seg', '   Compiling x v1', 'seg: slot a -> out', 'hub: building slot a in nodes/hub', '   Compiling y v1'];
    expect(buildingNode({ ...none, build: job({ lines }) }, reach)).toBe('hub');
  });

  test('a boot core built while signing shows on its card too', () => {
    const sign = job({ kind: 'sign', lines: ['watch: building the boot core for slot a'] });
    expect(buildingNode({ ...none, build: job({ state: 'succeeded', lines: ['hub: building slot a'] }), sign }, reach)).toBe('watch');
  });

  test('none once it is over, before the first line, or for an unknown id', () => {
    expect(buildingNode({ ...none, build: job({ state: 'succeeded', lines: ['hub: building slot a'] }) }, reach)).toBeNull();
    expect(buildingNode({ ...none, build: job({}) }, reach)).toBeNull();
    expect(buildingNode({ ...none, build: job({ lines: ['other: building slot a'] }) }, reach)).toBeNull();
    expect(buildingNode(none, reach)).toBeNull();
  });
});

describe('the node being built, over a whole build', () => {
  const none2 = { job: null, node: null };
  const running = (lines: string[], id = 1) => ({ ...none, build: job({ id, state: 'running', lines }) });

  test('names nobody until cargo does, then only the one it names', () => {
    let m = buildingNow(running([]), reach, none2);
    expect(m.node).toBeNull();
    m = buildingNow(running(['hub: building slot a']), reach, m);
    expect(m.node).toBe('hub');
    m = buildingNow(running(['hub: building slot a', 'seg: building slot a']), reach, m);
    expect(m.node).toBe('seg');
  });

  test('holds the name under long compiler output, while the same job runs', () => {
    let m = buildingNow(running(['seg: building slot a']), reach, none2);
    m = buildingNow(running(['   Compiling x', '   Compiling y']), reach, m);
    expect(m.node).toBe('seg');
  });

  test('another job does not inherit it, and an ended job names nobody', () => {
    const m = buildingNow(running(['seg: building slot a']), reach, none2);
    expect(buildingNow({ ...none, sign: job({ id: 2, kind: 'sign', state: 'running', lines: [] }) }, reach, m).node).toBeNull();
    expect(buildingNow({ ...none, build: job({ state: 'succeeded', lines: ['seg: building slot a'] }) }, reach, m)).toEqual(none2);
  });
});

describe('what a failed build says', () => {
  // As cargo prints it through dagon build, on Windows.
  const cargo = [
    'hub: building slot a in nodes/hub',
    'hub: slot a -> out/build/hub/app-a.elf (100 bytes)',
    'seg: building slot a in nodes/seg',
    '   Compiling seg v0.1.0',
    'error[E0308]: mismatched types',
    '  --> src\\app.rs:55:21',
    '   |',
    'error: could not compile `seg` (bin "seg") due to 1 previous error',
    'cargo build failed in nodes/seg',
  ];

  test('the failed node, the place and the compiler message; how many nodes before it were built', () => {
    expect(buildFailure({ ...none, build: job({ state: 'failed', lines: cargo }) }, reach)).toEqual({ node: 'seg', reason: 'src/app.rs:55  mismatched types', file: 'app.rs', line: 55, others: 1, skeleton: null });
  });

  test('a failure without a compiler line gives the job\'s last line', () => {
    const f = buildFailure({ ...none, build: job({ state: 'failed', lines: ['no [build.hub] in dagon.toml'] }) }, reach);
    expect(f).toEqual({ node: null, reason: 'no [build.hub] in dagon.toml', file: null, line: null, others: 0, skeleton: 'none' });
  });

  test('a boot crate changed since `dagon gen`: the skeleton is what to make again', () => {
    const lines = ['hub: building the boot core for slot a', '  boot/hub/graph.stamp: the boot crate of node hub changed since its dependency graph was checked: run `dagon gen`'];
    const f = buildFailure({ ...none, sign: job({ kind: 'sign', state: 'failed', lines }) }, reach);
    expect(f.skeleton).toBe('stale');
  });

  test('a failed signing step names no line and no other nodes', () => {
    const sign = job({ kind: 'sign', state: 'failed', lines: ['the signing key is not available'] });
    const f = buildFailure({ ...none, build: job({ state: 'succeeded', lines: cargo.slice(0, 3) }), sign }, reach);
    expect(f).toEqual({ node: null, reason: 'the signing key is not available', file: null, line: null, others: 0, skeleton: null });
  });
});

describe('a signing failure', () => {
  test('names its node and gives the reason without the name', () => {
    const sign = job({ kind: 'sign', state: 'failed', lines: [], result: { error: 'watch: espflash save-image failed' } });
    const f = buildFailure({ ...none, sign }, reach);
    expect(f.node).toBe('watch');
    expect(f.reason).toBe('espflash save-image failed');
  });
});

describe('the first write of a node, while connected', () => {
  test('only in a state where a write can start', () => {
    const nodes = [{ id: 'seg' }];
    const silent = { seg: { responding: false } };
    expect(firstWriteNodes(nodes, silent, true, 'fileReady')).toEqual(['seg']);
    expect(firstWriteNodes(nodes, silent, true, 'writing')).toEqual([]);
    expect(firstWriteNodes(nodes, silent, false, 'idle')).toEqual(['seg']);
  });
});

describe('the first write of a node', () => {
  const nodes = [{ id: 'hub' }, { id: 'seg' }, { id: 'garden', notYet: 'no link' }];

  test('with nothing connected every writable node can be started; one writing cannot reach, not', () => {
    expect(firstWriteNodes(nodes, {}, false, 'idle')).toEqual(['hub', 'seg']);
  });

  test('connected: only those whose DAGON does not answer', () => {
    const live = { hub: { responding: true }, seg: { responding: false } };
    expect(firstWriteNodes(nodes, live, true, 'ready')).toEqual(['seg']);
    expect(firstWriteNodes(nodes, { hub: { responding: true }, seg: { responding: true } }, true, 'ready')).toEqual([]);
  });

  test('a node not heard from yet counts as not answering', () => {
    expect(firstWriteNodes(nodes, { hub: { responding: true }, seg: { responding: null } }, true, 'ready')).toEqual(['seg']);
  });
});
