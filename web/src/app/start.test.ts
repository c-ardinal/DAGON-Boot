import { describe, expect, test } from 'vitest';
import type { Cli, FsList, ProjectResult } from './define/cli.svelte.ts';
import { Start } from './start.svelte.ts';

const HOME: FsList = { path: 'C:/Users/u', parent: null, folders: [{ name: 'desk', project: true }, { name: 'work', project: false }] };

function fakeCli(features: string[], answer: ProjectResult = 'opened') {
  const calls: string[] = [];
  const cli = {
    has: (f: string) => features.includes(f),
    project: async () => ({ open: false }),
    fs: async (p?: string) => {
      calls.push(`fs ${p ?? ''}`);
      return p ? { path: p, parent: HOME.path, folders: [] } : HOME;
    },
    openProject: async (p: string) => (calls.push(`open ${p}`), answer),
    newProject: async (p: string, sample: boolean) => (calls.push(`new ${p} ${sample}`), answer),
  } as unknown as Cli;
  return { cli, calls };
}

describe('SC-S0 start', () => {
  test('new: the home folder list, a name, then serve makes the folder and the page opens it', async () => {
    const { cli, calls } = fakeCli(['project']);
    let opened = 0;
    const s = new Start({ cli, opened: async () => void opened++, local: null });
    await s.choose('new');
    expect(s.list).toEqual(HOME);
    s.name = 'my-desk';
    await s.make();
    expect(calls).toEqual(['fs ', 'new C:/Users/u/my-desk false']);
    expect([opened, s.failed]).toEqual([1, null]);
  });

  test('sample: from desk-demo; open: a project folder opens, another folder is entered', async () => {
    const { cli, calls } = fakeCli(['project']);
    const s = new Start({ cli, opened: async () => undefined, local: null });
    await s.choose('sample');
    expect(s.name).toBe('desk-demo');
    await s.make();
    await s.choose('open');
    await s.open('C:/Users/u/desk');
    await s.browse('C:/Users/u/work');
    expect(calls).toEqual(['fs ', 'new C:/Users/u/desk-demo true', 'fs C:/Users/u', 'open C:/Users/u/desk', 'fs C:/Users/u/work']);
  });

  test('a refusal stays on SC-S0 with its reason; nothing opens', async () => {
    const { cli } = fakeCli(['project'], 'busy');
    let opened = 0;
    const s = new Start({ cli, opened: async () => void opened++, local: null });
    await s.choose('open');
    await s.open('C:/Users/u/desk');
    expect([s.failed, s.making, opened]).toEqual(['busy', false, 0]);
  });

  test('without serve: unavailable, except the development stand-ins, which start here', async () => {
    const { cli } = fakeCli([]);
    const off = new Start({ cli, opened: async () => undefined, local: null });
    expect(['new', 'sample', 'open'].map((c) => off.usable(c as 'new'))).toEqual([false, false, false]);
    const started: string[] = [];
    const dev = new Start({ cli, opened: async () => undefined, local: (c) => void started.push(c) });
    await dev.choose('sample');
    await dev.choose('open');
    expect([started, dev.usable('open')]).toEqual([['sample'], false]);
  });
});
