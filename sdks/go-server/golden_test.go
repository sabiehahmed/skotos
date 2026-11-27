package skotos

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

type goldenSuiteFile struct {
	Suite    string                  `json:"suite"`
	Kind     string                  `json:"kind"`
	Flags    map[string]*EvalFlag    `json:"flags"`
	Segments map[string]*EvalSegment `json:"segments"`
	Tests    []goldenTestCase        `json:"tests"`
}

type goldenTestCase struct {
	Name            string                  `json:"name"`
	FlagKey         string                  `json:"flagKey"`
	Getter          string                  `json:"getter"`
	DefaultValue    any                     `json:"defaultValue"`
	Context         Context                 `json:"context"`
	Flags           map[string]*EvalFlag    `json:"flags"`
	Segments        map[string]*EvalSegment `json:"segments"`
	RedactionConfig *RedactionConfig        `json:"redactionConfig"`
	Expected        json.RawMessage         `json:"expected"`
}

type mapStore struct {
	flags    map[string]*EvalFlag
	segments map[string]*EvalSegment
}

func (m *mapStore) GetFlag(key string) (*EvalFlag, bool) {
	f, ok := m.flags[key]
	return f, ok
}

func (m *mapStore) GetSegment(key string) (*EvalSegment, bool) {
	s, ok := m.segments[key]
	return s, ok
}

func TestAllGoldenEvaluationSuites(t *testing.T) {
	specDir := filepath.Join("..", "..", "spec", "evaluation-tests")
	suites := []string{
		"basic.json",
		"operators.json",
		"rollouts.json",
		"multi-context.json",
		"prerequisites.json",
		"segments.json",
		"errors.json",
		"redaction.json",
	}

	totalVectors := 0
	for _, filename := range suites {
		raw, err := os.ReadFile(filepath.Join(specDir, filename))
		if err != nil {
			t.Fatalf("failed reading %s: %v", filename, err)
		}
		var suite goldenSuiteFile
		if err := json.Unmarshal(raw, &suite); err != nil {
			t.Fatalf("failed parsing %s: %v", filename, err)
		}

		for _, tc := range suite.Tests {
			totalVectors++
			if suite.Kind == "redaction" {
				cfg := RedactionConfig{}
				if tc.RedactionConfig != nil {
					cfg = *tc.RedactionConfig
				}
				actual := RedactContext(tc.Context, cfg)
				aJSON, _ := json.Marshal(actual)
				var actualAny, expectedAny any
				_ = json.Unmarshal(aJSON, &actualAny)
				if err := json.Unmarshal(tc.Expected, &expectedAny); err != nil {
					t.Fatalf("[%s] %s: bad expected JSON: %v", suite.Suite, tc.Name, err)
				}
				if !deepJSONEqual(actualAny, expectedAny) {
					t.Errorf("[%s] %q redaction mismatch:\n  got:  %s\n  want: %s", suite.Suite, tc.Name, aJSON, tc.Expected)
				}
				continue
			}

			store := &mapStore{
				flags:    map[string]*EvalFlag{},
				segments: map[string]*EvalSegment{},
			}
			for k, f := range suite.Flags {
				store.flags[k] = f
			}
			for k, s := range suite.Segments {
				store.segments[k] = s
			}
			for k, f := range tc.Flags {
				store.flags[k] = f
			}
			for k, s := range tc.Segments {
				store.segments[k] = s
			}

			var detail EvaluationDetail
			switch tc.Getter {
			case "bool":
				def, _ := tc.DefaultValue.(bool)
				detail = BoolVariation(store, tc.FlagKey, tc.Context, def)
			case "string":
				def, _ := tc.DefaultValue.(string)
				detail = StringVariation(store, tc.FlagKey, tc.Context, def)
			case "number":
				def, _ := toFloat64(tc.DefaultValue)
				detail = NumberVariation(store, tc.FlagKey, tc.Context, def)
			default:
				detail = Evaluate(store, tc.FlagKey, tc.Context, tc.DefaultValue)
			}

			actualBytes, _ := json.Marshal(detail)
			var actualAny, expectedAny any
			_ = json.Unmarshal(actualBytes, &actualAny)
			_ = json.Unmarshal(tc.Expected, &expectedAny)

			if !deepJSONEqual(actualAny, expectedAny) {
				t.Errorf("[%s] %q evaluation mismatch:\n  got:  %s\n  want: %s", suite.Suite, tc.Name, actualBytes, tc.Expected)
			}
		}
	}

	if totalVectors < 100 {
		t.Fatalf("expected >= 100 golden vectors, ran %d", totalVectors)
	}
	t.Logf("Go Server SDK passed %d/%d canonical golden evaluation vectors", totalVectors, totalVectors)
}

func deepJSONEqual(a, b any) bool {
	if fa, ok1 := toFloat64(a); ok1 {
		if fb, ok2 := toFloat64(b); ok2 {
			return fa == fb
		}
		return false
	}
	switch va := a.(type) {
	case map[string]any:
		vb, ok := b.(map[string]any)
		if !ok || len(va) != len(vb) {
			return false
		}
		for k, valA := range va {
			valB, exists := vb[k]
			if !exists || !deepJSONEqual(valA, valB) {
				return false
			}
		}
		return true
	case []any:
		vb, ok := b.([]any)
		if !ok || len(va) != len(vb) {
			return false
		}
		for i := range va {
			if !deepJSONEqual(va[i], vb[i]) {
				return false
			}
		}
		return true
	default:
		return a == b
	}
}

func TestOfflineTestDataAndOpenFeatureGoProvider(t *testing.T) {
	td := NewTestData().SetBoolFlag("checkout-v2", true)
	client := NewClientWithTestData(td)
	defer client.Close()

	if !client.BoolVariation("checkout-v2", NewContext("user", "u1"), false) {
		t.Fatal("expected checkout-v2 to evaluate true")
	}

	provider := NewOpenFeatureProvider(client)
	res := provider.BooleanEvaluation("checkout-v2", false, map[string]any{"targetingKey": "u1"})
	if !res.Value || res.Variant != "0" || res.Reason != "DEFAULT" || res.ErrorCode != "" {
		t.Fatalf("unexpected OpenFeature resolution: %+v", res)
	}
	missing := provider.BooleanEvaluation("missing", false, map[string]any{"targetingKey": "u1"})
	if missing.ErrorCode != "FLAG_NOT_FOUND" {
		t.Fatalf("expected FLAG_NOT_FOUND, got %+v", missing)
	}
}
