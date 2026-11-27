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

  it("renders flags list, environment banner, and navigates to flag detail simulator", async () => {
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
  });
});
