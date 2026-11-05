// The timeline's record: every change of a node's stage while an update runs,
// at the time since the update began.
import { untrack } from 'svelte';
import type { StageEvent } from '../screens/Timeline.svelte';

/** What the log watches: when the update began (null: none runs) and each node's stage. */
export interface StageSource {
  startedAt: number | null;
  progress: Record<string, { stage: string }>;
}

export interface StageSink {
  /** A new update began at `start` (wall ms): the log starts over. */
  begin(start: number): void;
  add(events: StageEvent[]): void;
}

/**
 * Records each node's stage changes from `source` into `sink`; call it where an effect may be made
 * (a component's script). Every stage is read inside the effect, so a change of one node's stage
 * alone is recorded, not only a new progress object.
 */
export function logStages(source: StageSource, sink: StageSink, now: () => number = Date.now): void {
  let started = 0;
  let last: Record<string, string> = {};
  $effect(() => {
    const start = source.startedAt;
    const stages = Object.entries(source.progress).map(([id, p]) => [id, p.stage] as const);
    if (start === null) return;
    untrack(() => {
      if (start !== started) {
        started = start;
        last = {};
        sink.begin(start);
      }
      const t = now() - start;
      const add = stages.filter(([id, stage]) => last[id] !== stage).map(([id, stage]) => ((last[id] = stage), { t, id, stage }));
      if (add.length) sink.add(add);
    });
  });
}
