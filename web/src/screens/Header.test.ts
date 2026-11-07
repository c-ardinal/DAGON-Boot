import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { MENU, MODE, PRODUCT, WRITE } from '../app/copy.ts';
import Header from './Header.svelte';

type Props = Parameters<typeof Header>[1];
const head = (p: Partial<Props> = {}) => render(Header, { props: { onmode: () => undefined, onmenu: () => undefined, ...p } }).body;
/** The mode toggle's segments, in rendered order: [label, checked]. */
const modes = (body: string) => [...body.matchAll(/<button role="radio" aria-checked="(true|false)"[^>]*>([^<]*)<\/button>/g)].map(([, c, l]) => [l, c]);

describe('header (TopBar)', () => {
  test('the modes run in the order the user works: 定義 then 運用', () => {
    expect(modes(head({ mode: 'operate' }))).toEqual([
      [MODE.define, 'false'],
      [MODE.operate, 'true'],
    ]);
  });

  test('the start screen has no mode toggle', () => {
    expect(modes(head({ onmode: undefined }))).toEqual([]);
  });

  test('brand, then the project name', () => {
    const body = head({ title: 'home-garden' });
    expect(body.indexOf(PRODUCT)).toBeLessThan(body.indexOf('home-garden'));
  });

  test('the menu is a ghost icon button with its name for screen readers', () => {
    expect(head()).toMatch(new RegExp(`<button class="button-icon" aria-label="${MENU.button}"`));
  });

  test("the mode's actions sit right of the toggle", () => {
    const actions = createRawSnippet(() => ({ render: () => `<button class="button-primary-header"><span>${WRITE.run}</span></button>` }));
    const body = head({ actions });
    expect(body.indexOf('radiogroup')).toBeLessThan(body.indexOf(WRITE.run));
  });
});
