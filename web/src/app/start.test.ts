import { describe, expect, test } from 'vitest';
import type { Cli, FsList, ProjectResult } from './define/cli.svelte.ts';
import { Start } from './start.svelte.ts';

const HOME: FsList = { path: 'C:/Users/u', parent: null, folders: [{ name: 'project', project: true }, { name: 'work', project: false }] };

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
    initProject: async (p: string) => (calls.push(`init ${p}`), 'opened'),
    newProject: async (p: string, sample: boolean) => (calls.push(`new ${p} ${sample}`), answer),
  } as unknown as Cli;
  return { cli, calls };
}

describe('start screen', () => {
  test('new: the home folder list, a name, then serve makes the folder and the page opens it', async () => {
    const { cli, calls } = fakeCli(['project']);
    let opened = 0;
    const s = new Start({ cli, opened: async () => void opened++, local: null });
    await s.choose('new');
    expect(s.list).toEqual(HOME);
    s.name = 'my-project';
    await s.make();
    expect(calls).toEqual(['fs ', 'new C:/Users/u/my-project false']);
    expect([opened, s.failed]).toEqual([1, null]);
  });

  test('sample: from sample; open: a project folder opens, another folder is entered', async () => {
    const { cli, calls } = fakeCli(['project']);
    const s = new Start({ cli, opened: async () => undefined, local: null });
    await s.choose('sample');
    expect(s.name).toBe('sample');
    await s.make();
    await s.choose('open');
    await s.open('C:/Users/u/project');
    await s.browse('C:/Users/u/work');
    expect(calls).toEqual(['fs ', 'new C:/Users/u/sample true', 'fs C:/Users/u', 'open C:/Users/u/project', 'fs C:/Users/u/work']);
  });

  test('a folder with only a 構成 asks first: yes makes dagon.toml and opens it, no stays', async () => {
    const { cli, calls } = fakeCli(['project'], 'yaml');
    let opened = 0;
    const s = new Start({ cli, opened: async () => void opened++, local: null });
    await s.choose('open');
    await s.open('C:/Users/u/y');
    expect([s.asking, s.failed, opened]).toEqual(['C:/Users/u/y', null, 0]);
    await s.init(false);
    expect([s.asking, opened]).toEqual([null, 0]);
    await s.open('C:/Users/u/y');
    await s.init(true);
    expect([s.asking, s.failed, opened]).toEqual([null, null, 1]);
    expect(calls.slice(-3)).toEqual(['open C:/Users/u/y', 'open C:/Users/u/y', 'init C:/Users/u/y']);
  });

  test('the recent list is serve\'s, at most 5 shown', async () => {
    const { cli } = fakeCli(['project']);
    const recent = Array.from({ length: 10 }, (_, i) => ({ path: `C:/Users/u/p${i}`, name: `p${i}` }));
    (cli as unknown as { project: () => Promise<unknown> }).project = async () => ({ open: false, recent });
    const s = new Start({ cli, opened: async () => undefined, local: null });
    await s.refresh();
    expect(s.recent.map((r) => r.name)).toEqual(['p0', 'p1', 'p2', 'p3', 'p4']);
  });

  test('a refusal stays on the start screen with its reason; nothing opens', async () => {
    const { cli } = fakeCli(['project'], 'busy');
    let opened = 0;
    const s = new Start({ cli, opened: async () => void opened++, local: null });
    await s.choose('open');
    await s.open('C:/Users/u/project');
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
