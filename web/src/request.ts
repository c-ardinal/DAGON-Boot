// The one place the Console sends an HTTP request from. A `dagon serve` route
// is named by an entry of ROUTES, so a call the server does not route does not
// compile, and app/contracts.test.ts holds the table to the server's.

/**
 * The `dagon serve` routes the Console calls: the method, then the path, where
 * `:name` is a segment filled from `params` (named as serve's route table
 * names it).
 */
export const ROUTES = [
  'GET /v1/hello',
  'GET /v1/agent',
  'POST /v1/agent/link',
  'DELETE /v1/agent/link',
  'PUT /v1/agent/key/:agent/:way',
  'DELETE /v1/agent/key/:agent/:way',
  'POST /v1/agent/ask',
  'GET /v1/definitions',
  'GET /v1/topology',
  'PUT /v1/topology',
  'POST /v1/build',
  'POST /v1/sign',
  'POST /v1/release',
  'GET /v1/release/latest',
  'GET /v1/release/:id',
  'GET /v1/kept',
  'GET /v1/kept/:id',
  'POST /v1/kept',
  'GET /v1/bootcore/:node',
  'GET /v1/stage0/:node',
  'GET /v1/layout',
  'PUT /v1/layout',
  'POST /v1/jobs/:id/answer',
  'POST /v1/jobs/:id/cancel',
  'POST /v1/device',
  'GET /v1/project',
  'POST /v1/project/open',
  'POST /v1/project/new',
  'POST /v1/project/init',
  'GET /v1/fs',
  'POST /v1/gen',
  'POST /v1/flash',
  'POST /v1/ai/port',
] as const;

export type Route = (typeof ROUTES)[number];

declare const pageFile: unique symbol;

/**
 * The address of a file the bundler put beside the page, which is no serve
 * route. Only the import of such a file has this type (page-file.d.ts), so no
 * other text can be asked for.
 */
export type PageFile = string & { readonly [pageFile]: true };

export interface RequestOptions {
  /** The values of the route's `:name` segments. */
  params?: Record<string, string | number>;
  /** What follows `?`. */
  query?: Record<string, string>;
  headers?: Record<string, string>;
  body?: BodyInit;
  /** What sends it instead of the browser. */
  via?: typeof fetch;
}

/** The browser's own sending, looked up when sending. */
const browser = (url: string, init?: RequestInit): Promise<Response> => fetch(url, init);

/** Sends `to`, a serve route, with its parameters, headers and body. */
export function request(to: Route, o: RequestOptions = {}): Promise<Response> {
  const send = o.via ?? browser;
  const [method, template] = to.split(' ');
  const path = template.replace(/:(\w+)/g, (_, name: string) => {
    const value = o.params?.[name];
    if (value === undefined) throw new Error(`${to}: no ${name}`);
    return encodeURIComponent(String(value));
  });
  const query = Object.entries(o.query ?? {}).map(([k, v]) => `${k}=${encodeURIComponent(v)}`);
  return send(query.length ? `${path}?${query.join('&')}` : path, { method, headers: o.headers, body: o.body });
}

/** A plain GET of a file beside the page. */
export function requestFile(file: PageFile): Promise<Response> {
  return browser(file);
}
