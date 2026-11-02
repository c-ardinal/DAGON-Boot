import { describe, expect, test } from 'vitest';
import { Autosave, AUTOSAVE_MS, type Timers } from './autosave.svelte.ts';
import type { SaveResult } from './cli.svelte.ts';
import { Define } from './define.svelte.ts';
import { setLink } from './model.ts';

/** Timers the test runs by hand. */
function manualTimers() {
  let due: { f: () => void; ms: number } | null = null;
  const timers: Timers = {
    set: (f, ms) => (due = { f, ms }),
    clear: () => void (due = null),
  };
  return { timers, due: () => due, fire: () => (due?.f(), (due = null)) };
}

function setup(answers: SaveResult[], enabled = true) {
  const d = new Define();
  d.placeSample();
  const puts: string[] = [];
  const t = manualTimers();
  const a = new Autosave(d, {
    put: async (yaml) => {
      puts.push(yaml);
      return answers.shift() ?? 'saved';
    },
    enabled: () => enabled,
    timers: t.timers,
  });
  return { d, a, puts, t };
}

const settle = () => new Promise((r) => setTimeout(r, 0));
const edit = (d: Define, addr: number) => d.apply(setLink(d.doc, 0, 'addr', addr));

describe('autosave of the 構成 (UC-D04)', () => {
  test('edits are saved once, AUTOSAVE_MS after the last one; a refused save keeps them and the next edit tries again', async () => {
    const { d, a, puts, t } = setup(['invalid', 'saved']);
    a.edited();
    edit(d, 0x43);
    a.edited();
    expect([t.due()?.ms, puts.length]).toEqual([AUTOSAVE_MS, 0]);
    t.fire();
    await settle();
    expect(puts).toHaveLength(1);
    expect(puts[0]).toContain('addr: 0x43');
    expect([d.unsaved, d.saveFailure, d.doc.topo.links![0].addr]).toEqual([true, 'invalid', 0x43]);

    edit(d, 0x44);
    a.edited();
    t.fire();
    await settle();
    expect([puts.length, d.unsaved, d.saveFailure]).toEqual([2, false, null]);
    a.edited();
    expect(t.due()).toBeNull();
  });

  test('leaving with unsaved edits saves first; when that fails the dialog asks (save again / discard / stay)', async () => {
    const { d, a, puts } = setup(['busy', 'failed', 'saved']);
    const went: string[] = [];
    await a.leave(() => went.push('start'));
    expect([went, a.asking !== null, d.saveFailure]).toEqual([[], true, 'busy']);
    await a.saveAgain();
    expect([went, a.asking !== null]).toEqual([[], true]);
    a.stay();
    expect([went, a.asking, d.unsaved]).toEqual([[], null, true]);

    await a.leave(() => went.push('start'));
    expect([went, puts.length, d.unsaved]).toEqual([['start'], 3, false]);
    await a.leave(() => went.push('again'));
    expect([went, puts.length]).toEqual([['start', 'again'], 3]);
  });

  test('discarding goes on without saving', async () => {
    const { a } = setup(['failed']);
    const went: string[] = [];
    await a.leave(() => went.push('restart'));
    a.discard();
    expect([went, a.asking]).toEqual([['restart'], null]);
  });

  test('運用 and 親機へ送る wait for the saved 構成: ready() is false while it cannot be saved, and saves what operate then uses', async () => {
    const { d, a, puts } = setup(['invalid', 'saved']);
    expect(await a.ready()).toBe(false);
    expect(await a.ready()).toBe(true);
    expect(puts[1]).toBe(d.config.yaml);
    expect(await a.ready()).toBe(true);
    expect(puts).toHaveLength(2);
  });

  test('a development stand-in has nothing to save to: no save, nothing pending', async () => {
    const { a, puts } = setup([], false);
    a.edited();
    expect([a.pending, await a.ready(), puts]).toEqual([false, true, []]);
  });
});
