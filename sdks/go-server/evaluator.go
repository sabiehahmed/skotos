package skotos

import (
	"slices"
	"sort"
	"strings"
)

// Evaluate executes the canonical 7-step Skotos evaluation algorithm.
func Evaluate(store EvalStore, flagKey string, ctx Context, defaultValue any) EvaluationDetail {
	if err := ctx.Validate(); err != nil {
		return errorDetail(defaultValue, "USER_NOT_SPECIFIED")
	}
	flag, ok := store.GetFlag(flagKey)
	if !ok || flag == nil {
		return errorDetail(defaultValue, "FLAG_NOT_FOUND")
	}
	var prereqStack []string
	return evaluateFlagInternal(store, flag, ctx, defaultValue, &prereqStack)
}

func BoolVariation(store EvalStore, flagKey string, ctx Context, defaultValue bool) EvaluationDetail {
	d := Evaluate(store, flagKey, ctx, defaultValue)
	if d.VariationIndex != nil {
		if _, ok := d.Value.(bool); !ok {
			return errorDetail(defaultValue, "WRONG_TYPE")
		}
	}
	return d
}

func StringVariation(store EvalStore, flagKey string, ctx Context, defaultValue string) EvaluationDetail {
	d := Evaluate(store, flagKey, ctx, defaultValue)
	if d.VariationIndex != nil {
		if _, ok := d.Value.(string); !ok {
			return errorDetail(defaultValue, "WRONG_TYPE")
		}
	}
	return d
}

func NumberVariation(store EvalStore, flagKey string, ctx Context, defaultValue float64) EvaluationDetail {
	d := Evaluate(store, flagKey, ctx, defaultValue)
	if d.VariationIndex != nil {
		if _, ok := toFloat64(d.Value); !ok {
			return errorDetail(defaultValue, "WRONG_TYPE")
		}
	}
	return d
}

func evaluateFlagInternal(
	store EvalStore,
	flag *EvalFlag,
	ctx Context,
	defaultValue any,
	prereqStack *[]string,
) EvaluationDetail {
	if !flag.On {
		return resolveOffVariation(flag, defaultValue, EvaluationReason{Kind: "OFF"})
	}

	if slices.Contains(*prereqStack, flag.Key) {
		return errorDetail(defaultValue, "MALFORMED_FLAG")
	}
	*prereqStack = append(*prereqStack, flag.Key)
	defer func() {
		*prereqStack = (*prereqStack)[:len(*prereqStack)-1]
	}()

	// Prerequisites
	for _, pre := range flag.Prerequisites {
		if slices.Contains(*prereqStack, pre.Key) {
			return errorDetail(defaultValue, "MALFORMED_FLAG")
		}
		preFlag, ok := store.GetFlag(pre.Key)
		if !ok || !preFlag.On {
			return resolveOffVariation(flag, defaultValue, EvaluationReason{
				Kind:            "PREREQUISITE_FAILED",
				PrerequisiteKey: pre.Key,
			})
		}
		preDetail := evaluateFlagInternal(store, preFlag, ctx, nil, prereqStack)
		if preDetail.Reason.Kind == "ERROR" && preDetail.Reason.ErrorKind == "MALFORMED_FLAG" {
			return errorDetail(defaultValue, "MALFORMED_FLAG")
		}
		if preDetail.VariationIndex == nil || *preDetail.VariationIndex != pre.Variation {
			return resolveOffVariation(flag, defaultValue, EvaluationReason{
				Kind:            "PREREQUISITE_FAILED",
				PrerequisiteKey: pre.Key,
			})
		}
	}

	// Individual context targets
	for _, t := range flag.Targets {
		kind := t.ContextKind
		if kind == "" {
			kind = "user"
		}
		if sc := ctx.GetKind(kind); sc != nil {
			if slices.Contains(t.Values, sc.Key) {
				return resolveVariation(flag, t.Variation, defaultValue, EvaluationReason{Kind: "TARGET_MATCH"})
			}
		}
	}

	// Targeting rules
	for idx, rule := range flag.Rules {
		allMatch := true
		var segStack []string
		for _, clause := range rule.Clauses {
			matched, valid := evalClause(store, clause, ctx, &segStack)
			if !valid {
				return errorDetail(defaultValue, "MALFORMED_FLAG")
			}
			if !matched {
				allMatch = false
				break
			}
		}
		if allMatch {
			ruleIdx := idx
			vor := EvalVariationOrRollout{Variation: rule.Variation, Rollout: rule.Rollout}
			return resolveVariationOrRollout(flag, vor, ctx, defaultValue, func(inExp bool) EvaluationReason {
				return EvaluationReason{
					Kind:         "RULE_MATCH",
					RuleIndex:    &ruleIdx,
					RuleID:       rule.ID,
					InExperiment: inExp,
				}
			})
		}
	}

	// Default fallthrough
	return resolveVariationOrRollout(flag, flag.Fallthrough, ctx, defaultValue, func(inExp bool) EvaluationReason {
		return EvaluationReason{
			Kind:         "FALLTHROUGH",
			InExperiment: inExp,
		}
	})
}

func resolveOffVariation(flag *EvalFlag, defaultValue any, reason EvaluationReason) EvaluationDetail {
	if flag.OffVariation == nil {
		return EvaluationDetail{Value: defaultValue, VariationIndex: nil, Reason: reason}
	}
	return resolveVariation(flag, *flag.OffVariation, defaultValue, reason)
}

func resolveVariation(flag *EvalFlag, index int, defaultValue any, reason EvaluationReason) EvaluationDetail {
	if index < 0 || index >= len(flag.Variations) {
		return errorDetail(defaultValue, "MALFORMED_FLAG")
	}
	idx := index
	return EvaluationDetail{
		Value:          flag.Variations[idx],
		VariationIndex: &idx,
		Reason:         reason,
	}
}

func resolveVariationOrRollout(
	flag *EvalFlag,
	vor EvalVariationOrRollout,
	ctx Context,
	defaultValue any,
	buildReason func(inExp bool) EvaluationReason,
) EvaluationDetail {
	if vor.Variation != nil {
		return resolveVariation(flag, *vor.Variation, defaultValue, buildReason(false))
	}
	if vor.Rollout != nil {
		res, ok := evaluateRollout(flag.Key, flag.Salt, vor.Rollout, ctx)
		if !ok {
			return errorDetail(defaultValue, "MALFORMED_FLAG")
		}
		return resolveVariation(flag, res.variationIndex, defaultValue, buildReason(res.inExperiment))
	}
	return errorDetail(defaultValue, "MALFORMED_FLAG")
}

func errorDetail(defaultValue any, errorKind string) EvaluationDetail {
	return EvaluationDetail{
		Value:          defaultValue,
		VariationIndex: nil,
		Reason: EvaluationReason{
			Kind:      "ERROR",
			ErrorKind: errorKind,
		},
	}
}

func evaluateSegment(store EvalStore, seg *EvalSegment, ctx Context, segStack *[]string) (bool, bool) {
	if slices.Contains(*segStack, seg.Key) {
		return false, false
	}
	if matchesKeyList(ctx, "user", seg.Included) {
		return true, true
	}
	for _, t := range seg.IncludedContexts {
		if matchesKeyList(ctx, t.ContextKind, t.Values) {
			return true, true
		}
	}
	if matchesKeyList(ctx, "user", seg.Excluded) {
		return false, true
	}
	for _, t := range seg.ExcludedContexts {
		if matchesKeyList(ctx, t.ContextKind, t.Values) {
			return false, true
		}
	}

	*segStack = append(*segStack, seg.Key)
	defer func() {
		*segStack = (*segStack)[:len(*segStack)-1]
	}()

	for _, rule := range seg.Rules {
		allMatch := true
		for _, clause := range rule.Clauses {
			matched, valid := evalClause(store, clause, ctx, segStack)
			if !valid {
				return false, false
			}
			if !matched {
				allMatch = false
				break
			}
		}
		if allMatch {
			if rule.Weight != nil {
				w := *rule.Weight
				if w == 0 {
					continue
				}
				if w >= 100_000 {
					return true, true
				}
				kind := "user"
				if rule.RolloutContextKind != nil && *rule.RolloutContextKind != "" {
					kind = *rule.RolloutContextKind
				}
				if bVal, sec, ok := extractBucketValue(ctx, kind, rule.BucketBy); ok {
					input := buildHashInput(seg.Key, seg.Salt, bVal, sec, nil)
					if HashToBucket(input) < uint64(w) {
						return true, true
					}
				}
			} else {
				return true, true
			}
		}
	}
	return false, true
}

func matchesKeyList(ctx Context, kind string, keys []string) bool {
	if len(keys) == 0 {
		return false
	}
	if kind == "" {
		kind = "user"
	}
	sc := ctx.GetKind(kind)
	if sc == nil {
		return false
	}
	return slices.Contains(keys, sc.Key)
}

// RedactContext strips private attributes (top-level and nested JSON pointers) and records `_meta.redactedAttributes`.
func RedactContext(ctx Context, cfg RedactionConfig) map[string]any {
	if ctx.Single != nil {
		return redactSingle(ctx.Single, cfg, true)
	}
	out := map[string]any{"kind": "multi"}
	for kind, sc := range ctx.Multi {
		out[kind] = redactSingle(sc, cfg, false)
	}
	return out
}

func redactSingle(sc *SingleContext, cfg RedactionConfig, includeKind bool) map[string]any {
	out := map[string]any{"key": sc.Key}
	if includeKind {
		out["kind"] = sc.Kind
	}
	if sc.Anonymous {
		out["anonymous"] = true
	}

	redactedSet := map[string]struct{}{}
	if cfg.AllAttributesPrivate {
		if sc.Name != nil {
			redactedSet["name"] = struct{}{}
		}
		for k := range sc.Attributes {
			redactedSet[k] = struct{}{}
		}
	} else {
		if sc.Name != nil {
			out["name"] = *sc.Name
		}
		for k, v := range sc.Attributes {
			out[k] = deepCopyAny(v)
		}

		refs := append([]string{}, cfg.GlobalPrivateAttributes...)
		refs = append(refs, sc.Meta.PrivateAttributes...)
		for _, rawRef := range refs {
			parts, ok := ParseAttributeRef(rawRef)
			if !ok {
				continue
			}
			first := parts[0]
			if first == "kind" || first == "key" || first == "anonymous" || first == "_meta" {
				continue
			}
			if removePath(out, parts) {
				redactedSet[canonicalRefString(parts, rawRef)] = struct{}{}
			}
		}
	}

	if len(redactedSet) > 0 {
		list := make([]string, 0, len(redactedSet))
		for k := range redactedSet {
			list = append(list, k)
		}
		sort.Strings(list)
		out["_meta"] = map[string]any{"redactedAttributes": list}
	}
	return out
}

func canonicalRefString(parts []string, raw string) string {
	if strings.HasPrefix(raw, "/") {
		return raw
	}
	return parts[0]
}

func removePath(obj map[string]any, parts []string) bool {
	if len(parts) == 1 {
		if _, exists := obj[parts[0]]; exists {
			delete(obj, parts[0])
			return true
		}
		return false
	}
	child, ok := obj[parts[0]].(map[string]any)
	if !ok {
		return false
	}
	return removePath(child, parts[1:])
}

func deepCopyAny(v any) any {
	switch val := v.(type) {
	case map[string]any:
		m := make(map[string]any, len(val))
		for k, item := range val {
			m[k] = deepCopyAny(item)
		}
		return m
	case []any:
		s := make([]any, len(val))
		for i, item := range val {
			s[i] = deepCopyAny(item)
		}
		return s
	default:
		return v
	}
}
