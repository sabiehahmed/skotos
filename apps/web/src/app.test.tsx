import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { App, WEB_APP_NAME } from "./App";

const mockOrg = {
  id: "11111111-1111-1111-1111-111111111111",
  slug: "default",
  name: "Skotos Local",
  planTier: "enterprise",
  createdAt: "2026-11-27T00:00:00Z",
};

const mockMe = {
  organization: mockOrg,
  actorType: "member",
  memberId: "22222222-2222-2222-2222-222222222222",
  email: "admin@skotos.local",
  displayName: "Skotos Admin",
  role: "owner",
};

const mockEnvs = [
  {
    id: "33333333-3333-3333-3333-333333333331",
    projectId: "44444444-4444-4444-4444-444444444441",
    key: "development",
    name: "Development",
    color: "#10B981",
    clientSideId: "sk_cid_dev123",
    requireComments: false,
    confirmChanges: false,
    revision: 1,
    createdAt: "2026-11-27T00:00:00Z",
  },
  {
    id: "33333333-3333-3333-3333-333333333332",
    projectId: "44444444-4444-4444-4444-444444444441",
    key: "production",
    name: "Production",
    color: "#EF4444",
    clientSideId: "sk_cid_prod123",
    requireComments: true,
    confirmChanges: true,
    revision: 1,
    createdAt: "2026-11-27T00:00:00Z",
  },
];

const onVarId = "55555555-5555-5555-5555-555555555551";
const offVarId = "55555555-5555-5555-5555-555555555552";

const mockFlag = {
  id: "66666666-6666-6666-6666-666666666661",
  projectId: "44444444-4444-4444-4444-444444444441",
  key: "express-checkout",
  name: "Express Checkout",
  description: "One-click checkout flow",
  kind: "boolean",
  variations: [
    { id: onVarId, value: true, name: "Enabled" },
    { id: offVarId, value: false, name: "Disabled" },
  ],
  temporary: true,
  clientSideAvailability: { usingEnvironmentId: false, usingMobileKey: false },
  tags: ["checkout"],
  archived: false,
  createdAt: "2026-11-27T00:00:00Z",
  updatedAt: "2026-11-27T00:00:00Z",
  environments: {
    development: {
      flagId: "66666666-6666-6666-6666-666666666661",
      envId: "33333333-3333-3333-3333-333333333331",
      version: 1,
      on: false,
      salt: "s1",
      config: {
        offVariationId: offVarId,
        prerequisites: [],
        targets: [],
        rules: [],
        fallthrough: { variationId: onVarId },
        trackEvents: false,
        trackEventsFallthrough: false,
      },
      updatedAt: "2026-11-27T00:00:00Z",
    },
    production: {
      flagId: "66666666-6666-6666-6666-666666666661",
      envId: "33333333-3333-3333-3333-333333333332",
      version: 1,
      on: false,
      salt: "s2",
      config: {
        offVariationId: offVarId,
        prerequisites: [],
        targets: [],
        rules: [],
        fallthrough: { variationId: onVarId },
        trackEvents: false,
        trackEventsFallthrough: false,
      },
      updatedAt: "2026-11-27T00:00:00Z",
    },
  },
};

let loggedIn = true;

const server = setupServer(
  http.get("*/api/v1/auth/config", () => HttpResponse.json({ mode: "self_hosted" })),
  http.get("*/api/v1/auth/me", () =>
    loggedIn
      ? HttpResponse.json(mockMe)
      : HttpResponse.json(
          { type: "about:blank", title: "Unauthorized", status: 401, detail: "Auth required", code: "unauthenticated" },
          { status: 401 },
        ),
  ),
  http.post("*/api/v1/auth/login", () => {
    loggedIn = true;
    return HttpResponse.json(mockMe);
  }),
  http.get("*/api/v1/projects", () =>
    HttpResponse.json({
      items: [
        {
          id: "44444444-4444-4444-4444-444444444441",
          orgId: mockOrg.id,
          key: "default",
          name: "Default Project",
          defaultClientSideAvailability: { usingEnvironmentId: false, usingMobileKey: false },
          createdAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/projects/:projectKey/environments", () =>
    HttpResponse.json({ items: mockEnvs }),
  ),
  http.get("*/api/v1/projects/:projectKey/flags", () =>
    HttpResponse.json({ items: [mockFlag] }),
  ),
  http.get("*/api/v1/projects/:projectKey/flags/:flagKey", () =>
    HttpResponse.json(mockFlag),
  ),
  http.get("*/api/v1/projects/:projectKey/flags/:flagKey/environments/:envKey/revisions", () =>
    HttpResponse.json({ items: [] }),
  ),
  http.get("*/api/v1/projects/:projectKey/flags/:flagKey/environments/:envKey/usage", () =>
    HttpResponse.json({
      flagKey: "express-checkout",
      totalEvaluations: 1240,
      variations: [
        { variationIndex: 0, count: 940 },
        { variationIndex: 1, count: 300 },
      ],
      contextKinds: ["user"],
      lastEvaluatedAt: "2026-11-27T12:00:00Z",
    }),
  ),
  http.get("*/api/v1/projects/:projectKey/flags/:flagKey/environments/:envKey/schedules", () =>
    HttpResponse.json({ items: [] }),
  ),
  http.get("*/api/v1/projects/:projectKey/flags/:flagKey/environments/:envKey/triggers", () =>
    HttpResponse.json({ items: [] }),
  ),
  http.get("*/api/v1/projects/:projectKey/environments/:envKey/segments", () =>
    HttpResponse.json({
      items: [
        {
          id: "77777777-7777-7777-7777-777777777771",
          envId: mockEnvs[0]!.id,
          key: "beta-testers",
          name: "Beta Testers",
          kind: "standard",
          version: 2,
          salt: "seg-salt",
          included: ["usr_beta_1", "usr_beta_2"],
          excluded: [],
          includedContexts: [],
          excludedContexts: [],
          rules: [],
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/webhooks", () =>
    HttpResponse.json({
      items: [
        {
          id: "88888888-8888-8888-8888-888888888881",
          orgId: mockOrg.id,
          name: "Datadog Flag Events",
          url: "https://hooks.example.com/skotos",
          enabled: true,
          events: ["flag.updated"],
          envFilter: ["production"],
          createdAt: "2026-11-27T00:00:00Z",
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.post("*/api/v1/projects/:projectKey/flags/:flagKey/environments/:envKey/simulate", () =>
    HttpResponse.json({
      flagKey: "express-checkout",
      environmentKey: "development",
      value: true,
      variationIndex: 0,
      variationId: onVarId,
      variationName: "Enabled",
      reason: { kind: "FALLTHROUGH" },
      usedDraft: true,
    }),
  ),
  http.get("*/api/v1/projects/:projectKey/flags/:flagKey/code-refs", () =>
    HttpResponse.json({
      items: [
        {
          flagKey: "express-checkout",
          repoName: "acme/storefront",
          branch: "main",
          commitSha: "abcdef1234567890",
          referenceCount: 1,
          extinct: false,
          hunks: [
            {
              flagKey: "express-checkout",
              filePath: "src/checkout.ts",
              lineNumber: 42,
              linePreview: 'const enabled = client.boolVariation("express-checkout", ctx, false);',
            },
          ],
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/projects/:projectKey/flags/:flagKey/environments/:envKey/workflows", () =>
    HttpResponse.json({ items: [] }),
  ),
  http.get("*/api/v1/approvals", () =>
    HttpResponse.json({
      items: [
        {
          id: "99999999-9999-9999-9999-999999999991",
          orgId: mockOrg.id,
          projectKey: "default",
          envKey: "production",
          flagKey: "express-checkout",
          status: "pending",
          description: "Enable express checkout for Q4",
          comment: "Reviewed by payments team",
          instructions: [{ kind: "turnFlagOn" }],
          minApprovals: 1,
          approvalCount: 0,
          requestedByEmail: "owner@acme.test",
          reviews: [],
          createdAt: "2026-11-27T00:00:00Z",
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/projects/:projectKey/release-pipelines", () =>
    HttpResponse.json({
      items: [
        {
          id: "99999999-9999-9999-9999-999999999992",
          orgId: mockOrg.id,
          projectId: "22222222-2222-2222-2222-222222222222",
          key: "standard-release",
          name: "Standard Progressive Rollout",
          description: "Dev -> Prod",
          phases: [
            { name: "Dev Rollout", environmentKey: "development", copyFromPreviousEnv: false, requireApproval: false },
            { name: "Prod Release", environmentKey: "production", copyFromPreviousEnv: true, requireApproval: true },
          ],
          createdAt: "2026-11-27T00:00:00Z",
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/applications", () =>
    HttpResponse.json({
      items: [
        {
          id: "99999999-9999-9999-9999-999999999993",
          orgId: mockOrg.id,
          key: "web-storefront",
          name: "Web Storefront",
          kind: "browser",
          description: "Primary SPA",
          versions: [],
          createdAt: "2026-11-27T00:00:00Z",
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/roles", () =>
    HttpResponse.json({
      items: [
        {
          id: "99999999-9999-9999-9999-999999999994",
          orgId: mockOrg.id,
          key: "prod-guard",
          name: "Production Guard",
          description: "Denies prod writes",
          statements: [
            { effect: "deny", actions: ["*"], resources: ["proj/*:env/production:flag/*"] },
          ],
          createdAt: "2026-11-27T00:00:00Z",
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/teams", () =>
    HttpResponse.json({
      items: [
        {
          id: "99999999-9999-9999-9999-999999999995",
          orgId: mockOrg.id,
          key: "checkout-eng",
          name: "Checkout Engineering",
          description: "Payments owners",
          customRoleKeys: ["prod-guard"],
          memberIds: [],
          createdAt: "2026-11-27T00:00:00Z",
          updatedAt: "2026-11-27T00:00:00Z",
        },
      ],
    }),
  ),
  http.get("*/api/v1/sso/providers", () => HttpResponse.json({ items: [] })),
  http.get("*/api/v1/audit-log/archives", () => HttpResponse.json({ items: [] })),
);

beforeAll(() => server.listen());
afterEach(() => {
  loggedIn = true;
  server.resetHandlers();
});
afterAll(() => server.close());

describe("@skotos/web management dashboard", () => {
  it("exports canonical app identifier", () => {
    expect(WEB_APP_NAME).toBe("@skotos/web");
  });

  it("renders login screen when unauthenticated and signs in", async () => {
    loggedIn = false;
    const user = userEvent.setup();
    render(<App initialEntries={["/"]} />);

    expect(await screen.findByRole("button", { name: "Sign in" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByText("Express Checkout")).toBeInTheDocument();
  });

  it("renders flags list, environment banner, simulator, and Phase 2/3 Usage & Automation tab", async () => {
    const user = userEvent.setup();
    render(<App initialEntries={["/projects/default/flags"]} />);

    expect(await screen.findByText("Express Checkout")).toBeInTheDocument();
    expect(screen.getByTestId("environment-banner")).toHaveTextContent("Development");

    await user.click(screen.getByText("Express Checkout"));
    expect(await screen.findByText("Test Context Simulator")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Evaluate Context/i }));
    await waitFor(() => {
      expect(screen.getByText("FALLTHROUGH")).toBeInTheDocument();
    });

    await user.click(screen.getByRole("tab", { name: "Usage & Automation" }));
    expect(await screen.findByText("Evaluation Usage (Last 30 Days)")).toBeInTheDocument();
    expect(await screen.findByText(/1,240 evaluations/i)).toBeInTheDocument();
    expect(screen.getByText("Flag Triggers (APM Kill-Switch URLs)")).toBeInTheDocument();
    expect(await screen.findByText("src/checkout.ts:42")).toBeInTheDocument();
  });

  it("navigates to Phase 2 & Phase 3 Segments, Webhooks, Pipelines, Approvals, Roles, and Teams views", async () => {
    const user = userEvent.setup();
    render(<App initialEntries={["/projects/default/flags"]} />);
    expect(await screen.findByText("Express Checkout")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Segments" }));
    expect(await screen.findByText("Beta Testers")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Release Pipelines" }));
    expect(await screen.findByText("Standard Progressive Rollout")).toBeInTheDocument();
    expect(await screen.findByText("Web Storefront")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Approvals" }));
    expect(await screen.findByText("Enable express checkout for Q4")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Webhooks" }));
    expect(await screen.findByText("Datadog Flag Events")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Custom Roles" }));
    expect(await screen.findByText("Production Guard")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Teams & SSO" }));
    expect(await screen.findByText("Checkout Engineering")).toBeInTheDocument();
  });
});
