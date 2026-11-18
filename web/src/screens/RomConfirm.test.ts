import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import { WRITE } from '../app/copy.ts';
import RomConfirm from './RomConfirm.svelte';

test('says the node does not answer and will be rewritten, and offers 中止 and 書込', () => {
  const { body } = render(RomConfirm, { props: { node: '7セグ', onwrite: () => undefined, oncancel: () => undefined } });
  expect(body).toContain('7セグが応答しません');
  expect(body).toContain('全内容を上書きします');
  expect(body).toContain('配線');
  expect(body).toMatch(new RegExp(`data-rom="cancel"[^>]*>${WRITE.factory.cancel}<`));
  expect(body).toMatch(new RegExp(`data-rom="write"[^>]*>${WRITE.run}<`));
});
