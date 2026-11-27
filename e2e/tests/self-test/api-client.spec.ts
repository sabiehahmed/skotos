import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test } from '@playwright/test';
import {
  SkotosApiClient,
  SkotosHttpClient,
  assertProblemDetails,
  parseEtagVersion,
  type ApiResponse,
} from '../../src/api-client';
import type { ProblemDetails } from '../../src/types';

let server: http.Server;
let baseUrl: string;

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let data = '';
    req.setEncoding('utf8');
    req.on('data', (chunk) => {
      data += chunk;
    });
    req.on('end', () => resolve(data));
    req.on('error', reject);
  });
}

function sendJson(
  res: http.ServerResponse,
  status: number,
  payload: unknown,
  headers: Record<string, string | string[]> = {},
): void {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    ...headers,
  });
  res.end(body);
}

function sendProblem(
  res: http.ServerResponse,
  problem: ProblemDetails,
  contentType = 'application/problem+json; charset=utf-8',
): void {
  res.writeHead(problem.status, { 'Content-Type': contentType });
  res.end(JSON.stringify(problem));
}

test.describe('SkotosHttpClient / SkotosApiClient', () => {
  test.beforeAll(async () => {
    server = http.createServer(async (req, res) => {
      const parsedUrl = new URL(req.url ?? '/', 'http://127.0.0.1');
      const pathname = parsedUrl.pathname;
      const method = (req.method ?? 'GET').toUpperCase();
      const rawBody = await readBody(req);

      if (method === 'OPTIONS') {
        res.writeHead(204, {
          'Access-Control-Allow-Origin': req.headers.origin ?? '*',
          'Access-Control-Allow-Credentials': 'true',
          'Access-Control-Allow-Methods': 'GET, POST, PATCH, DELETE, OPTIONS',
          'Access-Control-Allow-Headers':
            String(req.headers['access-control-request-headers'] ?? 'Content-Type, X-Requested-With'),
        });
        res.end();
        return;
      }

      if (method === 'GET' && pathname === '/healthz') {
        sendJson(res, 200, { status: 'ok' });
        return;
      }

      if (method === 'POST' && pathname === '/api/v1/auth/login') {
        const parsed = JSON.parse(rawBody) as { email: string; password: string };
        if (parsed.email === 'admin@skotos.local' && parsed.password === 'secret') {
          sendJson(
            res,
            200,
            {
              member: {
                id: '00000000-0000-4000-8000-000000000001',
                orgId: '00000000-0000-4000-8000-000000000010',
                email: parsed.email,
                displayName: 'Admin',
                role: 'owner',
                createdAt: '2026-11-27T00:00:00.000Z',
              },
              organization: {
                id: '00000000-0000-4000-8000-000000000010',
                slug: 'default',
                name: 'Default Org',
                planTier: 'enterprise',
                createdAt: '2026-11-27T00:00:00.000Z',
              },
            },
            {
              'Set-Cookie': [
                'skotos_session=hmac_abc=123==; HttpOnly; Path=/; SameSite=Lax',
                'skotos_csrf=csrf_tok_99; Path=/; SameSite=Strict',
              ],
            },
          );
          return;
        }
        sendProblem(res, {
          type: 'https://skotos.dev/errors/unauthorized',
          title: 'Unauthorized',
          status: 401,
          detail: 'Invalid credentials',
          instance: pathname,
          code: 'UNAUTHORIZED',
        });
        return;
      }

      if (method === 'POST' && pathname === '/api/v1/auth/logout') {
        res.writeHead(204, {
          'Set-Cookie': [
            'skotos_session=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax',
            'skotos_csrf=expired; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/',
          ],
        });
        res.end();
        return;
      }

      if (method === 'GET' && pathname === '/api/v1/auth/me') {
        const auth = req.headers.authorization ?? null;
        const cookie = req.headers.cookie ?? null;

        if (auth === 'Bearer sk_api_test_0123456789') {
          sendJson(res, 200, {
            principal: {
              orgId: '00000000-0000-4000-8000-000000000010',
              actorType: 'api_token',
              role: 'admin',
            },
            organization: {
              id: '00000000-0000-4000-8000-000000000010',
              slug: 'default',
              name: 'Default Org',
              planTier: 'enterprise',
              createdAt: '2026-11-27T00:00:00.000Z',
            },
            echo: { auth, cookie },
          });
          return;
        }

        if (cookie?.includes('skotos_session=hmac_abc=123==')) {
          sendJson(res, 200, {
            principal: {
              orgId: '00000000-0000-4000-8000-000000000010',
              actorType: 'member',
              role: 'owner',
            },
            organization: {
              id: '00000000-0000-4000-8000-000000000010',
              slug: 'default',
              name: 'Default Org',
              planTier: 'enterprise',
              createdAt: '2026-11-27T00:00:00.000Z',
            },
            echo: { auth, cookie },
          });
          return;
        }

        sendProblem(res, {
          type: 'https://skotos.dev/errors/unauthorized',
          title: 'Unauthorized',
          status: 401,
          detail: 'Missing or invalid session',
          instance: pathname,
          code: 'UNAUTHORIZED',
        });
        return;
      }

      if (method === 'GET' && pathname === '/api/v1/projects/core/flags/checkout') {
        sendJson(
          res,
          200,
          {
            key: 'checkout',
            version: 13,
            environments: { prod: { version: 13, on: true } },
          },
          { ETag: '"13"' },
        );
        return;
      }

      if (method === 'PATCH' && pathname === '/api/v1/projects/core/flags/checkout') {
        const ifMatch = req.headers['if-match'];
        if (ifMatch === '"13"') {
          sendProblem(res, {
            type: 'https://skotos.dev/errors/version-conflict',
            title: 'Version Conflict',
            status: 409,
            detail: 'Expected version 13 but current version is 14',
            instance: pathname,
            code: 'VERSION_CONFLICT',
            currentVersion: 14,
          });
          return;
        }
        sendJson(
          res,
          200,
          { key: 'checkout', version: 15, receivedIfMatch: ifMatch ?? null },
          { ETag: '"15"' },
        );
        return;
      }

      if (method === 'GET' && pathname === '/api/v1/audit-log') {
        const cursor = parsedUrl.searchParams.get('cursor');
        const q = parsedUrl.searchParams.get('q');
        if (!cursor) {
          sendJson(res, 200, {
            items: [
              { id: '1', resourceKey: 'flag-a', queryEcho: q },
              { id: '2', resourceKey: 'flag-b', queryEcho: q },
            ],
            nextCursor: 'cur_2',
            hasMore: true,
          });
        } else {
          sendJson(res, 200, {
            items: [{ id: '3', resourceKey: 'flag-c', queryEcho: q }],
            nextCursor: null,
            hasMore: false,
          });
        }
        return;
      }

      if (pathname === '/echo-headers') {
        sendJson(res, 200, {
          authorization: req.headers.authorization ?? null,
          cookie: req.headers.cookie ?? null,
          origin: req.headers.origin ?? null,
          xRequestedWith: req.headers['x-requested-with'] ?? null,
          contentType: req.headers['content-type'] ?? null,
          rawBody,
        });
        return;
      }

      if (pathname === '/stub/502-html') {
        res.writeHead(502, { 'Content-Type': 'text/html; charset=utf-8' });
        res.end('<html>upstream down</html>');
        return;
      }

      if (pathname === '/stub/bad-json') {
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end('{bad-json');
        return;
      }

      if (pathname === '/stub/204') {
        res.writeHead(204);
        res.end();
        return;
      }

      if (pathname === '/stub/problem-with-errors') {
        sendProblem(res, {
          type: 'https://skotos.dev/errors/validation',
          title: 'Validation Failed',
          status: 422,
          detail: 'Rollout weights must sum to 100000',
          instance: pathname,
          code: 'VALIDATION_ERROR',
          errors: [{ field: 'fallthrough.rollout.variations', message: 'sum was 99999' }],
        });
        return;
      }

      if (pathname === '/stub/wrong-content-type-problem') {
        sendProblem(
          res,
          {
            type: 'https://skotos.dev/errors/bad-request',
            title: 'Bad Request',
            status: 400,
            detail: 'Malformed payload',
            instance: pathname,
            code: 'BAD_REQUEST',
          },
          'application/json',
        );
        return;
      }

      res.writeHead(404, { 'Content-Type': 'application/problem+json' });
      res.end(
        JSON.stringify({
          type: 'https://skotos.dev/errors/not-found',
          title: 'Not Found',
          status: 404,
          detail: `No route for ${method} ${pathname}`,
          instance: pathname,
          code: 'NOT_FOUND',
        }),
      );
    });

    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', () => resolve());
    });
    const addr = server.address() as AddressInfo;
    baseUrl = `http://127.0.0.1:${addr.port}`;
  });

  test.afterAll(async () => {
    await new Promise<void>((resolve, reject) => {
      server.close((err) => (err ? reject(err) : resolve()));
    });
  });

  test('login captures multiple Set-Cookie headers (with = in value), replays Cookie, and logout evicts via Max-Age=0 and past Expires', async () => {
    expect(SkotosApiClient).toBe(SkotosHttpClient);
    const client = new SkotosApiClient({ baseUrl });

    const loginRes = await client.login({ email: 'admin@skotos.local', password: 'secret' });
    expect(loginRes.status).toBe(200);
    expect(loginRes.ok).toBe(true);

    const sessionCookie = client.getCookie('skotos_session');
    expect(sessionCookie).toBeDefined();
    expect(sessionCookie?.value).toBe('hmac_abc=123==');
    expect(sessionCookie?.httpOnly).toBe(true);
    expect(sessionCookie?.sameSite).toBe('Lax');
    expect(sessionCookie?.path).toBe('/');
    expect(client.getCookie('skotos_csrf')?.value).toBe('csrf_tok_99');

    const meRes = await client.getMe();
    expect(meRes.status).toBe(200);
    expect(meRes.body?.principal.role).toBe('owner');

    const logoutRes = await client.logout();
    expect(logoutRes.status).toBe(204);
    expect(logoutRes.rawText).toBe('');
    expect(client.getCookie('skotos_session')).toBeUndefined();
    expect(client.getCookie('skotos_csrf')).toBeUndefined();

    const afterLogout = await client.getMe();
    expect(afterLogout.status).toBe(401);
    const problem = assertProblemDetails(afterLogout, 401, 'UNAUTHORIZED');
    expect(problem.detail).toContain('Missing or invalid session');
  });

  test('clone() isolates cookie jars and bearer tokens between concurrent clients', async () => {
    const admin = new SkotosApiClient({ baseUrl });
    await admin.login({ email: 'admin@skotos.local', password: 'secret' });

    const anon = admin.clone();
    expect(anon.getCookie('skotos_session')).toBeUndefined();
    expect((await admin.getMe()).status).toBe(200);
    expect((await anon.getMe()).status).toBe(401);
  });

  test('Bearer auth excludes Cookie in auto mode and honors explicit authMode overrides', async () => {
    const client = new SkotosApiClient({ baseUrl });
    await client.login({ email: 'admin@skotos.local', password: 'secret' });
    expect(client.getCookie('skotos_session')).toBeDefined();

    // Setting a Bearer token MUST exclude the Cookie header in 'auto' mode.
    client.setBearerToken('sk_api_test_0123456789');
    const bearerRes = await client.get<{
      authorization: string | null;
      cookie: string | null;
    }>('/echo-headers');
    expect(bearerRes.status).toBe(200);
    expect(bearerRes.body?.authorization).toBe('Bearer sk_api_test_0123456789');
    expect(bearerRes.body?.cookie).toBeNull();

    // Explicit 'cookie' override sends Cookie only.
    const cookieRes = await client.get<{
      authorization: string | null;
      cookie: string | null;
    }>('/echo-headers', { authMode: 'cookie' });
    expect(cookieRes.body?.authorization).toBeNull();
    expect(cookieRes.body?.cookie).toContain('skotos_session=hmac_abc=123==');

    // Explicit 'none' sends neither.
    const noneRes = await client.get<{
      authorization: string | null;
      cookie: string | null;
    }>('/echo-headers', { authMode: 'none' });
    expect(noneRes.body?.authorization).toBeNull();
    expect(noneRes.body?.cookie).toBeNull();

    // Also verify Bearer authentication works on /api/v1/auth/me.
    const meWithToken = await client.getMe();
    expect(meWithToken.status).toBe(200);
    expect(meWithToken.body?.principal.actorType).toBe('api_token');
  });

  test('ETag parsing, If-Match transmission, and 409 Conflict ProblemDetails parsing', async () => {
    expect(parseEtagVersion('"13"')).toBe(13);
    expect(parseEtagVersion('W/"42"')).toBe(42);
    expect(parseEtagVersion('invalid')).toBeNull();
    expect(parseEtagVersion(null)).toBeNull();

    const client = new SkotosApiClient({ baseUrl });
    const getRes = await client.getFlag('core', 'checkout', { env: 'prod' });
    expect(getRes.status).toBe(200);
    expect(getRes.etag).toBe('"13"');
    expect(parseEtagVersion(getRes.etag)).toBe(13);

    const conflictRes = await client.patchFlag(
      'core',
      'checkout',
      { environmentKey: 'prod', expectedVersion: 13, instructions: [{ kind: 'turnFlagOff' }] },
      { ifMatch: 13 },
    );
    expect(conflictRes.status).toBe(409);
    expect(conflictRes.ok).toBe(false);
    expect(conflictRes.body).toBeNull();

    const problem = assertProblemDetails(conflictRes, 409, 'VERSION_CONFLICT');
    expect(problem.currentVersion).toBe(14);
  });

  test('cursor pagination and query parameter encoding ignore undefined values', async () => {
    const client = new SkotosApiClient({ baseUrl });

    const page1 = await client.listAuditLogs({
      limit: 2,
      q: 'a&b=c',
      cursor: undefined,
      envKey: undefined,
    });
    expect(page1.status).toBe(200);
    expect(page1.body?.hasMore).toBe(true);
    expect(page1.body?.nextCursor).toBe('cur_2');
    expect(page1.body?.items).toHaveLength(2);
    expect((page1.body?.items[0] as unknown as { queryEcho: string }).queryEcho).toBe('a&b=c');

    const page2 = await client.listAuditLogs({
      limit: 2,
      cursor: page1.body?.nextCursor ?? undefined,
      q: 'a&b=c',
    });
    expect(page2.status).toBe(200);
    expect(page2.body?.hasMore).toBe(false);
    expect(page2.body?.nextCursor).toBeNull();
    expect(page2.body?.items).toHaveLength(1);
  });

  test('CORS preflight, Origin, X-Requested-With, and rawBody request transport work cleanly', async () => {
    const client = new SkotosApiClient({
      baseUrl,
      defaultHeaders: { 'X-Requested-With': 'skotos-web' },
    });

    const pre = await client.preflight('/api/v1/auth/config', 'http://127.0.0.1:5173', 'POST', [
      'Content-Type',
      'X-Requested-With',
    ]);
    expect(pre.status).toBe(204);
    expect(pre.headers.get('access-control-allow-origin')).toBe('http://127.0.0.1:5173');
    expect(pre.headers.get('access-control-allow-credentials')).toBe('true');

    const rawRes = await client.request<{
      origin: string | null;
      xRequestedWith: string | null;
      contentType: string | null;
      rawBody: string;
    }>('/echo-headers', {
      method: 'POST',
      origin: 'http://127.0.0.1:5173',
      rawBody: '{malformed-json',
    });
    expect(rawRes.status).toBe(200);
    expect(rawRes.body?.origin).toBe('http://127.0.0.1:5173');
    expect(rawRes.body?.xRequestedWith).toBe('skotos-web');
    expect(rawRes.body?.contentType).toBe('application/json');
    expect(rawRes.body?.rawBody).toBe('{malformed-json');
  });

  test('preserves rawText on 502 HTML, malformed JSON, and empty 204 without throwing', async () => {
    const client = new SkotosApiClient({ baseUrl });

    const html502 = await client.get('/stub/502-html');
    expect(html502.status).toBe(502);
    expect(html502.ok).toBe(false);
    expect(html502.body).toBeNull();
    expect(html502.problem).toBeNull();
    expect(html502.rawText).toBe('<html>upstream down</html>');

    const badJson = await client.get('/stub/bad-json');
    expect(badJson.status).toBe(200);
    expect(badJson.ok).toBe(true);
    expect(badJson.body).toBeNull();
    expect(badJson.problem).toBeNull();
    expect(badJson.rawText).toBe('{bad-json');

    const empty204 = await client.get('/stub/204');
    expect(empty204.status).toBe(204);
    expect(empty204.ok).toBe(true);
    expect(empty204.rawText).toBe('');
    expect(empty204.body).toBeNull();
  });

  test('assertProblemDetails passes compliant RFC 9457 responses including errors[] and regex code', async () => {
    const client = new SkotosApiClient({ baseUrl });
    const res = await client.get('/stub/problem-with-errors');

    const problem = assertProblemDetails(res, 422, /^VALIDATION_/);
    expect(problem.code).toBe('VALIDATION_ERROR');
    expect(problem.errors).toEqual([
      { field: 'fallthrough.rollout.variations', message: 'sum was 99999' },
    ]);
  });

  test('assertProblemDetails rejects wrong content-type, status mismatch, code mismatch, and missing fields', async () => {
    const client = new SkotosApiClient({ baseUrl });

    // 1. Wrong Content-Type (application/json instead of application/problem+json)
    const wrongCt = await client.get('/stub/wrong-content-type-problem');
    expect(wrongCt.status).toBe(400);
    expect(() => assertProblemDetails(wrongCt, 400, 'BAD_REQUEST')).toThrow(
      /application\/problem\+json/,
    );

    // 2. Wrong expected status / wrong expected code
    const valid422 = await client.get('/stub/problem-with-errors');
    expect(() => assertProblemDetails(valid422, 400)).toThrow(/Expected ProblemDetails HTTP status 400/);
    expect(() => assertProblemDetails(valid422, 422, 'COMMENT_REQUIRED')).toThrow(
      /Expected ProblemDetails\.code "COMMENT_REQUIRED"/,
    );

    // 3. Missing required RFC 9457 fields
    const malformedResp: ApiResponse<unknown> = {
      status: 400,
      ok: false,
      headers: new Headers({ 'content-type': 'application/problem+json' }),
      etag: null,
      contentType: 'application/problem+json',
      rawText: '{"status":400}',
      body: null,
      problem: {
        type: '',
        title: 'Bad Request',
        status: 400,
        detail: 'Missing type',
        instance: '/x',
        code: 'BAD_REQUEST',
      },
    };
    expect(() => assertProblemDetails(malformedResp, 400)).toThrow(/missing non-empty string "type"/);

    // 4. Body status != HTTP status
    const mismatchedBodyStatus: ApiResponse<unknown> = {
      ...malformedResp,
      problem: {
        type: 'https://skotos.dev/errors/bad',
        title: 'Bad Request',
        status: 422,
        detail: 'Mismatched status',
        instance: '/x',
        code: 'BAD_REQUEST',
      },
    };
    expect(() => assertProblemDetails(mismatchedBodyStatus, 400)).toThrow(
      /does not match HTTP status 400/,
    );
  });
});
