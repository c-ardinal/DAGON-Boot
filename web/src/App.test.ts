import { render } from 'svelte/server';
import { afterEach, describe, expect, test, vi } from 'vitest';
import App from './App.svelte';
import { BUTTON, CANVAS, CLI_LINK, DEFINE, UNSUPPORTED } from './app/copy.ts';

// The first paint must not depend on dagon-wasm: rendering the component
// touches no WASM export, so a missing .wasm cannot leave the page blank.
vi.mock('./wasm/dagon.ts', () => ({
  dagonReady: () => Promise.reject(new Error('not loaded')),
  decodeHelloResp: () => {
    throw new Error('not loaded');
  },
  encodeHelloReq: () => {
    throw new Error('not loaded');
  },
  versionText: () => {
    throw new Error('not loaded');
  },
}));

function stubBrowser(search: string, serial: unknown) {
  vi.stubGlobal('location', { search, href: `http://127.0.0.1/${search}` });
  vi.stubGlobal('navigator', { serial });
}

afterEach(() => vi.unstubAllGlobals());

describe('first paint without dagon-wasm', () => {
  test('the page opens on 定義 (運用 waits for a 構成 on the 親機); its empty state renders', () => {
    stubBrowser('?mock', undefined);
    const { body } = render(App);
    // Not served by dagon serve here: the band says how to start it (C11).
    expect(body).toContain(CLI_LINK.needCli);
    expect(body).toContain(DEFINE.fromSample);
    expect(body).toContain(DEFINE.listHeading);
    expect(body).toContain(CANVAS.empty);
    expect(body).toContain(BUTTON.connectDiscover);
    expect(body).toContain(BUTTON.placeSample);
  });

  test('the unsupported-browser screen renders', () => {
    stubBrowser('', undefined);
    expect(render(App).body).toContain(UNSUPPORTED.heading);
  });
});
