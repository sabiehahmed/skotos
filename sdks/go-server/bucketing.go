package skotos

import (
	"crypto/sha1"
	"encoding/binary"
	"fmt"
	"math"
	"math/bits"
	"strconv"
)

const bucketScale uint64 = 100_000

// HashToBucket computes the exact deterministic SHA-1 60-bit integer basis-point bucket in 0..=99,999.
// Formula: ((top60Bits * 100_000) / (1 << 60)) using 128-bit exact integer math (bits.Mul64/Div64).
func HashToBucket(hashInput string) uint64 {
	sum := sha1.Sum([]byte(hashInput))
	top60 := binary.BigEndian.Uint64(sum[:8]) >> 4
	hi, lo := bits.Mul64(top60, bucketScale)
	// Divide (hi, lo) by 2^60: right-shift 128-bit integer by 60 bits == (hi << 4) | (lo >> 60)
	return (hi << 4) | (lo >> 60)
}

func buildHashInput(key, salt, bucketValue, secondary string, seed *int64) string {
	if seed != nil {
		return fmt.Sprintf("%d.%s", *seed, bucketValue)
	}
	if secondary != "" {
		return fmt.Sprintf("%s.%s.%s.%s", key, salt, bucketValue, secondary)
	}
	return fmt.Sprintf("%s.%s.%s", key, salt, bucketValue)
}

func extractBucketValue(ctx Context, contextKind string, bucketBy *string) (string, string, bool) {
	if contextKind == "" {
		contextKind = "user"
	}
	sc := ctx.GetKind(contextKind)
	if sc == nil {
		return "", "", false
	}
	attr := "key"
	if bucketBy != nil && *bucketBy != "" {
		attr = *bucketBy
	}
	parts, ok := ParseAttributeRef(attr)
	if !ok {
		return "", "", false
	}
	val, exists := sc.GetAttribute(parts)
	if !exists {
		return "", "", false
	}
	switch v := val.(type) {
	case string:
		return v, sc.Meta.Secondary, true
	case float64:
		if math.Trunc(v) == v && !math.IsInf(v, 0) && !math.IsNaN(v) {
			return strconv.FormatInt(int64(v), 10), sc.Meta.Secondary, true
		}
		return "", "", false
	case int:
		return strconv.Itoa(v), sc.Meta.Secondary, true
	case int64:
		return strconv.FormatInt(v, 10), sc.Meta.Secondary, true
	default:
		return "", "", false
	}
}

type rolloutResult struct {
	variationIndex int
	inExperiment   bool
}

func evaluateRollout(flagKey, salt string, rollout *EvalRollout, ctx Context) (rolloutResult, bool) {
	if rollout == nil || len(rollout.Variations) == 0 {
		return rolloutResult{}, false
	}
	isExperiment := rollout.Kind == "experiment"
	var effectiveBucketBy *string
	if !isExperiment {
		effectiveBucketBy = rollout.BucketBy
	}

	contextKind := rollout.ContextKind
	if contextKind == "" {
		contextKind = "user"
	}

	var bucket uint64
	validCtx := false
	if bVal, sec, ok := extractBucketValue(ctx, contextKind, effectiveBucketBy); ok {
		effectiveSec := sec
		if isExperiment {
			effectiveSec = ""
		}
		input := buildHashInput(flagKey, salt, bVal, effectiveSec, rollout.Seed)
		bucket = HashToBucket(input)
		validCtx = true
	}

	var cumulative uint64
	for _, wv := range rollout.Variations {
		cumulative += uint64(wv.Weight)
		if bucket < cumulative {
			return rolloutResult{
				variationIndex: wv.Variation,
				inExperiment:   isExperiment && validCtx && !wv.Untracked,
			}, true
		}
	}

	var fallback EvalWeightedVariation
	if cumulative == 0 {
		fallback = rollout.Variations[0]
	} else {
		fallback = rollout.Variations[len(rollout.Variations)-1]
	}
	return rolloutResult{
		variationIndex: fallback.Variation,
		inExperiment:   isExperiment && validCtx && !fallback.Untracked,
	}, true
}
