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

describe('「プロジェクトを作る」 through the autosave queue (C-377)', () => {
  test('gen runs after the save already running and the one for the newest edit, never on an older YAML', async () => {
    const d = new Define();
    d.placeSample();
    const order: string[] = [];
    let release = null as (() => void) | null;
    const a = new Autosave(d, {
      put: async (yaml) => {
        order.push(`put ${/addr: (0x\w+)/.exec(yaml)?.[1]}`);
        if (!release) await new Promise<void>((r) => (release = r));
        return 'saved';
      },
      enabled: () => true,
      timers: manualTimers().timers,
    });
    edit(d, 0x43);
    const first = a.now();
    await settle();
    edit(d, 0x44);
    const made = a.whenSaved(async () => void order.push('gen'));
    await settle();
    release!();
    await first;
    expect(await made).toBe(true);
    expect(order).toEqual(['put 0x43', 'put 0x44', 'gen']);
    expect(d.unsaved).toBe(false);
  });

  test('a save that does not go through makes nothing', async () => {
    const { d, a } = setup(['invalid']);
    edit(d, 0x43);
    let made = false;
    expect(await a.whenSaved(async () => void (made = true))).toBe(false);
    expect([made, d.saveFailure]).toEqual([false, 'invalid']);
  });

  test('savedAt never moves back: an older save answering last does not mark newer edits saved', () => {
    const d = new Define();
    d.placeSample();
    edit(d, 0x43);
    const older = d.revision;
    edit(d, 0x44);
    d.saved(d.revision, 'saved');
    d.saved(older, 'saved');
    expect(d.savedAt).toBe(d.revision);
    edit(d, 0x45);
    expect(d.unsaved).toBe(true);
    d.saved(older, 'saved');
    expect(d.unsaved).toBe(true);
  });
});

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

describe("blocked (the project's definitions do not load)", () => {
  test('no save is ever sent: edits, the quiet time, 保存, leaving, unload and mode switch all stop at the one save', async () => {
    const d = new Define();
    d.placeSample();
    const puts: string[] = [];
    const t = manualTimers();
    let blocked = false;
    const a = new Autosave(d, {
      put: async (yaml) => {
        puts.push(yaml);
        return 'saved';
      },
      enabled: () => true,
      blocked: () => blocked,
      timers: t.timers,
    });
    // An edit before the block: its save is waiting for the quiet time.
    edit(d, 0x43);
    a.edited();
    expect(t.due()?.ms).toBe(AUTOSAVE_MS);
    // Entering blocked drops it (App cancels), and nothing schedules another.
    blocked = true;
    a.cancel();
    expect(t.due()).toBeNull();
    edit(d, 0x44);
    a.edited();
    expect(t.due()).toBeNull();
    await new Promise((r) => setTimeout(r, AUTOSAVE_MS + 50));
    // 保存, unload flush (now), mode switch (ready), leaving (leave): none sends.
    expect(await a.now()).toBe(false);
    expect(await a.ready()).toBe(false);
    let left = false;
    await a.leave(() => (left = true));
    expect(left).toBe(false);
    expect(a.asking).not.toBeNull();
    await a.saveAgain();
    await settle();
    expect(puts).toEqual([]);
    expect(a.pending).toBe(true);
  });
});
