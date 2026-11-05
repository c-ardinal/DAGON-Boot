import { describe, expect, test } from 'vitest';
import { LOCK } from './copy.ts';
import { activeStates, lockOf, stateName, type Control, type LockInput } from './locks.ts';

const none: LockInput = {
  demo: false, updating: false, recovering: false, flashing: false, building: false,
  unsaved: false, connecting: false, disconnected: false, thinking: false, site: false, cliNone: false,
};
const states = (p: Partial<LockInput>) => activeStates({ ...none, ...p });
const all = Object.keys(LOCK.controls) as Control[];

describe('lock table', () => {
  test('待機: every operation can be used', () => {
    expect(all.filter((c) => lockOf(c, states({})))).toEqual([]);
    expect(stateName(states({}))).toBe('待機');
  });

  test('更新中 stops all but asking and linking DAGON, each with its reason', () => {
    const s = states({ updating: true });
    expect(all.filter((c) => !lockOf(c, s))).toEqual(['ask', 'link']);
    expect(lockOf('connect', s)).toBe(LOCK.states.updating.why.connect);
  });

  test('several states: the leftmost one gives the reason', () => {
    const s = states({ disconnected: true, updating: true });
    expect(lockOf('update', s)).toBe(LOCK.states.updating.why.update);
    expect(stateName(s)).toBe('書き込み中');
  });

  test('保存中 keeps editing open and waits for the save before the CLI operations', () => {
    const s = states({ unsaved: true });
    expect(lockOf('edit', s)).toBeNull();
    expect(lockOf('build', s)).toBe(LOCK.states.unsaved.why.build);
  });

  test('without the CLI, DAGON cannot be linked or asked; the CLI state alone is not shown as the state', () => {
    const s = states({ cliNone: true });
    expect(lockOf('link', s)).toBe(LOCK.states.cliNone.why.link);
    expect(lockOf('ask', s)).toBe(LOCK.states.cliNone.why.ask);
    expect(lockOf('update', s)).toBeNull();
    expect(stateName(s)).toBe('待機');
  });

  test('the public demo page: files, projects, CLI jobs and DAGON stop with the demo reason; the simulated desk stays usable', () => {
    const s = states({ site: true, cliNone: true });
    expect(all.filter((c) => lockOf(c, s))).toEqual(['gen', 'build', 'flash', 'release', 'open', 'ask', 'apply', 'link', 'file']);
    for (const c of all.filter((c) => lockOf(c, s))) expect(lockOf(c, s)).toBe((LOCK.states.site.why as Record<string, string>)[c]);
    expect(stateName(s)).toBe('待機');
  });

  test('返事待ち stops asking again and applying, nothing else', () => {
    const s = states({ thinking: true });
    expect(all.filter((c) => lockOf(c, s))).toEqual(['ask', 'apply']);
  });

  test('every reason is a polite sentence without red words', () => {
    for (const st of Object.values(LOCK.states)) {
      for (const why of Object.values(st.why) as string[]) {
        expect(why, why).toMatch(/(ます|ません|ください|です)$/);
        expect(why, why).not.toMatch(/×|エラー/);
      }
    }
  });
});
