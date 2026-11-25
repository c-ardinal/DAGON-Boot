// Every rule, reason, job kind and job state dagon-wasm names has the page's words: the lists come
// from the Rust side (dagon_topology::RULES, dagon_host::REASONS, dagon_host::jobs), so one added
// there fails here until it has them, and a word for something no list names fails too.
import { describe, expect, test } from 'vitest';
import { JOBS, NODE, RULE_FIX, RULE_SHORT, SILENCE, SLOT, WRITE } from './copy.ts';
import { stageWord } from './console/states.ts';
import { resultChip } from './nodeState.ts';
import { failureStage, REFUSALS } from './writeProgress.ts';
import { failure_reasons, job_kinds, job_states, rule_ids, silence_keys } from '../wasm/pkg/dagon_wasm.js';
import { wireNumbers } from '../wasm/dagon.ts';

const sorted = (xs: Iterable<string>) => [...xs].sort();

describe('the check rules', () => {
  test('have a short name and a fix, keyed by every rule and nothing else', () => {
    const rules = sorted(rule_ids());
    expect(rules.length).toBeGreaterThan(20);
    expect(sorted(Object.keys(RULE_SHORT))).toEqual(rules);
    expect(sorted(Object.keys(RULE_FIX))).toEqual(rules);
  });
});

describe("dagon-host's reasons", () => {
  // Where a stage has its own dialog, the dialog's words stand for the detail.
  const OWN: Record<string, string> = { previous: WRITE.rolled.reason, coreBack: WRITE.coreBack.detail };
  test.each(failure_reasons())('%s has a chip and a cause', (reason) => {
    const f = failureStage(reason);
    expect(stageWord(f.stage)).not.toBe(f.stage);
    const why = WRITE.why[reason]?.reason ?? WRITE.why[f.stage]?.reason ?? OWN[f.stage];
    expect(why).toBeTruthy();
  });
  test('the refused chips and causes name only reasons dagon-host gives', () => {
    const reasons = new Set(failure_reasons());
    expect(Object.keys(NODE.refused).filter((r) => !reasons.has(r))).toEqual([]);
    const stages = ['silent', 'refused', 'cannotFix'];
    expect(Object.keys(WRITE.why).filter((r) => !reasons.has(r) && !stages.includes(r))).toEqual([]);
    expect(REFUSALS.every((r) => reasons.has(r))).toBe(true);
  });
});

describe('a node that does not answer', () => {
  test("has the design's word for each of dagon-host's cases, and for nothing else", () => {
    expect(sorted(Object.keys(SILENCE))).toEqual(sorted(silence_keys()));
    expect(Object.values(SILENCE).map((s) => s.word)).toEqual(['途絶', '通信実績なし', '履歴不明']);
  });
  test('says its case on its card and in the cause of a stop', () => {
    expect(resultChip('silent', 'lost')).toEqual({ chip: '× 途絶', spoken: '途絶', silent: true });
    expect(resultChip('silent', 'neverReached')?.chip).toBe('× 通信実績なし');
    expect(resultChip('silent', 'historyUnknown')?.chip).toBe('× 履歴不明');
    expect(WRITE.why[failureStage('LOST').reason!].reason).toContain(SILENCE.lost.word);
    expect(WRITE.why[failureStage('HISTORY UNKNOWN').reason!].reason).toContain(SILENCE.historyUnknown.word);
  });
});

describe("dagon serve's jobs", () => {
  test('have words for every kind and state, and for nothing else (unknown: what the page shows otherwise)', () => {
    expect(sorted(Object.keys(JOBS.kinds))).toEqual(sorted(job_kinds()));
    expect(sorted(Object.keys(JOBS.states))).toEqual(sorted([...job_states(), 'unknown']));
  });
});

describe('the slot states', () => {
  test('have a word each, at their wire number', () => {
    const states = Object.values(wireNumbers().slot);
    expect(SLOT.states.length).toBe(states.length);
    expect(states.every((n) => SLOT.states[n])).toBe(true);
  });
});
