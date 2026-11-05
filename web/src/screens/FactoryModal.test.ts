import { render } from 'svelte/server';
import { describe, expect, test, vi } from 'vitest';
import { FLASH, NOTICE, WRITE, fill } from '../app/copy.ts';
import type { Job } from '../app/define/cli.svelte.ts';
import { answerConsent } from '../app/factory.ts';
import FactoryModal from './FactoryModal.svelte';

const job = (p: Partial<Job>): Job => ({ id: 7, kind: 'flash', state: 'queued', target: 'hub', stage: '', percent: null, lines: [], result: null, question: null, started: 0, ended: null, ...p });
const html = (j: Job | null, canWrite = true) =>
  render(FactoryModal, {
    props: {
      node: { label: '[1] メイン', target: '[1] メイン（親機）· Waveshare RP2040-Touch-LCD' },
      how: { label: 'USB', hint: 'BOOTSEL を押したまま USB をつなぐ' },
      canWrite,
      job: j,
      onwrite: () => undefined,
      oncancel: () => undefined,
      onclose: () => undefined,
      onmore: () => undefined,
    },
  }).body;
/** The footer button with this label: its attributes. */
const button = (body: string, label: string) => new RegExp(`<button[^>]*>${label}</button>`).exec(body)?.[0] ?? '';

describe('direct writing modal', () => {
  test('before writing: 初回, why and how, 中止 and 書き込む, closable', () => {
    const b = html(null);
    expect(b).toContain(WRITE.factory.first);
    expect(b).toContain(fill(WRITE.factory.intro, { node: '[1] メイン' }));
    expect(b).toContain('BOOTSEL を押したまま USB をつなぐ');
    expect(button(b, WRITE.factory.cancel)).toContain('aria-disabled="false"');
    expect(button(b, WRITE.run)).toContain('aria-disabled="false"');
    expect(b).toContain(`aria-label="${NOTICE.close}"`);
  });

  test('waiting for serve: 待機中, 中止 still possible, 書き込む held', () => {
    const b = html(job({ state: 'question', question: { id: 'q1', text: 'write?' } }));
    expect(b).toContain(WRITE.factory.waiting);
    expect(button(b, WRITE.factory.cancel)).toContain('aria-disabled="false"');
    expect(button(b, WRITE.run)).toContain('aria-disabled="true"');
  });

  test('writing: 中止 is held with its reason, no close', () => {
    const b = html(job({ state: 'running' }));
    expect(b).toContain(WRITE.state.writing);
    expect(button(b, WRITE.factory.cancel)).toContain('aria-disabled="true"');
    expect(button(b, WRITE.factory.cancel)).toContain(`title="${WRITE.factory.cancelLocked}"`);
    expect(b).not.toContain(`aria-label="${NOTICE.close}"`);
  });

  test('done: 書き込み済み, no actions left', () => {
    const b = html(job({ state: 'succeeded' }));
    expect(b).toContain(WRITE.state.done);
    expect(button(b, WRITE.run)).toBe('');
  });

  test("failed: serve's reason and もう一度書き込む", () => {
    const b = html(job({ state: 'failed', result: { error: 'no RPI-RP2 drive' } }));
    expect(b).toContain(NOTICE.reason);
    expect(b).toContain('no RPI-RP2 drive');
    expect(button(b, WRITE.retry)).toContain('aria-disabled="false"');
  });

  test('an MCU serve cannot write: says so, offers no write', () => {
    const b = html(null, false);
    expect(b).toContain(FLASH.otherTools);
    expect(button(b, WRITE.run)).toBe('');
  });
});

describe('the modal press is the consent serve asks for', () => {
  test("a flash job's question is answered yes, once", () => {
    const answer = vi.fn();
    const seen = new Set<string>();
    const j = job({ state: 'question', question: { id: 'q1', text: 'write?' } });
    expect(answerConsent(j, answer, seen)).toBe(true);
    expect(answerConsent(j, answer, seen)).toBe(false);
    expect(answer).toHaveBeenCalledTimes(1);
    expect(answer).toHaveBeenCalledWith(j, FLASH.yes);
  });

  test('nothing is answered without a question, or for another kind of job', () => {
    const answer = vi.fn();
    const seen = new Set<string>();
    expect(answerConsent(null, answer, seen)).toBe(false);
    expect(answerConsent(job({ state: 'running' }), answer, seen)).toBe(false);
    expect(answerConsent(job({ kind: 'build', state: 'question', question: { id: 'q', text: '?' } }), answer, seen)).toBe(false);
    expect(answer).not.toHaveBeenCalled();
  });
});
