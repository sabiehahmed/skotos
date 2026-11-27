package skotos

import (
	"encoding/json"
	"errors"
	"strings"
)

// ContextMeta holds optional metadata on a single Skotos evaluation context.
type ContextMeta struct {
	PrivateAttributes []string `json:"privateAttributes,omitempty"`
	Secondary         string   `json:"secondary,omitempty"`
}

// SingleContext represents one entity (user, organization, device, etc.) being evaluated.
type SingleContext struct {
	Kind       string         `json:"kind"`
	Key        string         `json:"key"`
	Name       *string        `json:"name,omitempty"`
	Anonymous  bool           `json:"anonymous,omitempty"`
	Meta       ContextMeta    `json:"_meta,omitempty"`
	Attributes map[string]any `json:"-"`
}

// NewContext creates a validated single-kind context (e.g., kind="user").
func NewContext(kind, key string) Context {
	return Context{
		Single: &SingleContext{
			Kind:       kind,
			Key:        key,
			Attributes: map[string]any{},
		},
	}
}

// Context represents either a single-kind context or a multi-context (`kind == "multi"`).
type Context struct {
	Single *SingleContext
	Multi  map[string]*SingleContext
}

// GetKind returns the nested or top-level SingleContext matching kind.
func (c Context) GetKind(kind string) *SingleContext {
	if c.Single != nil {
		if c.Single.Kind == kind {
			return c.Single
		}
		return nil
	}
	if c.Multi != nil {
		return c.Multi[kind]
	}
	return nil
}

// Validate checks canonical context invariants per design/evaluation-engine.md.
func (c Context) Validate() error {
	if c.Single != nil {
		if !isValidContextKind(c.Single.Kind) {
			return errors.New("invalid context kind")
		}
		if c.Single.Key == "" {
			return errors.New("context key cannot be empty")
		}
		return nil
	}
	if len(c.Multi) == 0 {
		return errors.New("empty multi-context")
	}
	for k, sc := range c.Multi {
		if !isValidContextKind(k) || sc == nil || sc.Kind != k || sc.Key == "" {
			return errors.New("invalid multi-context entry")
		}
	}
	return nil
}

func isValidContextKind(kind string) bool {
	if kind == "" || kind == "kind" || kind == "multi" {
		return false
	}
	for i := 0; i < len(kind); i++ {
		ch := kind[i]
		if !((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9') || ch == '.' || ch == '_' || ch == '-') {
			return false
		}
	}
	return true
}

// UnmarshalJSON decodes both single-kind and multi-kind Skotos Context JSON objects.
func (c *Context) UnmarshalJSON(data []byte) error {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}
	kind := "user"
	if kRaw, ok := raw["kind"]; ok {
		var kStr string
		if err := json.Unmarshal(kRaw, &kStr); err != nil {
			return err
		}
		kind = kStr
	}

	if kind == "multi" {
		multi := make(map[string]*SingleContext)
		for k, v := range raw {
			if k == "kind" {
				continue
			}
			sc, err := parseSingleContext(k, v)
			if err != nil {
				return err
			}
			multi[k] = sc
		}
		c.Single = nil
		c.Multi = multi
		return nil
	}

	sc, err := parseSingleContext(kind, data)
	if err != nil {
		return err
	}
	c.Single = sc
	c.Multi = nil
	return nil
}

func parseSingleContext(defaultKind string, data []byte) (*SingleContext, error) {
	var raw map[string]any
	if err := json.Unmarshal(data, &raw); err != nil {
		return nil, err
	}
	sc := &SingleContext{
		Kind:       defaultKind,
		Attributes: map[string]any{},
	}
	for k, v := range raw {
		switch k {
		case "kind":
			if s, ok := v.(string); ok {
				sc.Kind = s
			}
		case "key":
			if s, ok := v.(string); ok {
				sc.Key = s
			}
		case "name":
			if s, ok := v.(string); ok {
				val := s
				sc.Name = &val
			}
		case "anonymous":
			if b, ok := v.(bool); ok {
				sc.Anonymous = b
			}
		case "_meta":
			metaBytes, _ := json.Marshal(v)
			_ = json.Unmarshal(metaBytes, &sc.Meta)
		default:
			sc.Attributes[k] = v
		}
	}
	return sc, nil
}

// ParseAttributeRef parses either a literal attribute name or an RFC 6901 JSON Pointer.
func ParseAttributeRef(raw string) ([]string, bool) {
	if raw == "" {
		return nil, false
	}
	if !strings.HasPrefix(raw, "/") {
		return []string{raw}, true
	}
	if raw == "/" || strings.HasSuffix(raw, "/") {
		return nil, false
	}
	parts := strings.Split(raw[1:], "/")
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if p == "" {
			return nil, false
		}
		unescaped, ok := unescapeJSONPointerToken(p)
		if !ok {
			return nil, false
		}
		out = append(out, unescaped)
	}
	return out, true
}

func unescapeJSONPointerToken(token string) (string, bool) {
	var b strings.Builder
	b.Grow(len(token))
	for i := 0; i < len(token); i++ {
		if token[i] == '~' {
			if i+1 >= len(token) {
				return "", false
			}
			switch token[i+1] {
			case '0':
				b.WriteByte('~')
			case '1':
				b.WriteByte('/')
			default:
				return "", false
			}
			i++
		} else {
			b.WriteByte(token[i])
		}
	}
	return b.String(), true
}

// GetAttribute resolves a parsed AttributeRef against a SingleContext.
func (sc *SingleContext) GetAttribute(components []string) (any, bool) {
	if len(components) == 0 {
		return nil, false
	}
	first := components[0]
	var cur any
	switch first {
	case "kind":
		if len(components) > 1 {
			return nil, false
		}
		return sc.Kind, true
	case "key":
		if len(components) > 1 {
			return nil, false
		}
		return sc.Key, true
	case "name":
		if len(components) > 1 || sc.Name == nil {
			return nil, false
		}
		return *sc.Name, true
	case "anonymous":
		if len(components) > 1 {
			return nil, false
		}
		return sc.Anonymous, true
	case "_meta":
		return nil, false
	default:
		val, ok := sc.Attributes[first]
		if !ok {
			return nil, false
		}
		cur = val
	}

	for _, seg := range components[1:] {
		m, ok := cur.(map[string]any)
		if !ok {
			return nil, false
		}
		next, exists := m[seg]
		if !exists {
			return nil, false
		}
		cur = next
	}
	return cur, true
}

// Compiled Evaluation Wire Models (matching crates/skotos-domain EvalFlag/EvalSegment)

type ClientSideAvailability struct {
	UsingEnvironmentID bool `json:"usingEnvironmentId"`
	UsingMobileKey     bool `json:"usingMobileKey"`
}

type EvalFlag struct {
	Key                    string                 `json:"key"`
	Version                int64                  `json:"version"`
	On                     bool                   `json:"on"`
	Salt                   string                 `json:"salt"`
	Variations             []any                  `json:"variations"`
	OffVariation           *int                   `json:"offVariation,omitempty"`
	Prerequisites          []EvalPrerequisite     `json:"prerequisites,omitempty"`
	Targets                []EvalTarget           `json:"targets,omitempty"`
	Rules                  []EvalRule             `json:"rules,omitempty"`
	Fallthrough            EvalVariationOrRollout `json:"fallthrough"`
	ClientSideAvailability ClientSideAvailability `json:"clientSideAvailability,omitempty"`
	TrackEvents            bool                   `json:"trackEvents,omitempty"`
}

type EvalPrerequisite struct {
	Key       string `json:"key"`
	Variation int    `json:"variation"`
}

type EvalTarget struct {
	ContextKind string   `json:"contextKind"`
	Values      []string `json:"values"`
	Variation   int      `json:"variation"`
}

type Clause struct {
	ContextKind string `json:"contextKind"`
	Attribute   string `json:"attribute"`
	Op          string `json:"op"`
	Values      []any  `json:"values"`
	Negate      bool   `json:"negate"`
}

type EvalRule struct {
	ID        string       `json:"id"`
	Clauses   []Clause     `json:"clauses"`
	Variation *int         `json:"variation,omitempty"`
	Rollout   *EvalRollout `json:"rollout,omitempty"`
}

type EvalVariationOrRollout struct {
	Variation *int         `json:"variation,omitempty"`
	Rollout   *EvalRollout `json:"rollout,omitempty"`
}

type EvalRollout struct {
	ContextKind string                  `json:"contextKind"`
	BucketBy    *string                 `json:"bucketBy,omitempty"`
	Seed        *int64                  `json:"seed,omitempty"`
	Kind        string                  `json:"kind,omitempty"`
	Variations  []EvalWeightedVariation `json:"variations"`
}

type EvalWeightedVariation struct {
	Variation int    `json:"variation"`
	Weight    uint32 `json:"weight"`
	Untracked bool   `json:"untracked,omitempty"`
}

type SegmentTarget struct {
	ContextKind string   `json:"contextKind"`
	Values      []string `json:"values"`
}

type SegmentRule struct {
	ID                 string   `json:"id"`
	Clauses            []Clause `json:"clauses"`
	Weight             *uint32  `json:"weight,omitempty"`
	BucketBy           *string  `json:"bucketBy,omitempty"`
	RolloutContextKind *string  `json:"rolloutContextKind,omitempty"`
}

type EvalSegment struct {
	Key              string          `json:"key"`
	Version          int64           `json:"version"`
	Salt             string          `json:"salt"`
	Included         []string        `json:"included,omitempty"`
	Excluded         []string        `json:"excluded,omitempty"`
	IncludedContexts []SegmentTarget `json:"includedContexts,omitempty"`
	ExcludedContexts []SegmentTarget `json:"excludedContexts,omitempty"`
	Rules            []SegmentRule   `json:"rules,omitempty"`
}

type EvaluationReason struct {
	Kind            string `json:"kind"`
	RuleIndex       *int   `json:"ruleIndex,omitempty"`
	RuleID          string `json:"ruleId,omitempty"`
	PrerequisiteKey string `json:"prerequisiteKey,omitempty"`
	InExperiment    bool   `json:"inExperiment,omitempty"`
	ErrorKind       string `json:"errorKind,omitempty"`
}

type EvaluationDetail struct {
	Value          any              `json:"value"`
	VariationIndex *int             `json:"variationIndex,omitempty"`
	Reason         EvaluationReason `json:"reason"`
}

type RedactionConfig struct {
	AllAttributesPrivate    bool     `json:"allAttributesPrivate"`
	GlobalPrivateAttributes []string `json:"globalPrivateAttributes"`
}

type EvalStore interface {
	GetFlag(key string) (*EvalFlag, bool)
	GetSegment(key string) (*EvalSegment, bool)
}
