import { Alert, Button, Card, FormField, Heading, Icons, Input, Stack, Text } from "@skotos/ui";
import { useState, type FormEvent } from "react";
import { formatErrorMessage } from "../lib/api";
import { useAuthConfig, useLoginMutation } from "../lib/auth";

export interface LoginPageProps {
  onLoggedIn: () => void;
}

export function LoginPage({ onLoggedIn }: LoginPageProps) {
  const { data: authConfig } = useAuthConfig();
  const login = useLoginMutation();
  const [email, setEmail] = useState("admin@skotos.local");
  const [password, setPassword] = useState("SkotosDevPass123!");
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await login.mutateAsync({ email, password });
      onLoggedIn();
    } catch (err) {
      setError(formatErrorMessage(err));
    }
  };

  return (
    <div
      style={{
        minHeight: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: "var(--sk-space-6)",
        background:
          "radial-gradient(var(--sk-canvas-dot) 1px, transparent 1.2px) 0 0 / 22px 22px, var(--sk-color-bg)",
      }}
    >
      <div style={{ width: "100%", maxWidth: 420 }}>
        <Card padded>
          <Stack gap={5}>
            <Stack gap={2}>
              <Stack direction="horizontal" align="center" gap={2}>
                <Icons.Flag size={22} weight="fill" aria-hidden="true" />
                <Heading level={1}>Skotos</Heading>
              </Stack>
              <Text tone="muted" size="sm">
                Sign in to the Skotos Feature Flag Control Plane.
              </Text>
            </Stack>

            {error && (
              <Alert tone="danger" title="Authentication failed">
                {error}
              </Alert>
            )}

            {authConfig?.mode === "external" ? (
              <Stack gap={3}>
                <Text size="sm" tone="secondary">
                  This workspace uses external SSO ({authConfig.oidcIssuer}).
                </Text>
                <Button
                  variant="primary"
                  onClick={() => {
                    if (authConfig.oidcIssuer) {
                      window.location.assign(authConfig.oidcIssuer);
                    }
                  }}
                >
                  Continue with SSO
                </Button>
              </Stack>
            ) : (
              <form onSubmit={handleSubmit}>
                <Stack gap={4}>
                  <FormField label="Email" htmlFor="login-email" required>
                    <Input
                      id="login-email"
                      type="email"
                      autoComplete="email"
                      value={email}
                      onChange={(e) => setEmail(e.target.value)}
                      required
                    />
                  </FormField>
                  <FormField label="Password" htmlFor="login-password" required>
                    <Input
                      id="login-password"
                      type="password"
                      autoComplete="current-password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      required
                    />
                  </FormField>
                  <Button type="submit" variant="primary" disabled={login.isPending}>
                    {login.isPending ? "Signing in..." : "Sign in"}
                  </Button>
                </Stack>
              </form>
            )}
          </Stack>
        </Card>
      </div>
    </div>
  );
}
