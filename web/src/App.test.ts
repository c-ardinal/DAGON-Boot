import { render } from 'svelte/server';
import { afterEach, describe, expect, test, vi } from 'vitest';
import App from './App.svelte';
import { BAND, BUTTON, CANVAS, UNSUPPORTED } from './app/copy.ts';

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
  test('the empty state, its sentence and its buttons render', () => {
    stubBrowser('?mock', undefined);
    const { body } = render(App);
    expect(body).toContain(BAND.M03.sentence);
    expect(body).toContain(CANVAS.empty);
    expect(body).toContain(BUTTON.connectDiscover);
    expect(body).toContain(BUTTON.placeSample);
  });

  test('the unsupported-browser screen renders', () => {
    stubBrowser('', undefined);
    expect(render(App).body).toContain(UNSUPPORTED.heading);
  });
});
