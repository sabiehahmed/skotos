export const API_CLIENT_NAME = "@skotos/api-client" as const;
export const API_CLIENT_VERSION = "0.0.1" as const;

export interface SkotosClientConfig {
  baseUrl: string;
  credentials: RequestCredentials;
}

/**
 * Placeholder configuration helper until OpenAPI 3.1 typed client codegen
 * lands in Milestone 1C (M4).
 */
export function createStubClientConfig(baseUrl = ""): SkotosClientConfig {
  return {
    baseUrl: baseUrl.replace(/\/+$/, ""),
    credentials: "include",
  };
}
