import { describe, expect, test, vi } from 'vitest';
import { request } from './request.ts';

describe('request()', () => {
  const sent = () => vi.fn(async (_url: string, _init?: RequestInit) => new Response(''));

  test('a route goes out with its method, headers and body', async () => {
    const via = sent();
    await request('PUT /v1/layout', { headers: { A: 'b' }, body: '{}', via: via as unknown as typeof fetch });
    expect(via.mock.calls).toEqual([['/v1/layout', { method: 'PUT', headers: { A: 'b' }, body: '{}' }]]);
  });

  test('a :name segment and the query are filled in, encoded', async () => {
    const via = sent();
    await request('GET /v1/kept/:id', { params: { id: 'a/b c' }, via: via as unknown as typeof fetch });
    await request('POST /v1/jobs/:id/cancel', { params: { id: 7 }, via: via as unknown as typeof fetch });
    await request('GET /v1/fs', { query: { path: 'C:\\a b' }, via: via as unknown as typeof fetch });
    await request('GET /v1/fs', { via: via as unknown as typeof fetch });
    expect(via.mock.calls.map((c) => c[0])).toEqual(['/v1/kept/a%2Fb%20c', '/v1/jobs/7/cancel', '/v1/fs?path=C%3A%5Ca%20b', '/v1/fs']);
  });

  test('a :name segment with no value is refused, not sent', () => {
    const via = sent();
    expect(() => request('GET /v1/kept/:id', { via: via as unknown as typeof fetch })).toThrow('id');
    expect(via).not.toHaveBeenCalled();
  });

  test('a file beside the page is a plain GET of its address', async () => {
    const via = sent();
    await request({ file: '/assets/x.dgb' }, { via: via as unknown as typeof fetch });
    expect(via.mock.calls).toEqual([['/assets/x.dgb']]);
  });

  test('without a stand-in it is the browser that sends, looked up when sending', async () => {
    const browser = sent();
    vi.stubGlobal('fetch', browser);
    await request('GET /v1/hello');
    vi.unstubAllGlobals();
    expect(browser.mock.calls[0][0]).toBe('/v1/hello');
  });
});
