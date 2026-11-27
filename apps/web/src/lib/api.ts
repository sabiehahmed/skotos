import {
  createSkotosClient,
  SkotosApiError,
  unwrapApiResult,
  type components,
} from "@skotos/api-client";

export type AuthConfig = components["schemas"]["AuthConfigResponse"];
export type AuthMe = components["schemas"]["AuthMeResponse"];
export type Project = components["schemas"]["Project"];
export type Environment = components["schemas"]["Environment"];
export type EnvironmentKey = components["schemas"]["EnvironmentKey"];
export type FlagResponse = components["schemas"]["FlagResponse"];
export type FlagEnvConfig = components["schemas"]["FlagEnvConfig"];
export type FlagEnvRevision = components["schemas"]["FlagEnvRevision"];
export type FlagPatchInstruction = components["schemas"]["FlagPatchInstruction"];
export type PatchFlagRequest = components["schemas"]["PatchFlagRequest"];
export type SimulateFlagResponse = components["schemas"]["SimulateFlagResponse"];
export type AuditLogEntry = components["schemas"]["AuditLogEntry"];
export type ApiToken = components["schemas"]["ApiToken"];
export type Member = components["schemas"]["Member"];
export type BuiltInRole = components["schemas"]["BuiltInRole"];
export type FlagKind = components["schemas"]["FlagKind"];
export type Variation = components["schemas"]["Variation"];
export type ClauseOp = components["schemas"]["ClauseOp"];

const baseUrl =
  import.meta.env.VITE_API_BASE_URL ||
  (typeof window !== "undefined" && window.location?.origin && window.location.origin !== "null"
    ? window.location.origin
    : "http://localhost:3000");

export const skotosApi = createSkotosClient({ baseUrl });

export { SkotosApiError, unwrapApiResult };

export function canWrite(role?: BuiltInRole): boolean {
  return role === "writer" || role === "admin" || role === "owner";
}

export function canAdmin(role?: BuiltInRole): boolean {
  return role === "admin" || role === "owner";
}

export function canOwner(role?: BuiltInRole): boolean {
  return role === "owner";
}

export function formatErrorMessage(err: unknown): string {
  if (err instanceof SkotosApiError) {
    if (err.errors && err.errors.length > 0) {
      return `${err.detail} (${err.errors.join(", ")})`;
    }
    return err.detail;
  }
  if (err instanceof Error) return err.message;
  return "An unexpected error occurred";
}
