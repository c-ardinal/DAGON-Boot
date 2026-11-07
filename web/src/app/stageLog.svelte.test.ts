import { flushSync } from 'svelte';
import { describe, expect, test } from 'vitest';
import type { StageEvent } from '../screens/Timeline.svelte';
import { logStages, type StageSource } from './stageLog.svelte.ts';

describe('the timeline records every stage change of an update', () => {
  test('a node moving from waiting to writing to done is recorded, not only the start', () => {
    // Operate replaces progress once per update and then changes one node's entry at a time.
    const source = $state<StageSource>({ startedAt: null, progress: {} });
    const events: StageEvent[] = [];
    let begun = 0;
    let t = 1_000;
    const stop = $effect.root(() => logStages(source, { begin: (s) => void (begun = s), add: (e) => events.push(...e) }, () => t));

    source.progress = { seg: { stage: 'waiting' }, watch: { stage: 'waiting' } };
    source.startedAt = 1_000;
    flushSync();
    t = 1_400;
    source.progress.seg = { stage: 'writing' };
    flushSync();
    t = 2_000;
    source.progress.seg = { stage: 'done' };
    source.progress.watch = { stage: 'writing' };
    flushSync();
    stop();

    expect(begun).toBe(1_000);
    expect(events).toEqual([
      { t: 0, id: 'seg', stage: 'waiting' },
      { t: 0, id: 'watch', stage: 'waiting' },
      { t: 400, id: 'seg', stage: 'writing' },
      { t: 1_000, id: 'seg', stage: 'done' },
      { t: 1_000, id: 'watch', stage: 'writing' },
    ]);
  });
});
