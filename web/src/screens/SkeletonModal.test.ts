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
const html = (j: Job | null, failure: string | null = null) =>
  render(SkeletonModal, { props: { job: j, failure, place: 'C:/work/home-garden', nodes, onclose: () => undefined, oncancel: () => undefined, onretry: () => undefined, onmore: () => undefined } }).body;

describe('雛形を作る modal', () => {
  test('progress: what is being made and 中止 only, no close; no number the job never reports', () => {
    const b = html(job({ percent: null }));
    expect(b).toContain(SKELETON.state.running);
    expect(b).toContain('nodes/main/');
    expect(b).not.toContain('progressbar');
    expect(b).not.toContain('%');
    expect(b).toContain(SKELETON.cancel);
    expect(b).not.toContain(`aria-label="${NOTICE.close}"`);
  });

  test('success: where each node was made, and what to do next', () => {
    const b = html(job({ state: 'succeeded', percent: 100 }));
    expect(b).toContain(SKELETON.state.done);
    for (const [label, folder] of nodes) {
      expect(b).toContain(label);
      expect(b).toContain(folder);
    }
    expect(b).toContain(SKELETON.next.heading);
  });

  test('success: the one chip says it; no chip per row and no copy button', () => {
    const b = html(job({ state: 'succeeded', result: { lines: 3, skipped: [] } }));
    expect(b.split(SKELETON.state.done).length - 1).toBe(1);
    expect(b).not.toContain('場所をコピー');
  });

  test('a node dagon gen skipped is a failure with its reason, never 作成済', () => {
    const b = html(job({ state: 'succeeded', result: { lines: 3, skipped: ['skipped    nodes/seg7: no template for this board'] } }));
    expect(b).toContain(SKELETON.state.failed);
    expect(b).not.toContain(SKELETON.state.done);
    expect(b).toContain('nodes/seg7: no template for this board');
    expect(b).toContain(SKELETON.retry);
  });

  test('a job serve would not take: the reason and もう一度作る, and it can be closed', () => {
    const b = html(null, SKELETON.startFailed);
    expect(b).toContain(SKELETON.state.failed);
    expect(b).toContain(SKELETON.startFailed);
    expect(b).toContain(SKELETON.retry);
    expect(b).toContain(`aria-label="${NOTICE.close}"`);
  });

  test('failure: the reason and もう一度作る', () => {
    const b = html(job({ state: 'failed', lines: ['started', 'nodes/seg7/ に書き込めません'] }));
    expect(b).toContain(SKELETON.state.failed);
    expect(b).toContain(NOTICE.reason);
    expect(b).toContain('nodes/seg7/ に書き込めません');
    expect(b).toContain(NOTICE.fix);
    expect(b).toContain(SKELETON.fail.fix);
    expect(b).toContain(SKELETON.retry);
  });
});
