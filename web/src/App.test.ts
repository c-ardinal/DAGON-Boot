import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { afterEach, describe, expect, test, vi } from 'vitest';
import App from './App.svelte';
import { CLI_LINK, DEFINE, MODE, START, UNSUPPORTED } from './app/copy.ts';

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

describe('失敗の理由を聞く', () => {
  test('every modal offering it goes through the one path that closes the modal', () => {
    const app = new TextDecoder().decode(readFileSync(new URL('./App.svelte', import.meta.url)));
    expect(app).not.toContain('askDagon(BUILD.askAgent');
    expect(app.match(/askWhy\((?:at|e\.currentTarget), /g)).toHaveLength(2);
  });
});

describe('first paint without dagon-wasm', () => {
  test('the page opens on the start screen: make or open, recent projects, no mode switch or steps yet', () => {
    stubBrowser('?mock', undefined);
    const { body } = render(App);
    for (const label of [START.new, START.open, START.recent]) expect(body).toContain(label);
    expect(body).not.toContain(MODE.label);
    expect(body).not.toContain(DEFINE.listHeading);
  });

  test('not served by dagon serve: the choices are unavailable and the band says how to start it (C11)', () => {
    stubBrowser('', { addEventListener: () => undefined });
    const { body } = render(App);
    expect(body).toContain(CLI_LINK.needCli);
    expect(body).toMatch(/class="button-primary" aria-disabled="true"/);
  });

  test('the unsupported-browser screen renders', () => {
    stubBrowser('', undefined);
    expect(render(App).body).toContain(UNSUPPORTED.heading);
  });
});
