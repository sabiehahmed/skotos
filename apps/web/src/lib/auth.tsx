import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { skotosApi, SkotosApiError, unwrapApiResult } from "./api";

export function useAuthConfig() {
  return useQuery({
    queryKey: ["auth", "config"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/auth/config")),
    staleTime: 60_000,
  });
}

export function useAuthMe() {
  return useQuery({
    queryKey: ["auth", "me"],
    queryFn: async () => {
      const res = await skotosApi.GET("/api/v1/auth/me");
      if (res.response.status === 401) {
        return null;
      }
      return unwrapApiResult(res);
    },
    retry: (count, error) => {
      if (error instanceof SkotosApiError && error.status === 401) return false;
      return count < 1;
    },
  });
}

export function useLoginMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: { email: string; password: string }) =>
      unwrapApiResult(await skotosApi.POST("/api/v1/auth/login", { body })),
    onSuccess: async (me) => {
      qc.setQueryData(["auth", "me"], me);
      await qc.invalidateQueries({ queryKey: ["projects"] });
    },
  });
}

export function useLogoutMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async () => {
      await skotosApi.POST("/api/v1/auth/logout");
    },
    onSuccess: () => {
      qc.setQueryData(["auth", "me"], null);
      qc.clear();
    },
  });
}
