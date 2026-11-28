import { describe, expect, test } from 'vitest';
import type { Cli, Job, PortRequest } from '../define/cli.svelte.ts';
import { PortFlow } from './portFlow.svelte.ts';

const REQ: PortRequest = { mcu: 'rp2040', board: 'pico', kind: 'S' };
const job = (state: Job['state']): Job => ({ id: 5, kind: 'ai_port', state, target: 'rp2040 · pico', stage: '', percent: null, lines: [], result: null, question: null, started: 0, ended: null });

function fake(answer: (r: PortRequest) => Promise<Job | { refused: string }>) {
  const cancelled: number[] = [];
  const cli = { jobs: [] as Job[], aiPort: answer, cancel: async (j: Job) => void cancelled.push(j.id) };
  return { cli, cancelled, flow: new PortFlow({ cli: cli as unknown as Cli, noAnswer: 'no answer' }) };
}

describe('ボード移植 flow', () => {
  test('a job serve takes is followed; closing while it runs leaves it, and the modal shows it again', async () => {
    const f = fake(async () => {
      const j = job('running');
      f.cli.jobs.push(j);
      return j;
    });
    f.flow.show();
    await f.flow.start(REQ);
    expect(f.flow.job?.id).toBe(5);
    expect(f.flow.refused).toBeNull();
    f.flow.close();
    expect(f.flow.open).toBe(false);
    f.flow.show();
    expect(f.flow.job?.id).toBe(5);
    await f.flow.cancel();
    expect(f.cancelled).toEqual([5]);
  });

  test('closing after the run ended puts it away: the next opening is the form', async () => {
    const f = fake(async () => {
      const j = job('succeeded');
      f.cli.jobs.push(j);
      return j;
    });
    await f.flow.start(REQ);
    f.flow.close();
    f.flow.show();
    expect(f.flow.job).toBeNull();
  });

  test('a refusal is its reason, no answer is said as such, and 入力への復帰 clears either', async () => {
    const refused = fake(async () => ({ refused: 'the board must be an id' }));
    await refused.flow.start(REQ);
    expect(refused.flow.refused).toBe('the board must be an id');
    expect(refused.flow.job).toBeNull();
    refused.flow.retry();
    expect(refused.flow.refused).toBeNull();
    const dead = fake(async () => {
      throw new TypeError('connection refused');
    });
    await dead.flow.start(REQ);
    expect(dead.flow.refused).toBe('no answer');
    expect(dead.flow.sending).toBe(false);
  });

  test('a second start while the first is with serve sends nothing', async () => {
    let sent = 0;
    let release: (j: Job) => void = () => undefined;
    const f = fake(() => {
      sent += 1;
      return new Promise((r) => (release = r));
    });
    const first = f.flow.start(REQ);
    await f.flow.start(REQ);
    expect(sent).toBe(1);
    expect(f.flow.sending).toBe(true);
    release(job('running'));
    await first;
    expect(f.flow.sending).toBe(false);
  });
});
