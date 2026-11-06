import { readFileSync } from 'node:fs';
import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { WRITE } from '../app/copy.ts';
import StopModal from './StopModal.svelte';

const html = () =>
  render(StopModal, {
    props: {
      target: '[3] ウォッチ',
      reason: '理由',
      fix: '次にすること',
      onclose: () => undefined,
      onmore: () => undefined,
      buttons: createRawSnippet(() => ({ render: () => '<button>書き込む</button>' })),
    },
  }).body;

describe('the modal of a write that could not be done', () => {
  test("its headings carry modal-h, and that style lives with Modal so it reaches every modal's body", () => {
    expect(html().match(/<h3 class="modal-h[^"]*"/g)).toHaveLength(2);
    const source = (rel: string) => new TextDecoder().decode(readFileSync(new URL(rel, import.meta.url)));
    expect(source('./Modal.svelte')).toMatch(/dialog :global\(\.modal-h\) \{/);
    expect(source('../App.svelte')).not.toMatch(/^\s*\.modal-h \{/m);
  });

  test('says it failed, why, what to do, and the actions after them', () => {
    const b = html();
    expect(b).toContain(WRITE.failed);
    expect(b).toContain('理由');
    expect(b.indexOf('次にすること')).toBeLessThan(b.indexOf('書き込む</button>'));
  });

  test('its own label, when the outcome has one', () => {
    const b = render(StopModal, {
      props: { target: '[2] 7セグ', label: WRITE.coreBack.label, reason: WRITE.coreBack.detail, fix: WRITE.coreBack.next, onclose: () => undefined, onmore: () => undefined, buttons: createRawSnippet(() => ({ render: () => '<button>x</button>' })) },
    }).body;
    for (const words of [WRITE.coreBack.label, WRITE.coreBack.detail, WRITE.coreBack.next]) expect(b).toContain(words);
    expect(b).not.toContain(WRITE.failed);
  });
});
