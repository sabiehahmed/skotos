import clsx from "clsx";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "../../icons";
import type { ReactNode } from "react";
import {
  Button,
  Checkbox,
  IconButton,
  Input,
  Select,
  SuggestInput,
  TagInput,
  Textarea,
  type SelectOption,
  type SuggestOption,
} from "../forms/Forms";
import { Badge, CodeBlock, EmptyState, Text } from "../typography/Typography";
import styles from "./Data.module.css";

// =============================================================================
// DataTable
// =============================================================================

export interface DataTableColumn<T> {
  key: string;
  header: ReactNode;
  render: (row: T) => ReactNode;
  width?: string;
}

export interface DataTableProps<T> {
  columns: DataTableColumn<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  onRowClick?: (row: T) => void;
  emptyTitle?: string;
  emptyDescription?: string;
  emptyAction?: ReactNode;
  ariaLabel?: string;
}

export function DataTable<T>({
  columns,
  rows,
  rowKey,
  onRowClick,
  emptyTitle = "No records found",
  emptyDescription,
  emptyAction,
  ariaLabel,
}: DataTableProps<T>) {
  if (rows.length === 0) {
    return (
      <EmptyState title={emptyTitle} description={emptyDescription} action={emptyAction} />
    );
  }

  return (
    <div className={styles.tableWrap}>
      <table className={styles.table} aria-label={ariaLabel}>
        <thead>
          <tr>
            {columns.map((col) => (
              <th key={col.key} style={col.width ? { width: col.width } : undefined}>
                {col.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr
              key={rowKey(row)}
              className={clsx(onRowClick && styles.tableRowClickable)}
              onClick={onRowClick ? () => onRowClick(row) : undefined}
            >
              {columns.map((col) => (
                <td key={col.key}>{col.render(row)}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// =============================================================================
// JsonEditor
// =============================================================================

export interface JsonEditorProps {
  value: string;
  onChange: (value: string) => void;
  ariaLabel?: string;
  rows?: number;
  disabled?: boolean;
}

export function JsonEditor({
  value,
  onChange,
  ariaLabel = "JSON editor",
  rows = 8,
  disabled,
}: JsonEditorProps) {
  let parseError: string | null = null;
  try {
    JSON.parse(value);
  } catch (err) {
    parseError = err instanceof Error ? err.message : "Invalid JSON";
  }

  const handleFormat = () => {
    try {
      const formatted = JSON.stringify(JSON.parse(value), null, 2);
      onChange(formatted);
    } catch {
      // Ignore formatting when invalid JSON
    }
  };

  return (
    <div className={styles.jsonEditorWrap}>
      <Textarea
        aria-label={ariaLabel}
        rows={rows}
        mono
        invalid={parseError !== null}
        disabled={disabled}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
      <div className={styles.jsonStatus}>
        <span className={parseError ? styles.jsonInvalid : styles.jsonValid}>
          {parseError ? `Invalid JSON: ${parseError}` : "Valid JSON"}
        </span>
        {!disabled && !parseError && (
          <Button size="sm" variant="ghost" onClick={handleFormat}>
            Format JSON
          </Button>
        )}
      </div>
    </div>
  );
}

// =============================================================================
// DiffViewer
// =============================================================================

export interface DiffInstructionSummary {
  kind: string;
  summary: string;
}

export interface DiffViewerProps {
  summaries?: DiffInstructionSummary[];
  before?: unknown;
  after?: unknown;
}

export function DiffViewer({ summaries, before, after }: DiffViewerProps) {
  return (
    <div className={styles.diffList}>
      {summaries && summaries.length > 0 && (
        <div className={styles.diffList}>
          {summaries.map((item, idx) => (
            <div key={`${item.kind}-${idx}`} className={styles.diffItem}>
              <Badge tone="accent">{item.kind}</Badge>
              <span>{item.summary}</span>
            </div>
          ))}
        </div>
      )}
      {(before !== undefined || after !== undefined) && (
        <div className={styles.diffJsonGrid}>
          {before !== undefined && (
            <CodeBlock
              label="Before"
              copyable={false}
              code={JSON.stringify(before, null, 2)}
            />
          )}
          {after !== undefined && (
            <CodeBlock
              label="After"
              copyable={false}
              code={JSON.stringify(after, null, 2)}
            />
          )}
        </div>
      )}
    </div>
  );
}

// =============================================================================
// RolloutAllocator (exact integer basis points summing to 100,000)
// =============================================================================

export interface RolloutVariationOption {
  id: string;
  label: string;
}

export interface RolloutWeightedVariation {
  variationId: string;
  weight: number;
}

export interface RolloutValue {
  contextKind: string;
  bucketBy?: string;
  variations: RolloutWeightedVariation[];
}

export interface RolloutAllocatorProps {
  value: RolloutValue;
  variations: RolloutVariationOption[];
  onChange: (next: RolloutValue) => void;
  disabled?: boolean;
}

const ROLLOUT_COLORS = ["#2563eb", "#10b981", "#f59e0b", "#8b5cf6", "#ec4899", "#06b6d4"];

export function RolloutAllocator({
  value,
  variations,
  onChange,
  disabled,
}: RolloutAllocatorProps) {
  const total = value.variations.reduce((acc, v) => acc + v.weight, 0);
  const valid = total === 100_000;

  const setWeightPercent = (variationId: string, pctStr: string) => {
    const parsed = Number.parseFloat(pctStr);
    const basisPoints = Number.isFinite(parsed)
      ? Math.max(0, Math.min(100_000, Math.round(parsed * 1000)))
      : 0;
    const exists = value.variations.some((v) => v.variationId === variationId);
    const nextVariations = exists
      ? value.variations.map((v) =>
          v.variationId === variationId ? { ...v, weight: basisPoints } : v,
        )
      : [...value.variations, { variationId, weight: basisPoints }];
    onChange({ ...value, variations: nextVariations });
  };

  return (
    <div className={styles.rolloutWrap}>
      <div className={styles.rolloutBar} aria-label="Rollout distribution bar">
        {variations.map((opt, idx) => {
          const wt = value.variations.find((v) => v.variationId === opt.id)?.weight ?? 0;
          const pct = (wt / 1000).toFixed(3);
          return (
            <div
              key={opt.id}
              style={{
                width: `${Math.min(100, wt / 1000)}%`,
                background: ROLLOUT_COLORS[idx % ROLLOUT_COLORS.length],
              }}
              title={`${opt.label}: ${pct}%`}
            />
          );
        })}
      </div>

      {variations.map((opt, idx) => {
        const wt = value.variations.find((v) => v.variationId === opt.id)?.weight ?? 0;
        const pctDisplay = String(Number((wt / 1000).toFixed(3)));
        return (
          <div key={opt.id} className={styles.rolloutRow}>
            <span
              className={styles.rolloutSwatch}
              style={{ background: ROLLOUT_COLORS[idx % ROLLOUT_COLORS.length] }}
            />
            <Text as="span" size="sm">
              {opt.label}
            </Text>
            <Input
              type="number"
              min={0}
              max={100}
              step={0.1}
              disabled={disabled}
              aria-label={`Percentage for ${opt.label}`}
              value={pctDisplay}
              onChange={(e) => setWeightPercent(opt.id, e.target.value)}
            />
            <Text as="span" size="xs" tone="muted" mono>
              % ({wt})
            </Text>
          </div>
        );
      })}

      <div className={styles.rolloutTotal}>
        <span style={{ color: valid ? "var(--sk-color-success)" : "var(--sk-color-danger)" }}>
          Total: {(total / 1000).toFixed(3)}% ({total} / 100,000 bps)
        </span>
        {!valid && (
          <span style={{ color: "var(--sk-color-danger)" }}>Weights must sum to 100%</span>
        )}
      </div>
    </div>
  );
}

// =============================================================================
// RuleBuilder (exact 15 canonical operators locked in PROJECT.md)
// =============================================================================

export const CANONICAL_CLAUSE_OPS: SelectOption[] = [
  { value: "in", label: "is one of (in)" },
  { value: "startsWith", label: "starts with" },
  { value: "endsWith", label: "ends with" },
  { value: "contains", label: "contains" },
  { value: "matches", label: "matches regex (RE2)" },
  { value: "lessThan", label: "< (less than)" },
  { value: "lessThanOrEqual", label: "<= (less or equal)" },
  { value: "greaterThan", label: "> (greater than)" },
  { value: "greaterThanOrEqual", label: ">= (greater or equal)" },
  { value: "before", label: "before (date)" },
  { value: "after", label: "after (date)" },
  { value: "semverEqual", label: "semver =" },
  { value: "semverLessThan", label: "semver <" },
  { value: "semverGreaterThan", label: "semver >" },
  { value: "segmentMatch", label: "in segment" },
];

export interface RuleBuilderClause {
  contextKind: string;
  attribute: string;
  op: string;
  values: string[];
  negate: boolean;
}

export interface RuleBuilderRule {
  id: string;
  description?: string;
  clauses: RuleBuilderClause[];
  variationId?: string;
  rollout?: RolloutValue;
}

const BUILT_IN_GROUP = "Built-in";

/** Context kinds offered in the rule clause dropdown before any SDK data is known. */
export const BUILT_IN_CONTEXT_KINDS: SuggestOption[] = [
  { value: "user", description: "default", group: BUILT_IN_GROUP },
  { value: "organization", group: BUILT_IN_GROUP },
  { value: "device", group: BUILT_IN_GROUP },
];

const COMMON_ATTRIBUTES: SuggestOption[] = [
  { value: "key", description: "unique id", group: BUILT_IN_GROUP },
  { value: "name", group: BUILT_IN_GROUP },
  { value: "anonymous", description: "boolean", group: BUILT_IN_GROUP },
];

const KIND_ATTRIBUTES: Record<string, SuggestOption[]> = {
  user: [
    { value: "email", group: BUILT_IN_GROUP },
    { value: "firstName", group: BUILT_IN_GROUP },
    { value: "lastName", group: BUILT_IN_GROUP },
    { value: "country", group: BUILT_IN_GROUP },
    { value: "ip", group: BUILT_IN_GROUP },
  ],
  device: [
    { value: "osName", group: BUILT_IN_GROUP },
    { value: "osVersion", description: "semver", group: BUILT_IN_GROUP },
    { value: "appVersion", description: "semver", group: BUILT_IN_GROUP },
  ],
};

/** Attributes offered for a context kind before any SDK data is known. */
export function builtInAttributeSuggestions(contextKind: string): SuggestOption[] {
  return [...COMMON_ATTRIBUTES, ...(KIND_ATTRIBUTES[contextKind.trim()] ?? [])];
}

export interface RuleBuilderProps {
  rules: RuleBuilderRule[];
  variations: RolloutVariationOption[];
  onChange: (rules: RuleBuilderRule[]) => void;
  disabled?: boolean;
  /** Extra context kinds (e.g. seen from SDKs), listed after the built-ins. */
  contextKindSuggestions?: SuggestOption[];
  /** Extra attributes per context kind (e.g. seen from SDKs), listed after the built-ins. */
  attributeSuggestions?: Record<string, SuggestOption[]>;
  /** Values offered in a clause's value field, e.g. segment keys or seen context keys. */
  getValueSuggestions?: (clause: RuleBuilderClause) => SuggestOption[];
}

export function RuleBuilder({
  rules,
  variations,
  onChange,
  disabled,
  contextKindSuggestions,
  attributeSuggestions,
  getValueSuggestions,
}: RuleBuilderProps) {
  const variationOptions: SelectOption[] = variations.map((v) => ({
    value: v.id,
    label: v.label,
  }));
  const kindOptions = [...BUILT_IN_CONTEXT_KINDS, ...(contextKindSuggestions ?? [])];
  const attributeOptionsFor = (kind: string) => [
    ...builtInAttributeSuggestions(kind),
    ...(attributeSuggestions?.[kind.trim()] ?? []),
  ];

  const addRule = () => {
    const firstVarId = variations[0]?.id;
    const next: RuleBuilderRule = {
      id: `rule-${Date.now().toString(36)}`,
      description: `Rule ${rules.length + 1}`,
      clauses: [
        {
          contextKind: "user",
          attribute: "email",
          op: "endsWith",
          values: [],
          negate: false,
        },
      ],
      variationId: firstVarId,
    };
    onChange([...rules, next]);
  };

  const updateRule = (idx: number, patch: Partial<RuleBuilderRule>) => {
    onChange(rules.map((r, i) => (i === idx ? { ...r, ...patch } : r)));
  };

  const moveRule = (idx: number, delta: -1 | 1) => {
    const target = idx + delta;
    if (target < 0 || target >= rules.length) return;
    const copy = [...rules];
    const [moved] = copy.splice(idx, 1);
    if (moved) {
      copy.splice(target, 0, moved);
      onChange(copy);
    }
  };

  return (
    <div className={styles.ruleList}>
      {rules.map((rule, ruleIdx) => {
        const serveMode = rule.rollout ? "rollout" : "variation";
        return (
          <div key={rule.id} className={styles.ruleCard}>
            <div className={styles.ruleHeader}>
              <Input
                aria-label={`Rule ${ruleIdx + 1} description`}
                value={rule.description ?? ""}
                placeholder={`Rule ${ruleIdx + 1}`}
                disabled={disabled}
                onChange={(e) => updateRule(ruleIdx, { description: e.target.value })}
                style={{ maxWidth: 280, height: 30 }}
              />
              <div style={{ display: "flex", gap: 4 }}>
                <IconButton
                  label={`Move rule ${ruleIdx + 1} up`}
                  size="sm"
                  disabled={disabled || ruleIdx === 0}
                  onClick={() => moveRule(ruleIdx, -1)}
                >
                  <ArrowUp size={14} aria-hidden="true" />
                </IconButton>
                <IconButton
                  label={`Move rule ${ruleIdx + 1} down`}
                  size="sm"
                  disabled={disabled || ruleIdx === rules.length - 1}
                  onClick={() => moveRule(ruleIdx, 1)}
                >
                  <ArrowDown size={14} aria-hidden="true" />
                </IconButton>
                <IconButton
                  label={`Remove rule ${ruleIdx + 1}`}
                  size="sm"
                  variant="ghost"
                  disabled={disabled}
                  onClick={() => onChange(rules.filter((_, i) => i !== ruleIdx))}
                >
                  <Trash2 size={14} aria-hidden="true" />
                </IconButton>
              </div>
            </div>

            <div className={styles.ruleBody}>
              {rule.clauses.map((clause, cIdx) => (
                <div key={cIdx} className={styles.clauseRow}>
                  <SuggestInput
                    ariaLabel={`Rule ${ruleIdx + 1} clause ${cIdx + 1} context kind`}
                    value={clause.contextKind}
                    suggestions={kindOptions}
                    placeholder="user"
                    disabled={disabled || clause.op === "segmentMatch"}
                    mono
                    onChange={(next) => {
                      const nextClauses = rule.clauses.map((c, j) =>
                        j === cIdx ? { ...c, contextKind: next } : c,
                      );
                      updateRule(ruleIdx, { clauses: nextClauses });
                    }}
                  />
                  <SuggestInput
                    ariaLabel={`Rule ${ruleIdx + 1} clause ${cIdx + 1} attribute`}
                    value={clause.op === "segmentMatch" ? "" : clause.attribute}
                    suggestions={attributeOptionsFor(clause.contextKind)}
                    placeholder={
                      clause.op === "segmentMatch" ? "not used" : "email or /address/country"
                    }
                    disabled={disabled || clause.op === "segmentMatch"}
                    mono
                    onChange={(next) => {
                      const nextClauses = rule.clauses.map((c, j) =>
                        j === cIdx ? { ...c, attribute: next } : c,
                      );
                      updateRule(ruleIdx, { clauses: nextClauses });
                    }}
                  />
                  <Select
                    ariaLabel={`Rule ${ruleIdx + 1} clause ${cIdx + 1} operator`}
                    value={clause.op}
                    options={CANONICAL_CLAUSE_OPS}
                    disabled={disabled}
                    onValueChange={(nextOp) => {
                      const nextClauses = rule.clauses.map((c, j) =>
                        j === cIdx ? { ...c, op: nextOp } : c,
                      );
                      updateRule(ruleIdx, { clauses: nextClauses });
                    }}
                  />
                  <TagInput
                    ariaLabel={`Rule ${ruleIdx + 1} clause ${cIdx + 1} values`}
                    values={clause.values}
                    disabled={disabled}
                    suggestions={getValueSuggestions?.(clause)}
                    placeholder={clause.op === "segmentMatch" ? "Add segment..." : "Add value..."}
                    onChange={(vals) => {
                      const nextClauses = rule.clauses.map((c, j) =>
                        j === cIdx ? { ...c, values: vals } : c,
                      );
                      updateRule(ruleIdx, { clauses: nextClauses });
                    }}
                  />
                  <Checkbox
                    checked={clause.negate}
                    label="NOT"
                    disabled={disabled}
                    onCheckedChange={(neg) => {
                      const nextClauses = rule.clauses.map((c, j) =>
                        j === cIdx ? { ...c, negate: neg } : c,
                      );
                      updateRule(ruleIdx, { clauses: nextClauses });
                    }}
                  />
                  <IconButton
                    label={`Remove clause ${cIdx + 1}`}
                    size="sm"
                    disabled={disabled || rule.clauses.length <= 1}
                    onClick={() =>
                      updateRule(ruleIdx, {
                        clauses: rule.clauses.filter((_, j) => j !== cIdx),
                      })
                    }
                  >
                    <Trash2 size={14} aria-hidden="true" />
                  </IconButton>
                </div>
              ))}

              {!disabled && (
                <div>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      updateRule(ruleIdx, {
                        clauses: [
                          ...rule.clauses,
                          {
                            contextKind: "user",
                            attribute: "key",
                            op: "in",
                            values: [],
                            negate: false,
                          },
                        ],
                      })
                    }
                  >
                    <Plus size={14} aria-hidden="true" />
                    Add AND clause
                  </Button>
                </div>
              )}

              <div className={styles.ruleServeRow}>
                <Text as="span" size="sm" bold>
                  Serve:
                </Text>
                <Select
                  ariaLabel={`Rule ${ruleIdx + 1} serve mode`}
                  value={serveMode}
                  disabled={disabled}
                  options={[
                    { value: "variation", label: "Single variation" },
                    { value: "rollout", label: "Percentage rollout" },
                  ]}
                  onValueChange={(mode) => {
                    if (mode === "rollout") {
                      const even = Math.floor(100_000 / Math.max(1, variations.length));
                      const rem = 100_000 - even * variations.length;
                      updateRule(ruleIdx, {
                        variationId: undefined,
                        rollout: {
                          contextKind: "user",
                          variations: variations.map((v, i) => ({
                            variationId: v.id,
                            weight: even + (i === 0 ? rem : 0),
                          })),
                        },
                      });
                    } else {
                      updateRule(ruleIdx, {
                        rollout: undefined,
                        variationId: variations[0]?.id,
                      });
                    }
                  }}
                />
                {serveMode === "variation" && (
                  <Select
                    ariaLabel={`Rule ${ruleIdx + 1} served variation`}
                    value={rule.variationId ?? variations[0]?.id}
                    options={variationOptions}
                    disabled={disabled}
                    onValueChange={(vid) => updateRule(ruleIdx, { variationId: vid })}
                  />
                )}
              </div>

              {serveMode === "rollout" && rule.rollout && (
                <RolloutAllocator
                  value={rule.rollout}
                  variations={variations}
                  disabled={disabled}
                  onChange={(nextRollout) => updateRule(ruleIdx, { rollout: nextRollout })}
                />
              )}
            </div>
          </div>
        );
      })}

      {!disabled && (
        <div>
          <Button variant="secondary" size="sm" onClick={addRule}>
            <Plus size={14} aria-hidden="true" />
            Add targeting rule
          </Button>
        </div>
      )}
    </div>
  );
}

// =============================================================================
// AuditTimeline
// =============================================================================

export interface AuditTimelineEntry {
  id: string;
  action: string;
  resourceType: string;
  resourceKey: string;
  actor: string;
  timestamp: string;
  comment?: string | null;
  summaries?: DiffInstructionSummary[];
  before?: unknown;
  after?: unknown;
  actionSlot?: ReactNode;
}

export interface AuditTimelineProps {
  entries: AuditTimelineEntry[];
  emptyTitle?: string;
}

export function AuditTimeline({
  entries,
  emptyTitle = "No audit events recorded yet",
}: AuditTimelineProps) {
  if (entries.length === 0) {
    return <EmptyState title={emptyTitle} />;
  }

  return (
    <div className={styles.timeline}>
      {entries.map((entry) => (
        <article key={entry.id} className={styles.timelineItem}>
          <div className={styles.timelineHeader}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <Badge tone="accent">{entry.action}</Badge>
              <Badge tone="neutral">{entry.resourceType}</Badge>
              <Text as="span" size="sm" mono bold>
                {entry.resourceKey}
              </Text>
              <Text as="span" size="xs" tone="muted">
                by {entry.actor}
              </Text>
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <Text as="span" size="xs" tone="muted" mono>
                {entry.timestamp}
              </Text>
              {entry.actionSlot}
            </div>
          </div>
          {entry.comment && (
            <Text size="sm" tone="secondary">
              “{entry.comment}”
            </Text>
          )}
          {(entry.summaries?.length || entry.before !== undefined || entry.after !== undefined) && (
            <DiffViewer
              summaries={entry.summaries}
              before={entry.before}
              after={entry.after}
            />
          )}
        </article>
      ))}
    </div>
  );
}

export interface DotMeterProps {
  /** Filled fraction, 0..1. Values outside the range are clamped. */
  value: number;
  /** Accessible name for the meter. */
  label: string;
  className?: string;
}

/**
 * Dot meter: a row of small squares, filled squares in the meter tone and the
 * empty track in black. Read-only data viz for ratios and percentages.
 */
export function DotMeter({ value, label, className }: DotMeterProps) {
  const clamped = Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
  const pct = Math.round(clamped * 1000) / 10;
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      className={clsx(styles.dotMeter, className)}
    >
      <span className={styles.dotMeterFill} style={{ width: `${pct}%` }} />
    </div>
  );
}
