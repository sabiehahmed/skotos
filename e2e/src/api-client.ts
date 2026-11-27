import { E2E_DEFAULTS, joinUrl, loadConfig, normalizeBaseUrl } from './config';
import type {
  ApiToken,
  AuditLogEntry,
  AuthConfigResponse,
  AuthMeResponse,
  CreateApiTokenRequest,
  CreateApiTokenResponse,
  CreateEnvironmentRequest,
  CreatedEnvironmentResponse,
  CreateFlagRequest,
  CreateMemberRequest,
  CreateProjectRequest,
  CursorPage,
  Environment,
  EnvironmentDetail,
  EnvironmentKeysResponse,
  EnvironmentSummary,
  FlagDetail,
  FlagEnvRevision,
  FlagKind,
  FlagListItem,
  FlagPatchRequest,
  HealthzResponse,
  JsonValue,
  LoginRequest,
  LoginResponse,
  Member,
  ProblemDetails,
  Project,
  ProjectDetail,
  ReadyzResponse,
  RestoreFlagRevisionRequest,
  RotateEnvironmentKeyRequest,
  RotateEnvironmentKeyResponse,
  SimulateFlagRequest,
  SimulateFlagResponse,
  UpdateEnvironmentRequest,
  UpdateMemberRequest,
  UpdateProjectRequest,
  BuiltInRole,
} from './types';

export interface StoredCookie {
  name: string;
  value: string;
  raw: string;
  path?: string;
  maxAge?: number;
  expires?: Date;
  httpOnly: boolean;
  secure: boolean;
  sameSite?: string;
}

export type RequestAuthMode = 'auto' | 'cookie' | 'bearer' | 'none';

export type QueryParamValue =
  | string
  | number
  | boolean
  | null
  | undefined
  | readonly (string | number | boolean)[];

export type QueryParams = Record<string, QueryParamValue>;

export interface SkotosApiClientOptions {
  baseUrl?: string;
  bearerToken?: string | null;
  defaultHeaders?: Record<string, string>;
  timeoutMs?: number;
  fetchImpl?: typeof fetch;
}

export interface ApiRequestOptions {
  method?: string;
  query?: QueryParams;
  headers?: Record<string, string>;
  body?: unknown;
  /** Raw body string for malformed-JSON or non-JSON Tier 2 boundary probes. */
  rawBody?: string;
  contentType?: string;
  ifMatch?: string | number;
  origin?: string;
  authMode?: RequestAuthMode;
  timeoutMs?: number;
}

export interface ApiResponse<T = unknown> {
  status: number;
  ok: boolean;
  headers: Headers;
  etag: string | null;
  contentType: string | null;
  rawText: string;
  body: T | null;
  problem: ProblemDetails | null;
}

export interface ListProjectsQuery {
  limit?: number;
  cursor?: string;
  q?: string;
}

export interface PaginationQuery {
  limit?: number;
  cursor?: string;
}

export interface ListFlagsQuery {
  env?: string;
  q?: string;
  tag?: string;
  archived?: boolean;
  on?: boolean;
  kind?: FlagKind;
  limit?: number;
  cursor?: string;
}

export interface ListAuditLogsQuery {
  projectKey?: string;
  envKey?: string;
  resourceType?: string;
  resourceKey?: string;
  actorId?: string;
  from?: string;
  to?: string;
  q?: string;
  limit?: number;
  cursor?: string;
}

export interface ListMembersQuery {
  limit?: number;
  cursor?: string;
  q?: string;
  role?: BuiltInRole;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function encodeSeg(value: string | number): string {
  return encodeURIComponent(String(value));
}

function hasHeaderCaseInsensitive(headers: Headers, name: string): boolean {
  return headers.has(name);
}

function getSetCookieHeaders(headers: Headers): string[] {
  const maybeGetSetCookie = (headers as Headers & { getSetCookie?: () => string[] }).getSetCookie;
  if (typeof maybeGetSetCookie === 'function') {
    return maybeGetSetCookie.call(headers);
  }
  const single = headers.get('set-cookie');
  return single ? [single] : [];
}

function parseSetCookieHeader(raw: string): StoredCookie | null {
  const parts = raw.split(';');
  const first = parts[0]?.trim() ?? '';
  const eqIdx = first.indexOf('=');
  if (eqIdx <= 0) {
    return null;
  }
  const name = first.slice(0, eqIdx).trim();
  const value = first.slice(eqIdx + 1).trim();
  if (!name) {
    return null;
  }

  const cookie: StoredCookie = {
    name,
    value,
    raw,
    httpOnly: false,
    secure: false,
  };

  for (let i = 1; i < parts.length; i++) {
    const attr = parts[i]?.trim();
    if (!attr) continue;
    const attrEq = attr.indexOf('=');
    const attrKey = (attrEq === -1 ? attr : attr.slice(0, attrEq)).trim().toLowerCase();
    const attrVal = attrEq === -1 ? '' : attr.slice(attrEq + 1).trim();

    switch (attrKey) {
      case 'httponly':
        cookie.httpOnly = true;
        break;
      case 'secure':
        cookie.secure = true;
        break;
      case 'samesite':
        cookie.sameSite = attrVal;
        break;
      case 'path':
        cookie.path = attrVal;
        break;
      case 'max-age': {
        const parsed = Number.parseInt(attrVal, 10);
        if (!Number.isNaN(parsed)) {
          cookie.maxAge = parsed;
        }
        break;
      }
      case 'expires': {
        const date = new Date(attrVal);
        if (!Number.isNaN(date.getTime())) {
          cookie.expires = date;
        }
        break;
      }
      default:
        break;
    }
  }

  return cookie;
}

function buildQueryString(query?: QueryParams): string {
  if (!query) return '';
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null) continue;
    if (Array.isArray(value)) {
      for (const entry of value) {
        params.append(key, String(entry));
      }
    } else {
      params.append(key, String(value));
    }
  }
  const serialized = params.toString();
  return serialized ? `?${serialized}` : '';
}

export function parseEtagVersion(etag: string | null): number | null {
  if (!etag) return null;
  let cleaned = etag.trim();
  if (cleaned.startsWith('W/') || cleaned.startsWith('w/')) {
    cleaned = cleaned.slice(2).trim();
  }
  if (cleaned.startsWith('"') && cleaned.endsWith('"') && cleaned.length >= 2) {
    cleaned = cleaned.slice(1, -1).trim();
  }
  if (!/^\d+$/.test(cleaned)) {
    return null;
  }
  const parsed = Number.parseInt(cleaned, 10);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

export function assertProblemDetails(
  res: ApiResponse<unknown>,
  expectedStatus: number,
  expectedCode?: string | RegExp,
): ProblemDetails {
  const ctx = `status=${res.status}, contentType=${res.contentType ?? '<none>'}, rawText=${res.rawText.slice(0, 400)}`;

  if (res.status !== expectedStatus) {
    throw new Error(
      `Expected ProblemDetails HTTP status ${expectedStatus}, received ${res.status} (${ctx})`,
    );
  }
  if (res.ok) {
    throw new Error(`Expected non-2xx ProblemDetails response, received ok=true (${ctx})`);
  }

  const mime = res.contentType?.split(';')[0]?.trim().toLowerCase() ?? '';
  if (mime !== 'application/problem+json') {
    throw new Error(
      `Expected Content-Type "application/problem+json", received "${res.contentType ?? '<none>'}" (${ctx})`,
    );
  }

  const problem = res.problem;
  if (!problem || !isPlainObject(problem)) {
    throw new Error(`Expected parsed RFC 9457 ProblemDetails JSON object (${ctx})`);
  }

  if (typeof problem.type !== 'string' || problem.type.trim().length === 0) {
    throw new Error(`ProblemDetails missing non-empty string "type" (${ctx})`);
  }
  if (typeof problem.title !== 'string' || problem.title.trim().length === 0) {
    throw new Error(`ProblemDetails missing non-empty string "title" (${ctx})`);
  }
  if (problem.status !== expectedStatus) {
    throw new Error(
      `ProblemDetails.status (${String(problem.status)}) does not match HTTP status ${expectedStatus} (${ctx})`,
    );
  }
  if (typeof problem.detail !== 'string' || problem.detail.trim().length === 0) {
    throw new Error(`ProblemDetails missing non-empty string "detail" (${ctx})`);
  }
  if (typeof problem.instance !== 'string' || problem.instance.trim().length === 0) {
    throw new Error(`ProblemDetails missing non-empty string "instance" (${ctx})`);
  }
  if (typeof problem.code !== 'string' || problem.code.trim().length === 0) {
    throw new Error(`ProblemDetails missing non-empty string "code" (${ctx})`);
  }

  if (expectedCode !== undefined) {
    if (expectedCode instanceof RegExp) {
      if (!expectedCode.test(problem.code)) {
        throw new Error(
          `Expected ProblemDetails.code to match ${String(expectedCode)}, received "${problem.code}" (${ctx})`,
        );
      }
    } else if (problem.code.toUpperCase() !== expectedCode.toUpperCase()) {
      throw new Error(
        `Expected ProblemDetails.code "${expectedCode}", received "${problem.code}" (${ctx})`,
      );
    }
  }

  if (problem.currentVersion !== undefined && problem.currentVersion !== null) {
    if (
      typeof problem.currentVersion !== 'number' ||
      !Number.isInteger(problem.currentVersion) ||
      problem.currentVersion < 1
    ) {
      throw new Error(
        `Expected ProblemDetails.currentVersion to be an integer >= 1, received ${String(problem.currentVersion)} (${ctx})`,
      );
    }
  }

  if (problem.errors !== undefined && problem.errors !== null) {
    if (!Array.isArray(problem.errors)) {
      throw new Error(`Expected ProblemDetails.errors to be an array (${ctx})`);
    }
    for (let i = 0; i < problem.errors.length; i++) {
      const entry = problem.errors[i];
      if (
        !isPlainObject(entry) ||
        typeof entry.field !== 'string' ||
        entry.field.trim().length === 0 ||
        typeof entry.message !== 'string' ||
        entry.message.trim().length === 0
      ) {
        throw new Error(
          `ProblemDetails.errors[${i}] must contain non-empty string "field" and "message" (${ctx})`,
        );
      }
    }
  }

  return problem;
}

export class SkotosHttpClient {
  public readonly baseUrl: string;
  private readonly defaultHeaders: Record<string, string>;
  private readonly timeoutMs: number;
  private readonly fetchImpl: typeof fetch;
  private readonly cookieJar = new Map<string, StoredCookie>();
  private bearerToken: string | null;

  constructor(optionsOrBaseUrl?: string | SkotosApiClientOptions) {
    const opts: SkotosApiClientOptions =
      typeof optionsOrBaseUrl === 'string'
        ? { baseUrl: optionsOrBaseUrl }
        : (optionsOrBaseUrl ?? {});

    const fallbackBaseUrl =
      opts.baseUrl !== undefined ? E2E_DEFAULTS.controlApiUrl : loadConfig().controlApiUrl;
    this.baseUrl = normalizeBaseUrl(opts.baseUrl, fallbackBaseUrl, 'CONTROL_API_URL');
    this.bearerToken = opts.bearerToken ?? null;
    this.defaultHeaders = { ...(opts.defaultHeaders ?? {}) };
    this.timeoutMs = opts.timeoutMs ?? 15_000;
    this.fetchImpl = opts.fetchImpl ?? globalThis.fetch;
  }

  // ---- Cookie Jar & Auth Management ----

  public getCookie(name = 'skotos_session'): StoredCookie | undefined {
    return this.cookieJar.get(name);
  }

  public getCookieHeader(): string | null {
    if (this.cookieJar.size === 0) return null;
    const pairs: string[] = [];
    for (const cookie of this.cookieJar.values()) {
      pairs.push(`${cookie.name}=${cookie.value}`);
    }
    return pairs.join('; ');
  }

  public setCookie(name: string, value: string): void {
    this.cookieJar.set(name, {
      name,
      value,
      raw: `${name}=${value}`,
      httpOnly: false,
      secure: false,
    });
  }

  public setBearerToken(token: string | null): void {
    this.bearerToken = token;
  }

  public getBearerToken(): string | null {
    return this.bearerToken;
  }

  public clearCookies(): void {
    this.cookieJar.clear();
  }

  public clearAuth(): void {
    this.cookieJar.clear();
    this.bearerToken = null;
  }

  public clone(overrides?: Partial<SkotosApiClientOptions>): SkotosHttpClient {
    return new SkotosHttpClient({
      baseUrl: overrides?.baseUrl ?? this.baseUrl,
      bearerToken: overrides?.bearerToken ?? null,
      defaultHeaders: overrides?.defaultHeaders ?? { ...this.defaultHeaders },
      timeoutMs: overrides?.timeoutMs ?? this.timeoutMs,
      fetchImpl: overrides?.fetchImpl ?? this.fetchImpl,
    });
  }

  private captureSetCookies(headers: Headers): void {
    const setCookies = getSetCookieHeaders(headers);
    const now = Date.now();
    for (const raw of setCookies) {
      const parsed = parseSetCookieHeader(raw);
      if (!parsed) continue;
      const expiredByMaxAge =
        parsed.maxAge !== undefined && !Number.isNaN(parsed.maxAge) && parsed.maxAge <= 0;
      const expiredByDate =
        parsed.expires !== undefined &&
        !Number.isNaN(parsed.expires.getTime()) &&
        parsed.expires.getTime() <= now;

      if (parsed.value === '' || expiredByMaxAge || expiredByDate) {
        this.cookieJar.delete(parsed.name);
      } else {
        this.cookieJar.set(parsed.name, parsed);
      }
    }
  }

  // ---- Generic HTTP Transport ----

  public async request<T = unknown>(
    pathname: string,
    options: ApiRequestOptions = {},
  ): Promise<ApiResponse<T>> {
    const method = (options.method ?? 'GET').toUpperCase();
    const url = `${joinUrl(this.baseUrl, pathname)}${buildQueryString(options.query)}`;

    const headers = new Headers();
    for (const [key, value] of Object.entries(this.defaultHeaders)) {
      headers.set(key, value);
    }
    if (options.headers) {
      for (const [key, value] of Object.entries(options.headers)) {
        headers.set(key, value);
      }
    }

    if (options.origin !== undefined) {
      headers.set('Origin', options.origin);
    }

    if (options.ifMatch !== undefined) {
      const formatted =
        typeof options.ifMatch === 'number' ? `"${options.ifMatch}"` : options.ifMatch;
      headers.set('If-Match', formatted);
    }

    const authMode: RequestAuthMode = options.authMode ?? 'auto';
    const hasExplicitAuth = hasHeaderCaseInsensitive(headers, 'authorization');
    const hasExplicitCookie = hasHeaderCaseInsensitive(headers, 'cookie');

    if (authMode === 'auto') {
      if (!hasExplicitAuth && !hasExplicitCookie) {
        if (this.bearerToken !== null) {
          headers.set('Authorization', `Bearer ${this.bearerToken}`);
        } else {
          const cookieHeader = this.getCookieHeader();
          if (cookieHeader) {
            headers.set('Cookie', cookieHeader);
          }
        }
      }
    } else if (authMode === 'bearer') {
      if (!hasExplicitAuth && this.bearerToken !== null) {
        headers.set('Authorization', `Bearer ${this.bearerToken}`);
      }
    } else if (authMode === 'cookie') {
      if (!hasExplicitCookie) {
        const cookieHeader = this.getCookieHeader();
        if (cookieHeader) {
          headers.set('Cookie', cookieHeader);
        }
      }
    }
    // authMode === 'none': inject neither Authorization nor Cookie.

    let serializedBody: string | undefined;
    if (options.rawBody !== undefined) {
      serializedBody = options.rawBody;
      if (!hasHeaderCaseInsensitive(headers, 'content-type')) {
        headers.set('Content-Type', options.contentType ?? 'application/json');
      }
    } else if (options.body !== undefined) {
      serializedBody = JSON.stringify(options.body);
      if (!hasHeaderCaseInsensitive(headers, 'content-type')) {
        headers.set('Content-Type', options.contentType ?? 'application/json');
      }
    } else if (options.contentType && !hasHeaderCaseInsensitive(headers, 'content-type')) {
      headers.set('Content-Type', options.contentType);
    }

    if (!hasHeaderCaseInsensitive(headers, 'accept')) {
      headers.set('Accept', 'application/json, application/problem+json;q=0.9, */*;q=0.1');
    }

    const timeoutMs = options.timeoutMs ?? this.timeoutMs;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);

    let response: Response;
    try {
      response = await this.fetchImpl(url, {
        method,
        headers,
        body: serializedBody,
        redirect: 'manual',
        signal: controller.signal,
      });
    } finally {
      clearTimeout(timer);
    }

    this.captureSetCookies(response.headers);

    const status = response.status;
    const ok = status >= 200 && status <= 299;
    const etag = response.headers.get('etag');
    const contentType = response.headers.get('content-type');
    const rawText = status === 204 || status === 304 ? '' : await response.text();
    const mime = contentType?.split(';')[0]?.trim().toLowerCase() ?? '';

    let body: T | null = null;
    let problem: ProblemDetails | null = null;

    if (
      rawText.trim().length > 0 &&
      (mime === 'application/problem+json' ||
        mime === 'application/json' ||
        mime.endsWith('+json'))
    ) {
      try {
        const parsed: unknown = JSON.parse(rawText);
        if (mime === 'application/problem+json' && isPlainObject(parsed)) {
          problem = parsed as unknown as ProblemDetails;
        } else if (
          status >= 400 &&
          isPlainObject(parsed) &&
          typeof parsed.status === 'number' &&
          typeof parsed.title === 'string'
        ) {
          problem = parsed as unknown as ProblemDetails;
        } else {
          body = parsed as T;
        }
      } catch {
        // Malformed JSON: never throw; preserve rawText with body=null, problem=null.
      }
    }

    return {
      status,
      ok,
      headers: response.headers,
      etag,
      contentType,
      rawText,
      body,
      problem,
    };
  }

  public get<T = unknown>(
    pathname: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<T>> {
    return this.request<T>(pathname, { ...options, method: 'GET' });
  }

  public post<T = unknown>(
    pathname: string,
    body?: unknown,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<T>> {
    return this.request<T>(pathname, { ...options, method: 'POST', body });
  }

  public patch<T = unknown>(
    pathname: string,
    body?: unknown,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<T>> {
    return this.request<T>(pathname, { ...options, method: 'PATCH', body });
  }

  public delete<T = unknown>(
    pathname: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<T>> {
    return this.request<T>(pathname, { ...options, method: 'DELETE' });
  }

  public preflight(
    pathname: string,
    origin: string,
    requestMethod: string,
    requestHeaders?: string | string[],
    options?: Omit<ApiRequestOptions, 'method' | 'origin'>,
  ): Promise<ApiResponse<null>> {
    const hdrs: Record<string, string> = {
      ...(options?.headers ?? {}),
      'Access-Control-Request-Method': requestMethod.toUpperCase(),
    };
    if (requestHeaders !== undefined) {
      hdrs['Access-Control-Request-Headers'] = Array.isArray(requestHeaders)
        ? requestHeaders.join(', ')
        : requestHeaders;
    }
    return this.request<null>(pathname, {
      ...options,
      method: 'OPTIONS',
      origin,
      headers: hdrs,
      authMode: options?.authMode ?? 'none',
    });
  }

  // ---- 1. Health & Readiness (Endpoints 1–2) ----

  public healthz(options?: Omit<ApiRequestOptions, 'method'>): Promise<ApiResponse<HealthzResponse>> {
    return this.get<HealthzResponse>('/healthz', { authMode: 'none', ...options });
  }

  public readyz(options?: Omit<ApiRequestOptions, 'method'>): Promise<ApiResponse<ReadyzResponse>> {
    return this.get<ReadyzResponse>('/readyz', { authMode: 'none', ...options });
  }

  // ---- 2. Authentication (Endpoints 3–6) ----

  public getAuthConfig(
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<AuthConfigResponse>> {
    return this.get<AuthConfigResponse>('/api/v1/auth/config', options);
  }

  public login(
    credentials: LoginRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<LoginResponse>> {
    return this.post<LoginResponse>('/api/v1/auth/login', credentials, {
      authMode: 'none',
      ...options,
    });
  }

  public logout(options?: Omit<ApiRequestOptions, 'method' | 'body'>): Promise<ApiResponse<null>> {
    return this.post<null>('/api/v1/auth/logout', undefined, options);
  }

  public getMe(options?: Omit<ApiRequestOptions, 'method'>): Promise<ApiResponse<AuthMeResponse>> {
    return this.get<AuthMeResponse>('/api/v1/auth/me', options);
  }

  // ---- 3. Projects (Endpoints 7–11) ----

  public listProjects(
    query?: ListProjectsQuery,
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<CursorPage<Project>>> {
    return this.get<CursorPage<Project>>('/api/v1/projects', {
      ...options,
      query: query as QueryParams | undefined,
    });
  }

  public createProject(
    payload: CreateProjectRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<ProjectDetail>> {
    return this.post<ProjectDetail>('/api/v1/projects', payload, options);
  }

  public getProject(
    projectKey: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<ProjectDetail>> {
    return this.get<ProjectDetail>(`/api/v1/projects/${encodeSeg(projectKey)}`, options);
  }

  public updateProject(
    projectKey: string,
    payload: UpdateProjectRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<Project>> {
    return this.patch<Project>(`/api/v1/projects/${encodeSeg(projectKey)}`, payload, options);
  }

  public deleteProject(
    projectKey: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<null>> {
    return this.delete<null>(`/api/v1/projects/${encodeSeg(projectKey)}`, options);
  }

  // ---- 4. Environments & Keys (Endpoints 12–18) ----

  public listEnvironments(
    projectKey: string,
    query?: PaginationQuery,
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<CursorPage<EnvironmentSummary>>> {
    return this.get<CursorPage<EnvironmentSummary>>(
      `/api/v1/projects/${encodeSeg(projectKey)}/environments`,
      { ...options, query: query as QueryParams | undefined },
    );
  }

  public createEnvironment(
    projectKey: string,
    payload: CreateEnvironmentRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<CreatedEnvironmentResponse>> {
    return this.post<CreatedEnvironmentResponse>(
      `/api/v1/projects/${encodeSeg(projectKey)}/environments`,
      payload,
      options,
    );
  }

  public getEnvironment(
    projectKey: string,
    envKey: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<EnvironmentDetail>> {
    return this.get<EnvironmentDetail>(
      `/api/v1/projects/${encodeSeg(projectKey)}/environments/${encodeSeg(envKey)}`,
      options,
    );
  }

  public updateEnvironment(
    projectKey: string,
    envKey: string,
    payload: UpdateEnvironmentRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<Environment>> {
    return this.patch<Environment>(
      `/api/v1/projects/${encodeSeg(projectKey)}/environments/${encodeSeg(envKey)}`,
      payload,
      options,
    );
  }

  public deleteEnvironment(
    projectKey: string,
    envKey: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<null>> {
    return this.delete<null>(
      `/api/v1/projects/${encodeSeg(projectKey)}/environments/${encodeSeg(envKey)}`,
      options,
    );
  }

  public listEnvironmentKeys(
    projectKey: string,
    envKey: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<EnvironmentKeysResponse>> {
    return this.get<EnvironmentKeysResponse>(
      `/api/v1/projects/${encodeSeg(projectKey)}/environments/${encodeSeg(envKey)}/keys`,
      options,
    );
  }

  public rotateEnvironmentKey(
    projectKey: string,
    envKey: string,
    payload: RotateEnvironmentKeyRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<RotateEnvironmentKeyResponse>> {
    return this.post<RotateEnvironmentKeyResponse>(
      `/api/v1/projects/${encodeSeg(projectKey)}/environments/${encodeSeg(envKey)}/keys/rotate`,
      payload,
      options,
    );
  }

  // ---- 5. Feature Flags, Patches, Simulate & Revisions (Endpoints 19–26) ----

  public listFlags(
    projectKey: string,
    query?: ListFlagsQuery,
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<CursorPage<FlagListItem>>> {
    return this.get<CursorPage<FlagListItem>>(`/api/v1/projects/${encodeSeg(projectKey)}/flags`, {
      ...options,
      query: query as QueryParams | undefined,
    });
  }

  public createFlag(
    projectKey: string,
    payload: CreateFlagRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<FlagDetail>> {
    return this.post<FlagDetail>(`/api/v1/projects/${encodeSeg(projectKey)}/flags`, payload, options);
  }

  public getFlag(
    projectKey: string,
    flagKey: string,
    query?: { env?: string },
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<FlagDetail>> {
    return this.get<FlagDetail>(
      `/api/v1/projects/${encodeSeg(projectKey)}/flags/${encodeSeg(flagKey)}`,
      { ...options, query: query as QueryParams | undefined },
    );
  }

  public patchFlag(
    projectKey: string,
    flagKey: string,
    payload: FlagPatchRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<FlagDetail>> {
    return this.patch<FlagDetail>(
      `/api/v1/projects/${encodeSeg(projectKey)}/flags/${encodeSeg(flagKey)}`,
      payload,
      options,
    );
  }

  public deleteFlag(
    projectKey: string,
    flagKey: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<null>> {
    return this.delete<null>(
      `/api/v1/projects/${encodeSeg(projectKey)}/flags/${encodeSeg(flagKey)}`,
      options,
    );
  }

  public simulateFlag<T extends JsonValue = JsonValue>(
    projectKey: string,
    flagKey: string,
    envKey: string,
    payload: SimulateFlagRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<SimulateFlagResponse<T>>> {
    return this.post<SimulateFlagResponse<T>>(
      `/api/v1/projects/${encodeSeg(projectKey)}/flags/${encodeSeg(flagKey)}/environments/${encodeSeg(envKey)}/simulate`,
      payload,
      options,
    );
  }

  public listFlagRevisions(
    projectKey: string,
    flagKey: string,
    envKey: string,
    query?: PaginationQuery,
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<CursorPage<FlagEnvRevision>>> {
    return this.get<CursorPage<FlagEnvRevision>>(
      `/api/v1/projects/${encodeSeg(projectKey)}/flags/${encodeSeg(flagKey)}/environments/${encodeSeg(envKey)}/revisions`,
      { ...options, query: query as QueryParams | undefined },
    );
  }

  public restoreFlagRevision(
    projectKey: string,
    flagKey: string,
    envKey: string,
    version: number,
    payload?: RestoreFlagRevisionRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<FlagDetail>> {
    return this.post<FlagDetail>(
      `/api/v1/projects/${encodeSeg(projectKey)}/flags/${encodeSeg(flagKey)}/environments/${encodeSeg(envKey)}/restore/${encodeSeg(version)}`,
      payload ?? {},
      options,
    );
  }

  // ---- 6. Audit Log (Endpoint 27) ----

  public listAuditLogs(
    query?: ListAuditLogsQuery,
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<CursorPage<AuditLogEntry>>> {
    return this.get<CursorPage<AuditLogEntry>>('/api/v1/audit-log', {
      ...options,
      query: query as QueryParams | undefined,
    });
  }

  // ---- 7. API Access Tokens (Endpoints 28–30) ----

  public listTokens(
    query?: PaginationQuery,
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<CursorPage<ApiToken>>> {
    return this.get<CursorPage<ApiToken>>('/api/v1/tokens', {
      ...options,
      query: query as QueryParams | undefined,
    });
  }

  public createToken(
    payload: CreateApiTokenRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<CreateApiTokenResponse>> {
    return this.post<CreateApiTokenResponse>('/api/v1/tokens', payload, options);
  }

  public revokeToken(
    tokenId: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<null>> {
    return this.delete<null>(`/api/v1/tokens/${encodeSeg(tokenId)}`, options);
  }

  // ---- 8. Members (Endpoints 31–34) ----

  public listMembers(
    query?: ListMembersQuery,
    options?: Omit<ApiRequestOptions, 'method' | 'query'>,
  ): Promise<ApiResponse<CursorPage<Member>>> {
    return this.get<CursorPage<Member>>('/api/v1/members', {
      ...options,
      query: query as QueryParams | undefined,
    });
  }

  public createMember(
    payload: CreateMemberRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<Member>> {
    return this.post<Member>('/api/v1/members', payload, options);
  }

  public updateMember(
    memberId: string,
    payload: UpdateMemberRequest,
    options?: Omit<ApiRequestOptions, 'method' | 'body'>,
  ): Promise<ApiResponse<Member>> {
    return this.patch<Member>(`/api/v1/members/${encodeSeg(memberId)}`, payload, options);
  }

  public deleteMember(
    memberId: string,
    options?: Omit<ApiRequestOptions, 'method'>,
  ): Promise<ApiResponse<null>> {
    return this.delete<null>(`/api/v1/members/${encodeSeg(memberId)}`, options);
  }
}

export { SkotosHttpClient as SkotosApiClient };
