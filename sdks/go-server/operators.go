package skotos

import (
	"math"
	"reflect"
	"regexp"
	"strconv"
	"strings"
	"time"
)

func evalClause(store EvalStore, clause Clause, ctx Context, segStack *[]string) (bool, bool) {
	if clause.Op == "segmentMatch" {
		matched := false
		for _, v := range clause.Values {
			segKey, ok := v.(string)
			if !ok {
				continue
			}
			seg, found := store.GetSegment(segKey)
			if !found {
				continue
			}
			res, valid := evaluateSegment(store, seg, ctx, segStack)
			if !valid {
				return false, false
			}
			if res {
				matched = true
				break
			}
		}
		if clause.Negate {
			return !matched, true
		}
		return matched, true
	}

	kind := clause.ContextKind
	if kind == "" {
		kind = "user"
	}
	sc := ctx.GetKind(kind)
	if sc == nil {
		return false, true
	}
	parts, ok := ParseAttributeRef(clause.Attribute)
	if !ok {
		return false, true
	}
	attrVal, exists := sc.GetAttribute(parts)
	if !exists || attrVal == nil {
		return false, true
	}

	var rawMatch bool
	if arr, isArr := attrVal.([]any); isArr {
		if len(arr) == 0 {
			return false, true
		}
		for _, item := range arr {
			if item == nil {
				continue
			}
			for _, cv := range clause.Values {
				if matchSingleValue(clause.Op, item, cv) {
					rawMatch = true
					break
				}
			}
			if rawMatch {
				break
			}
		}
	} else {
		for _, cv := range clause.Values {
			if matchSingleValue(clause.Op, attrVal, cv) {
				rawMatch = true
				break
			}
		}
	}

	if clause.Negate {
		return !rawMatch, true
	}
	return rawMatch, true
}

func matchSingleValue(op string, actual, target any) bool {
	switch op {
	case "in":
		return jsonValuesEqual(actual, target)
	case "startsWith":
		a, ok1 := actual.(string)
		t, ok2 := target.(string)
		return ok1 && ok2 && strings.HasPrefix(a, t)
	case "endsWith":
		a, ok1 := actual.(string)
		t, ok2 := target.(string)
		return ok1 && ok2 && strings.HasSuffix(a, t)
	case "contains":
		a, ok1 := actual.(string)
		t, ok2 := target.(string)
		return ok1 && ok2 && strings.Contains(a, t)
	case "matches":
		a, ok1 := actual.(string)
		pat, ok2 := target.(string)
		if !ok1 || !ok2 {
			return false
		}
		re, err := regexp.Compile(pat)
		return err == nil && re.MatchString(a)
	case "lessThan":
		cmp, ok := compareNumbers(actual, target)
		return ok && cmp < 0
	case "lessThanOrEqual":
		cmp, ok := compareNumbers(actual, target)
		return ok && cmp <= 0
	case "greaterThan":
		cmp, ok := compareNumbers(actual, target)
		return ok && cmp > 0
	case "greaterThanOrEqual":
		cmp, ok := compareNumbers(actual, target)
		return ok && cmp >= 0
	case "before":
		cmp, ok := compareDates(actual, target)
		return ok && cmp < 0
	case "after":
		cmp, ok := compareDates(actual, target)
		return ok && cmp > 0
	case "semverEqual":
		cmp, ok := compareSemVer(actual, target)
		return ok && cmp == 0
	case "semverLessThan":
		cmp, ok := compareSemVer(actual, target)
		return ok && cmp < 0
	case "semverGreaterThan":
		cmp, ok := compareSemVer(actual, target)
		return ok && cmp > 0
	default:
		return false
	}
}

func toFloat64(v any) (float64, bool) {
	switch n := v.(type) {
	case float64:
		return n, !math.IsNaN(n) && !math.IsInf(n, 0)
	case int:
		return float64(n), true
	case int64:
		return float64(n), true
	default:
		return 0, false
	}
}

func jsonValuesEqual(a, b any) bool {
	if fa, ok1 := toFloat64(a); ok1 {
		if fb, ok2 := toFloat64(b); ok2 {
			return fa == fb
		}
		return false
	}
	return reflect.DeepEqual(a, b)
}

func compareNumbers(a, b any) (int, bool) {
	fa, ok1 := toFloat64(a)
	fb, ok2 := toFloat64(b)
	if !ok1 || !ok2 {
		return 0, false
	}
	if fa < fb {
		return -1, true
	}
	if fa > fb {
		return 1, true
	}
	return 0, true
}

func parseTimestampNanos(v any) (int64, bool) {
	switch val := v.(type) {
	case string:
		t, err := time.Parse(time.RFC3339Nano, val)
		if err != nil {
			return 0, false
		}
		return t.UnixNano(), true
	default:
		f, ok := toFloat64(v)
		if !ok {
			return 0, false
		}
		return int64(f * 1_000_000), true
	}
}

func compareDates(a, b any) (int, bool) {
	ta, ok1 := parseTimestampNanos(a)
	tb, ok2 := parseTimestampNanos(b)
	if !ok1 || !ok2 {
		return 0, false
	}
	if ta < tb {
		return -1, true
	}
	if ta > tb {
		return 1, true
	}
	return 0, true
}

type parsedSemVer struct {
	major, minor, patch uint64
	prerelease          []string
}

func parseSemVerRelaxed(input string) (parsedSemVer, bool) {
	if input == "" || input[0] == 'v' || input[0] == 'V' {
		return parsedSemVer{}, false
	}
	// Strip build metadata (+...) — ignored in SemVer 2.0.0 precedence
	mainAndPre := input
	if plusIdx := strings.IndexByte(mainAndPre, '+'); plusIdx >= 0 {
		build := mainAndPre[plusIdx+1:]
		if build == "" || !validSemVerIdents(strings.Split(build, "."), false) {
			return parsedSemVer{}, false
		}
		mainAndPre = mainAndPre[:plusIdx]
	}

	var preParts []string
	core := mainAndPre
	if dashIdx := strings.IndexByte(mainAndPre, '-'); dashIdx >= 0 {
		preStr := mainAndPre[dashIdx+1:]
		if preStr == "" {
			return parsedSemVer{}, false
		}
		preParts = strings.Split(preStr, ".")
		if !validSemVerIdents(preParts, true) {
			return parsedSemVer{}, false
		}
		core = mainAndPre[:dashIdx]
	}

	coreParts := strings.Split(core, ".")
	if len(coreParts) < 1 || len(coreParts) > 3 {
		return parsedSemVer{}, false
	}
	nums := [3]uint64{}
	for i, p := range coreParts {
		if p == "" || (len(p) > 1 && p[0] == '0') {
			return parsedSemVer{}, false
		}
		n, err := strconv.ParseUint(p, 10, 64)
		if err != nil {
			return parsedSemVer{}, false
		}
		nums[i] = n
	}
	return parsedSemVer{
		major:      nums[0],
		minor:      nums[1],
		patch:      nums[2],
		prerelease: preParts,
	}, true
}

func validSemVerIdents(parts []string, noLeadingZeroNumeric bool) bool {
	for _, p := range parts {
		if p == "" {
			return false
		}
		allDigits := true
		for i := 0; i < len(p); i++ {
			ch := p[i]
			if !((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || ch == '-') {
				return false
			}
			if ch < '0' || ch > '9' {
				allDigits = false
			}
		}
		if noLeadingZeroNumeric && allDigits && len(p) > 1 && p[0] == '0' {
			return false
		}
	}
	return true
}

func compareSemVer(a, b any) (int, bool) {
	sa, ok1 := a.(string)
	sb, ok2 := b.(string)
	if !ok1 || !ok2 {
		return 0, false
	}
	va, ok1 := parseSemVerRelaxed(sa)
	vb, ok2 := parseSemVerRelaxed(sb)
	if !ok1 || !ok2 {
		return 0, false
	}
	if va.major != vb.major {
		return cmpU64(va.major, vb.major), true
	}
	if va.minor != vb.minor {
		return cmpU64(va.minor, vb.minor), true
	}
	if va.patch != vb.patch {
		return cmpU64(va.patch, vb.patch), true
	}
	if len(va.prerelease) == 0 && len(vb.prerelease) == 0 {
		return 0, true
	}
	if len(va.prerelease) == 0 {
		return 1, true
	}
	if len(vb.prerelease) == 0 {
		return -1, true
	}
	minLen := len(va.prerelease)
	if len(vb.prerelease) < minLen {
		minLen = len(vb.prerelease)
	}
	for i := 0; i < minLen; i++ {
		pa, pb := va.prerelease[i], vb.prerelease[i]
		if pa == pb {
			continue
		}
		na, errA := strconv.ParseUint(pa, 10, 64)
		nb, errB := strconv.ParseUint(pb, 10, 64)
		switch {
		case errA == nil && errB == nil:
			return cmpU64(na, nb), true
		case errA == nil && errB != nil:
			return -1, true
		case errA != nil && errB == nil:
			return 1, true
		default:
			if pa < pb {
				return -1, true
			}
			return 1, true
		}
	}
	return cmpU64(uint64(len(va.prerelease)), uint64(len(vb.prerelease))), true
}

func cmpU64(a, b uint64) int {
	if a < b {
		return -1
	}
	if a > b {
		return 1
	}
	return 0
}
