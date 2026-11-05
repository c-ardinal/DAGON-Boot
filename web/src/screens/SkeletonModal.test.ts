import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { NOTICE, SKELETON } from '../app/copy.ts';
import type { Job } from '../app/define/cli.svelte.ts';
import SkeletonModal from './SkeletonModal.svelte';

const job = (p: Partial<Job>): Job => ({ id: 1, kind: 'gen', state: 'running', target: '', stage: '', percent: 40, lines: [], result: null, question: null, started: 0, ended: null, ...p });
const nodes: [string, string][] = [
  ['[1] メイン', 'nodes/main/'],
  ['[2] 7セグ', 'nodes/seg7/'],
];
const html = (j: Job | null) =>
  render(SkeletonModal, { props: { job: j, place: 'C:/work/home-garden', nodes, onclose: () => undefined, oncancel: () => undefined, onretry: () => undefined, onmore: () => undefined } }).body;

describe('雛形を作る modal', () => {
  test('progress: the bar and 中止 only, no close', () => {
    const b = html(job({ percent: 50 }));
    expect(b).toContain(SKELETON.state.running);
    expect(b).toContain('1 / 2 · 50%');
    expect(b).toContain(SKELETON.cancel);
    expect(b).not.toContain(`aria-label="${NOTICE.close}"`);
  });

  test('success: where each node was made, and what to do next', () => {
    const b = html(job({ state: 'succeeded', percent: 100 }));
    expect(b).toContain(SKELETON.state.done);
    for (const [label, folder] of nodes) expect(b).toContain(label) && expect(b).toContain(folder);
    expect(b).toContain(SKELETON.next.heading);
    expect(b).toContain(SKELETON.copyPath);
  });

  test('failure: the reason and もう一度作る', () => {
    const b = html(job({ state: 'failed', lines: ['started', 'nodes/seg7/ に書き込めません'] }));
    expect(b).toContain(SKELETON.state.failed);
    expect(b).toContain(NOTICE.reason);
    expect(b).toContain('nodes/seg7/ に書き込めません');
    expect(b).toContain(SKELETON.retry);
  });
});
