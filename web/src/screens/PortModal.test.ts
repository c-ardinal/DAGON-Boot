import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { NOTICE, PORT } from '../app/copy.ts';
import { PORT_DESCRIPTION_MAX, PORT_ID, type Job } from '../app/define/cli.svelte.ts';
import PortModal from './PortModal.svelte';

const job = (p: Partial<Job>): Job => ({ id: 3, kind: 'ai_port', state: 'running', target: 'rp2040 · pico', stage: 'port', percent: null, lines: [], result: null, question: null, started: 0, ended: null, ...p });
const none = () => undefined;
const html = (p: { job?: Job | null; refused?: string | null; off?: string | null; sending?: boolean }) =>
  render(PortModal, { props: { job: null, ...p, onstart: none, oncancel: none, onretry: none, onclose: none, onmore: none } }).body;

describe('ボード移植 modal', () => {
  test('the form: what to port, what the agent may do, and a start that waits for the ids and the consent', () => {
    const b = html({});
    for (const word of [PORT.intro, PORT.mcu, PORT.board, PORT.kind, PORT.description, PORT.consent.heading, PORT.consent.agree, ...PORT.consent.items]) expect(b).toContain(word);
    expect(b).toMatch(/data-control="port-start"[^>]*aria-disabled="true"/);
    expect(b).toContain(PORT.startWhy);
    expect(b).toContain(`aria-label="${NOTICE.close}"`);
    expect(b).not.toContain(PORT.manual.heading);
  });

  test('a page that cannot start it shows why and the same port by hand, and offers no start', () => {
    for (const off of [PORT.off.noFeature, PORT.off.notLinked]) {
      const b = html({ off });
      expect(b).toContain(off);
      expect(b).toContain(PORT.manual.heading);
      expect(b).toContain('/dagon:dagon-port &lt;mcu> &lt;board> S');
      expect(b).not.toContain('data-control="port-start"');
      expect(b).not.toContain(PORT.consent.agree);
    }
  });

  test('running: the agent\'s last steps and 中止 only; it cannot be closed', () => {
    const lines = Array.from({ length: 20 }, (_, i) => `[Bash] step ${i}`);
    const b = html({ job: job({ lines }) });
    expect(b).toContain(PORT.state.running);
    expect(b).toContain('rp2040 · pico');
    expect(b).toContain('[Bash] step 19');
    expect(b).toContain('[Bash] step 8');
    expect(b).not.toContain('[Bash] step 7\n');
    expect(b).toContain(PORT.cancel);
    expect(b).not.toContain(`aria-label="${NOTICE.close}"`);
    expect(html({ job: job({}) })).toContain(PORT.logNone);
  });

  test('a request still with serve shows as running, with nothing to cancel yet', () => {
    const b = html({ sending: true });
    expect(b).toContain(PORT.state.running);
    expect(b).not.toContain(PORT.cancel);
  });

  test('done: the agent\'s report, its figures and what to do next', () => {
    const b = html({ job: job({ state: 'succeeded', result: { ok: true, text: 'conformance 12/12', turns: 41, cost_usd: 3.456 } }) });
    expect(b).toContain(PORT.state.done);
    expect(b).toContain('conformance 12/12');
    expect(b).toContain(PORT.figures(41, 3.456));
    expect(b).toContain(PORT.after);
    expect(PORT.figures(41, null)).not.toContain('USD');
  });

  test('an agent that did not finish its run is a failure with its report, never 完了', () => {
    const b = html({ job: job({ state: 'succeeded', result: { ok: false, text: 'stopped at the iteration limit, 7/12', turns: 200, cost_usd: null } }) });
    expect(b).toContain(PORT.state.failed);
    expect(b).not.toContain(PORT.state.done);
    expect(b).toContain('stopped at the iteration limit, 7/12');
    expect(b).toContain(PORT.retry);
  });

  test('a failed job and a job serve would not take: the reason, the fix and the port by hand', () => {
    const failed = html({ job: job({ state: 'failed', lines: ['[Read] x'], result: { error: 'the packages of tools/agent-sdk are not installed' } }) });
    const refused = html({ refused: 'the port kind must be S or V' });
    for (const [b, why] of [
      [failed, 'the packages of tools/agent-sdk are not installed'],
      [refused, 'the port kind must be S or V'],
    ]) {
      expect(b).toContain(PORT.state.failed);
      expect(b).toContain(why);
      expect(b).toContain(PORT.fail.fix);
      expect(b).toContain(PORT.manual.heading);
      expect(b).toContain(PORT.retry);
      expect(b).toContain(`aria-label="${NOTICE.close}"`);
    }
  });

  test('the id rule is serve\'s: letters, digits, "-", "_" and ".", up to 32', () => {
    for (const ok of ['rp2040', 'nucleo-f401re', 'a.b_c', 'x'.repeat(32)]) expect(PORT_ID.test(ok)).toBe(true);
    for (const bad of ['', 'x'.repeat(33), 'a b', 'ボード', 'a/b', '.hidden']) expect(PORT_ID.test(bad)).toBe(false);
    expect(PORT_DESCRIPTION_MAX).toBe(32 * 1024);
  });
});
