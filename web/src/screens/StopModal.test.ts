import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { BUILD, WRITE } from '../app/copy.ts';
import StopModal from './StopModal.svelte';

const html = (back: boolean, linked: boolean) =>
  render(StopModal, {
    props: {
      target: '[3] ウォッチ',
      back,
      reason: '理由',
      fix: '次にすること',
      linked,
      onask: () => undefined,
      onclose: () => undefined,
      onmore: () => undefined,
      buttons: createRawSnippet(() => ({ render: () => '<button>復旧する</button>' })),
    },
  }).body;
const ask = `data-control="ask"`;

describe('the modal of a stopped write', () => {
  test('went back and an agent is linked: 失敗の理由を聞く, before the band\'s actions', () => {
    const b = html(true, true);
    expect(b).toContain(WRITE.previous);
    expect(b).toContain(`${ask}`);
    expect(b).toContain(BUILD.askAgent);
    expect(b.indexOf(BUILD.askAgent)).toBeLessThan(b.indexOf('復旧する'));
  });

  test('no link: the action is absent, the rest of the modal stays', () => {
    const b = html(true, false);
    expect(b).not.toContain(ask);
    expect(b).not.toContain(BUILD.askAgent);
    expect(b).toContain('理由');
    expect(b).toContain('復旧する');
  });

  test('stopped without going back: no record to ask about, so no action', () => {
    expect(html(false, true)).not.toContain(ask);
  });
});
