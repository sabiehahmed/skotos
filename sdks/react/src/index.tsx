import type { BrowserEvaluationDetail, SkotosBrowserClient } from "@skotos/browser-sdk";
import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

const SkotosReactContext = createContext<SkotosBrowserClient | null>(null);

export interface SkotosProviderProps {
  client: SkotosBrowserClient;
  children: ReactNode;
}

/**
 * Provides a [`SkotosBrowserClient`] instance to descendant hooks with automatic live re-renders.
 */
export function SkotosProvider({ client, children }: SkotosProviderProps) {
  return <SkotosReactContext.Provider value={client}>{children}</SkotosReactContext.Provider>;
}

export function useSkotosClient(): SkotosBrowserClient {
  const client = useContext(SkotosReactContext);
  if (!client) {
    throw new Error("useSkotosClient must be used inside <SkotosProvider>");
  }
  return client;
}

export function useFlagDetail<T = unknown>(
  flagKey: string,
  defaultValue: T,
): BrowserEvaluationDetail<T> {
  const client = useSkotosClient();
  const [detail, setDetail] = useState<BrowserEvaluationDetail<T>>(() =>
    client.variationDetail(flagKey, defaultValue),
  );

  useEffect(() => {
    setDetail(client.variationDetail(flagKey, defaultValue));
    return client.on(`change:${flagKey}`, () => {
      setDetail(client.variationDetail(flagKey, defaultValue));
    });
  }, [client, flagKey, defaultValue]);

  return detail;
}

export function useFlag<T = unknown>(flagKey: string, defaultValue: T): T {
  return useFlagDetail(flagKey, defaultValue).value;
}

export function useFlags(): Record<string, unknown> {
  const client = useSkotosClient();
  const [flags, setFlags] = useState<Record<string, unknown>>(() => client.allFlags());

  useEffect(() => {
    setFlags(client.allFlags());
    return client.on("change", () => {
      setFlags(client.allFlags());
    });
  }, [client]);

  return flags;
}
