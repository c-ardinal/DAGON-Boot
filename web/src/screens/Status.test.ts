import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { DRAWER, HUB_TOGGLE, NOTICE, STATUS } from '../app/copy.ts';
import Drawer from './Drawer.svelte';
import Status from './Status.svelte';
import Toast from './Toast.svelte';

const status = (p: Partial<Parameters<typeof Status>[1]> = {}) =>
  render(Status, {
    props: {
      hub: '未接続',
      connected: false,
      hubAction: HUB_TOGGLE.connect,
      onhub: () => undefined,
      state: '待機',
      cli: 'CLI 0.4.0',
      cliReady: true,
      oncli: () => undefined,
      saved: STATUS.saved,
      savedOk: true,
      sentence: '親機につないでください。',
      status: '',
      alert: '',
      ...p,
    },
  }).body;

describe('status row', () => {
  test('where the 親機 stands, the one sentence, marks, the connection action last', () => {
    const body = status();
    const at = (s: string) => body.indexOf(s);
    const order = ['未接続', '親機につないでください。', `aria-label="${STATUS.saved}"`, 'aria-label="CLI 0.4.0"', HUB_TOGGLE.connect];
    expect(order.map(at)).toEqual([...order.map(at)].sort((a, b) => a - b));
    expect(order.every((s) => at(s) >= 0)).toBe(true);
  });

  test('the sentence is shown, not only spoken', () => {
    expect(status()).toMatch(/<p class="sentence[^>]*>.*親機につないでください。<\/p>/);
  });

  test('what shows elsewhere is not repeated in the band', () => {
    const body = status();
    for (const word of [DRAWER.problems, '準備', '書込']) expect(body).not.toContain(word);
  });

  test('a held connection stays focusable and says why', () => {
    const body = status({ hubLocked: '書き込みが終わるまで、切断できません。' });
    expect(body).toMatch(/data-control="connect" aria-disabled="true" aria-describedby="hub-why"/);
    expect(body).toContain('書き込みが終わるまで、切断できません。');
  });
});

describe('drawer', () => {
  const tabs = [
    { id: 'problems', label: DRAWER.problems, count: 0 },
    { id: 'log', label: DRAWER.log },
  ];
  const body = createRawSnippet(() => ({ render: () => '<p>body</p>' }));

  test('collapsed: the tab strip stays, no body', () => {
    const html = render(Drawer, { props: { tabs, open: null, onpick: () => undefined, onclose: () => undefined, body } }).body;
    expect(html).toContain(DRAWER.problems);
    expect(html).not.toContain('<p>body</p>');
  });

  test('open: the body of the chosen tab', () => {
    const html = render(Drawer, { props: { tabs, open: 'log', onpick: () => undefined, onclose: () => undefined, body } }).body;
    expect(html).toContain('<p>body</p>');
    expect(html).toContain(`aria-label="${DRAWER.close}"`);
  });
});

describe('toast', () => {
  test('a failure is an alert with a 詳しく見る link', () => {
    const html = render(Toast, { props: { notice: { word: '× 送信失敗', error: true }, onmore: () => undefined, ondone: () => undefined } }).body;
    expect(html).toContain('role="alert"');
    expect(html).toContain(NOTICE.more);
  });

  test('a confirmation is a status', () => {
    const html = render(Toast, { props: { notice: { word: '書き込み済み 3 ノード' }, ondone: () => undefined } }).body;
    expect(html).toContain('role="status"');
  });
});
