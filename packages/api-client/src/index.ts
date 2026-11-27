import createClient, { type ClientOptions } from "openapi-fetch";
import type { components, paths } from "./schema";

export type { components, paths };

export type ProblemDetails = components["schemas"]["ProblemDetails"];

/**
 * Typed RFC 9457 error wrapper thrown by helpers when an API request returns 4xx/5xx.
 */
export class SkotosApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly detail: string;
  readonly currentVersion?: number | null;
  readonly errors?: string[] | null;
  readonly problem?: ProblemDetails;

  constructor(status: number, problem?: Partial<ProblemDetails>) {
    const detail = problem?.detail ?? `Request failed with status ${status}`;
    super(detail);
    this.name = "SkotosApiError";
    this.status = status;
    this.code = problem?.code ?? "unknown_error";
    this.detail = detail;
    this.currentVersion = problem?.currentVersion;
    this.errors = problem?.errors;
    this.problem = problem as ProblemDetails | undefined;
  }
}

export interface CreateSkotosClientOptions extends Omit<ClientOptions, "baseUrl"> {
  baseUrl?: string;
}

/**
 * Creates a fully typed OpenAPI fetch client configured with `credentials: "include"`
 * for Skotos HttpOnly session cookies and explicit CORS support.
 */
export function createSkotosClient(options: CreateSkotosClientOptions = {}) {
  const { baseUrl = "", credentials = "include", fetch: customFetch, ...rest } = options;
  return createClient<paths>({
    baseUrl,
    credentials,
    fetch: customFetch ?? ((request) => globalThis.fetch(request)),
    ...rest,
  });
}

export type SkotosClient = ReturnType<typeof createSkotosClient>;

/**
 * Unwraps an `openapi-fetch` `{ data, error, response }` tuple or throws [`SkotosApiError`].
 */
export function unwrapApiResult<T>(result: {
  data?: T;
  error?: unknown;
  response: Response;
}): T {
  if (result.error !== undefined || !result.response.ok) {
    throw new SkotosApiError(
      result.response.status,
      typeof result.error === "object" && result.error !== null
        ? (result.error as Partial<ProblemDetails>)
        : undefined,
    );
  }
  return result.data as T;
}
