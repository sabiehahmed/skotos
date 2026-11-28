import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";
import {
  Alert,
  AlertDialog,
  AppShell,
  AuditTimeline,
  Badge,
  Breadcrumb,
  Button,
  CANONICAL_CLAUSE_OPS,
  Card,
  Checkbox,
  CodeBlock,
  Combobox,
  CommandPalette,
  DataTable,
  Dialog,
  DiffViewer,
  Drawer,
  DropdownMenu,
  EmptyState,
  EnvironmentBanner,
  FormField,
  Grid,
  Heading,
  IconButton,
  Icons,
  Input,
  JsonEditor,
  KeyValueEditor,
  Label,
  PageHeader,
  Pagination,
  Popover,
  RolloutAllocator,
  RuleBuilder,
  Select,
  Separator,
  Sidebar,
  Skeleton,
  Slider,
  Spinner,
  SplitPane,
  Stack,
  StatusDot,
  SuggestInput,
  Switch,
  Tabs,
  TagInput,
  Text,
  Textarea,
  ThemeProvider,
  ToastProvider,
  ToggleGroup,
  Tooltip,
  TooltipProvider,
  TopBar,
  useTheme,
  useToast,
} from "../index";

function ThemeToggleHarness() {
  const { resolvedTheme, toggleTheme } = useTheme();
  const { toast } = useToast();
  return (
    <Stack direction="horizontal" gap={2}>
      <Button onClick={toggleTheme}>Theme: {resolvedTheme}</Button>
      <Button onClick={() => toast({ title: "Saved", tone: "success" })}>Notify</Button>
    </Stack>
  );
}

describe("@skotos/ui full component library", () => {
  it("renders shell, theme toggle, environment banner, and toast with zero axe violations", async () => {
    const user = userEvent.setup();
    const { container } = render(
      <ThemeProvider defaultTheme="light">
        <TooltipProvider>
          <ToastProvider>
            <AppShell
              envColor="#EF4444"
              sidebar={
                <Sidebar header={<Text bold>Skotos</Text>}>
                  <Button variant="ghost">Flags</Button>
                </Sidebar>
              }
              topBar={
                <TopBar
                  left={<Breadcrumb items={[{ label: "Project" }, { label: "Flags" }]} />}
                  right={<ThemeToggleHarness />}
                />
              }
              banner={
                <EnvironmentBanner
                  environmentName="Production"
                  environmentKey="production"
                  color="#EF4444"
                  protectedEnv
                />
              }
            >
              <PageHeader
                title={<Heading level={1}>Feature Flags</Heading>}
                description={<Text tone="muted">Manage targeting across environments</Text>}
                actions={
                  <Tooltip content="Add a flag">
                    <Button variant="primary">Create Flag</Button>
                  </Tooltip>
                }
              />
              <SplitPane
                primary={
                  <Card header={<Text bold>Primary</Text>}>
                    <Stack gap={3}>
                      <Alert tone="warning" title="Protected">
                        Requires review comment
                      </Alert>
                      <Badge tone="success">
                        <StatusDot tone="success" /> Active
                      </Badge>
                      <CodeBlock code='{"on":true}' label="Snapshot" />
                      <Separator />
                      <Grid columns={2}>
                        <Skeleton height={24} />
                        <Spinner />
                      </Grid>
                    </Stack>
                  </Card>
                }
                secondary={
                  <EmptyState
                    icon={<Icons.Flag size={20} />}
                    title="No selection"
                    description="Pick a flag"
                  />
                }
              />
            </AppShell>
          </ToastProvider>
        </TooltipProvider>
      </ThemeProvider>,
    );

    expect(screen.getByTestId("environment-banner")).toHaveTextContent("Production");
    const themeBtn = screen.getByRole("button", { name: /Theme: light/i });
    await user.click(themeBtn);
    expect(screen.getByRole("button", { name: /Theme: dark/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Notify" }));
    expect(await screen.findByText("Saved")).toBeInTheDocument();

    const axeResults = await axe.run(container);
    expect(axeResults.violations).toEqual([]);
  });

  it("renders interactive forms, tags, key-values, sliders, switches, and checkboxes", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    const onCheck = vi.fn();
    const onTags = vi.fn();
    const onPairs = vi.fn();
    const onSelect = vi.fn();
    const onCombo = vi.fn();

    render(
      <Stack gap={3}>
        <FormField label="Flag Name" htmlFor="flag-name" required hint="Human readable">
          <Input id="flag-name" defaultValue="Checkout" />
        </FormField>
        <Label htmlFor="flag-desc">Description</Label>
        <Textarea id="flag-desc" defaultValue="Rollout" />
        <Switch checked={false} onCheckedChange={onSwitch} label="Targeting ON" />
        <Checkbox checked={false} onCheckedChange={onCheck} label="Temporary" />
        <Select
          ariaLabel="Flag kind"
          value="boolean"
          onValueChange={onSelect}
          options={[
            { value: "boolean", label: "Boolean" },
            { value: "string", label: "String" },
          ]}
        />
        <Combobox
          ariaLabel="Choose environment"
          value="dev"
          onChange={onCombo}
          options={[
            { value: "dev", label: "Development" },
            { value: "prod", label: "Production" },
          ]}
        />
        <ToggleGroup
          value="form"
          onValueChange={() => {}}
          options={[
            { value: "form", label: "Form" },
            { value: "json", label: "JSON" },
          ]}
        />
        <Slider value={50} onValueChange={() => {}} ariaLabel="Rollout pct" />
        <TagInput values={["alpha"]} onChange={onTags} ariaLabel="Tags" />
        <KeyValueEditor pairs={[{ key: "plan", value: "pro" }]} onChange={onPairs} />
        <IconButton label="Delete item">
          <Icons.Trash2 size={14} />
        </IconButton>
      </Stack>,
    );

    await user.click(screen.getByRole("switch", { name: "Targeting ON" }));
    expect(onSwitch).toHaveBeenCalledWith(true);

    await user.click(screen.getByRole("checkbox", { name: "Temporary" }));
    expect(onCheck).toHaveBeenCalledWith(true);

    const tagInput = screen.getByLabelText("Tags");
    await user.type(tagInput, "beta{Enter}");
    expect(onTags).toHaveBeenCalledWith(["alpha", "beta"]);

    await user.click(screen.getByRole("button", { name: /Add attribute/i }));
    expect(onPairs).toHaveBeenCalledWith([
      { key: "plan", value: "pro" },
      { key: "", value: "" },
    ]);
  });

  it("SuggestInput lists presets on focus, filters as you type, and keeps custom text", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const suggestions = [
      { value: "key", description: "unique id", group: "Built-in" },
      { value: "email", group: "Built-in" },
      { value: "plan", group: "Seen from SDKs" },
    ];
    const { rerender } = render(
      <SuggestInput ariaLabel="Attribute" value="" onChange={onChange} suggestions={suggestions} />,
    );

    await user.click(screen.getByRole("combobox", { name: "Attribute" }));
    expect(screen.getAllByRole("option")).toHaveLength(3);
    expect(screen.getByText("Seen from SDKs")).toBeInTheDocument();

    await user.click(screen.getByRole("option", { name: /email/ }));
    expect(onChange).toHaveBeenLastCalledWith("email");

    rerender(
      <SuggestInput ariaLabel="Attribute" value="" onChange={onChange} suggestions={suggestions} />,
    );
    const input = screen.getByRole("combobox", { name: "Attribute" });
    await user.click(input);
    await user.type(input, "p");
    expect(onChange).toHaveBeenLastCalledWith("p");

    rerender(
      <SuggestInput ariaLabel="Attribute" value="p" onChange={onChange} suggestions={suggestions} />,
    );
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["plan"]);
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onChange).toHaveBeenLastCalledWith("plan");
  });

  it("TagInput offers suggestions and skips values already added", async () => {
    const user = userEvent.setup();
    const onTags = vi.fn();
    render(
      <TagInput
        ariaLabel="Segments"
        values={["beta"]}
        onChange={onTags}
        suggestions={[{ value: "beta" }, { value: "internal" }]}
      />,
    );
    await user.click(screen.getByRole("combobox", { name: "Segments" }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["internal"]);
    await user.click(screen.getByRole("option", { name: "internal" }));
    expect(onTags).toHaveBeenCalledWith(["beta", "internal"]);
  });

  it("RuleBuilder offers built-in context kinds and per-kind attributes", async () => {
    const user = userEvent.setup();
    const onRules = vi.fn();
    render(
      <RuleBuilder
        rules={[
          {
            id: "r1",
            clauses: [{ contextKind: "device", attribute: "", op: "in", values: [], negate: false }],
          },
        ]}
        variations={[{ id: "on", label: "On" }]}
        onChange={onRules}
        contextKindSuggestions={[{ value: "tenant", group: "Seen from SDKs" }]}
        attributeSuggestions={{ device: [{ value: "region", group: "Seen from SDKs" }] }}
      />,
    );

    await user.click(screen.getByRole("combobox", { name: "Rule 1 clause 1 context kind" }));
    const kinds = screen.getAllByRole("option").map((o) => o.textContent);
    expect(kinds.some((k) => k?.startsWith("user"))).toBe(true);
    expect(kinds).toContain("tenant");

    await user.click(screen.getByRole("combobox", { name: "Rule 1 clause 1 attribute" }));
    const attrs = screen.getAllByRole("option").map((o) => o.textContent ?? "");
    expect(attrs.some((a) => a.startsWith("key"))).toBe(true);
    expect(attrs.some((a) => a.startsWith("osVersion"))).toBe(true);
    expect(attrs).toContain("region");
    expect(attrs.some((a) => a.startsWith("email"))).toBe(false);

    await user.click(screen.getByRole("option", { name: /osVersion/ }));
    expect(onRules).toHaveBeenLastCalledWith([
      expect.objectContaining({
        clauses: [expect.objectContaining({ contextKind: "device", attribute: "osVersion" })],
      }),
    ]);
  });

  it("supports RuleBuilder (15 canonical ops), RolloutAllocator (100,000 bps), DataTable, JsonEditor, DiffViewer, and AuditTimeline", async () => {
    expect(CANONICAL_CLAUSE_OPS).toHaveLength(15);

    const user = userEvent.setup();
    const onRules = vi.fn();
    const onRollout = vi.fn();
    const onJson = vi.fn();

    render(
      <Stack gap={4}>
        <RuleBuilder
          rules={[]}
          variations={[
            { id: "v1", label: "On (true)" },
            { id: "v2", label: "Off (false)" },
          ]}
          onChange={onRules}
        />
        <RolloutAllocator
          value={{
            contextKind: "user",
            variations: [
              { variationId: "v1", weight: 50000 },
              { variationId: "v2", weight: 50000 },
            ],
          }}
          variations={[
            { id: "v1", label: "On" },
            { id: "v2", label: "Off" },
          ]}
          onChange={onRollout}
        />
        <DataTable
          columns={[{ key: "k", header: "Key", render: (r: { key: string }) => r.key }]}
          rows={[{ key: "checkout-v2" }]}
          rowKey={(r) => r.key}
        />
        <JsonEditor value='{"kind":"user","key":"u1"}' onChange={onJson} />
        <DiffViewer
          summaries={[{ kind: "turnFlagOn", summary: "Turned targeting ON" }]}
          before={{ on: false }}
          after={{ on: true }}
        />
        <AuditTimeline
          entries={[
            {
              id: "a1",
              action: "patch",
              resourceType: "flag",
              resourceKey: "checkout-v2",
              actor: "admin@skotos.local",
              timestamp: "2026-11-27T12:00:00Z",
              comment: "Enable beta",
            },
          ]}
        />
      </Stack>,
    );

    await user.click(screen.getByRole("button", { name: /Add targeting rule/i }));
    expect(onRules).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/100\.000%/)).toBeInTheDocument();
    expect(screen.getByText("Valid JSON")).toBeInTheDocument();
    expect(screen.getByText("Turned targeting ON")).toBeInTheDocument();
  });

  it("renders overlays: Dialog, AlertDialog, Drawer, Popover, DropdownMenu, Tabs, Pagination, CommandPalette", async () => {
    const onConfirm = vi.fn();
    render(
      <Stack gap={3}>
        <Dialog open={false} onOpenChange={() => {}} title="Hidden Dialog">
          Body
        </Dialog>
        <AlertDialog
          open
          onOpenChange={() => {}}
          title="Delete Flag"
          description="Irreversible action"
          onConfirm={onConfirm}
        />
        <Drawer open={false} onOpenChange={() => {}} title="Details">
          Drawer
        </Drawer>
        <Popover trigger={<Button>Open Popover</Button>}>Pop body</Popover>
        <DropdownMenu
          trigger={<Button>Menu</Button>}
          items={[{ label: "Archive", onSelect: () => {} }]}
        />
        <Tabs
          items={[
            { value: "t1", label: "Targeting", content: <Text>Tab 1 content</Text> },
            { value: "t2", label: "History", content: <Text>Tab 2 content</Text> },
          ]}
        />
        <Pagination hasNext summary="Page 1" onNext={() => {}} />
        <CommandPalette open={false} onOpenChange={() => {}} items={[]} />
      </Stack>,
    );

    expect(screen.getByText("Delete Flag")).toBeInTheDocument();
    expect(screen.getByText("Tab 1 content")).toBeInTheDocument();
  });
});
