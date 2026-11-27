package skotos

import (
	"bufio"
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"time"
)

// FeatureStore is a thread-safe, monotonically version-checked flag & segment store.
type FeatureStore struct {
	mu          sync.RWMutex
	initialized bool
	flags       map[string]*storedFlag
	segments    map[string]*storedSegment
}

type storedFlag struct {
	version int64
	flag    *EvalFlag
}

type storedSegment struct {
	version int64
	seg     *EvalSegment
}

func NewFeatureStore() *FeatureStore {
	return &FeatureStore{
		flags:    make(map[string]*storedFlag),
		segments: make(map[string]*storedSegment),
	}
}

func (s *FeatureStore) IsInitialized() bool {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.initialized
}

func (s *FeatureStore) Put(flags map[string]*EvalFlag, segments map[string]*EvalSegment) []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	var changed []string
	for k, f := range flags {
		if old, ok := s.flags[k]; !ok || old.version != f.Version || old.flag == nil {
			changed = append(changed, k)
		}
	}
	s.flags = make(map[string]*storedFlag, len(flags))
	for k, f := range flags {
		s.flags[k] = &storedFlag{version: f.Version, flag: f}
	}
	s.segments = make(map[string]*storedSegment, len(segments))
	for k, seg := range segments {
		s.segments[k] = &storedSegment{version: seg.Version, seg: seg}
	}
	s.initialized = true
	return changed
}

func (s *FeatureStore) UpsertFlag(flag *EvalFlag) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if cur, ok := s.flags[flag.Key]; ok && flag.Version <= cur.version {
		return false
	}
	s.flags[flag.Key] = &storedFlag{version: flag.Version, flag: flag}
	s.initialized = true
	return true
}

func (s *FeatureStore) DeleteFlag(key string, version int64) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if cur, ok := s.flags[key]; ok && version <= cur.version {
		return false
	}
	s.flags[key] = &storedFlag{version: version, flag: nil}
	return true
}

func (s *FeatureStore) UpsertSegment(seg *EvalSegment) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if cur, ok := s.segments[seg.Key]; ok && seg.Version <= cur.version {
		return false
	}
	s.segments[seg.Key] = &storedSegment{version: seg.Version, seg: seg}
	return true
}

func (s *FeatureStore) DeleteSegment(key string, version int64) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if cur, ok := s.segments[key]; ok && version <= cur.version {
		return false
	}
	s.segments[key] = &storedSegment{version: version, seg: nil}
	return true
}

func (s *FeatureStore) GetFlag(key string) (*EvalFlag, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	item, ok := s.flags[key]
	if !ok || item.flag == nil {
		return nil, false
	}
	return item.flag, true
}

func (s *FeatureStore) GetSegment(key string) (*EvalSegment, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	item, ok := s.segments[key]
	if !ok || item.seg == nil {
		return nil, false
	}
	return item.seg, true
}

// TestData provides an offline mock data source for zero-network unit testing.
type TestData struct {
	store *FeatureStore
}

func NewTestData() *TestData {
	s := NewFeatureStore()
	s.Put(map[string]*EvalFlag{}, map[string]*EvalSegment{})
	return &TestData{store: s}
}

func (td *TestData) SetBoolFlag(key string, value bool) *TestData {
	ver := int64(1)
	if old, ok := td.store.GetFlag(key); ok {
		ver = old.Version + 1
	}
	offIdx := 1
	fallIdx := 1
	if value {
		fallIdx = 0
	}
	td.store.UpsertFlag(&EvalFlag{
		Key:          key,
		Version:      ver,
		On:           true,
		Salt:         "test-salt",
		Variations:   []any{true, false},
		OffVariation: &offIdx,
		Fallthrough:  EvalVariationOrRollout{Variation: &fallIdx},
	})
	return td
}

// Config configures a Skotos Go Server SDK Client.
type Config struct {
	SDKKey        string
	StreamURL     string
	PollURL       string
	EventsURL     string
	Offline       bool
	UsePolling    bool
	PollInterval  time.Duration
	FlushInterval time.Duration
	Redaction     RedactionConfig
}

type summaryKey struct {
	flagKey   string
	version   int64
	variation int
	hasVar    bool
}

type summaryAgg struct {
	count int64
	value any
	def   any
}

// Client is the production Skotos Go Server-Side SDK client.
type Client struct {
	cfg       Config
	store     *FeatureStore
	http      *http.Client
	cancel    context.CancelFunc
	readyCh   chan struct{}
	readyOnce sync.Once

	listenersMu sync.RWMutex
	listeners   []func(flagKey string)

	eventsMu      sync.Mutex
	windowStartMs int64
	summary       map[summaryKey]*summaryAgg
	flagKinds     map[string]map[string]struct{}
}

func NewClient(cfg Config) *Client {
	if cfg.PollURL == "" {
		cfg.PollURL = cfg.StreamURL
	}
	if cfg.PollInterval <= 0 {
		cfg.PollInterval = 30 * time.Second
	}
	if cfg.FlushInterval <= 0 {
		cfg.FlushInterval = 5 * time.Second
	}

	ctx, cancel := context.WithCancel(context.Background())
	c := &Client{
		cfg:           cfg,
		store:         NewFeatureStore(),
		http:          &http.Client{},
		cancel:        cancel,
		readyCh:       make(chan struct{}),
		windowStartMs: time.Now().UnixMilli(),
		summary:       make(map[summaryKey]*summaryAgg),
		flagKinds:     make(map[string]map[string]struct{}),
	}

	if cfg.Offline {
		c.markReady()
		return c
	}

	if cfg.UsePolling {
		go c.pollLoop(ctx)
	} else {
		go c.streamLoop(ctx)
	}
	if cfg.EventsURL != "" {
		go c.flushLoop(ctx)
	}
	return c
}

func NewClientWithTestData(td *TestData) *Client {
	_, cancel := context.WithCancel(context.Background())
	c := &Client{
		cfg:           Config{Offline: true},
		store:         td.store,
		http:          &http.Client{},
		cancel:        cancel,
		readyCh:       make(chan struct{}),
		windowStartMs: time.Now().UnixMilli(),
		summary:       make(map[summaryKey]*summaryAgg),
		flagKinds:     make(map[string]map[string]struct{}),
	}
	c.markReady()
	return c
}

func (c *Client) markReady() {
	c.readyOnce.Do(func() { close(c.readyCh) })
}

func (c *Client) WaitForInitialization(timeout time.Duration) bool {
	select {
	case <-c.readyCh:
		return true
	case <-time.After(timeout):
		return c.store.IsInitialized()
	}
}

func (c *Client) OnChange(fn func(flagKey string)) {
	c.listenersMu.Lock()
	defer c.listenersMu.Unlock()
	c.listeners = append(c.listeners, fn)
}

func (c *Client) emitChanges(keys []string) {
	c.listenersMu.RLock()
	fns := append([]func(string){}, c.listeners...)
	c.listenersMu.RUnlock()
	for _, k := range keys {
		for _, fn := range fns {
			fn(k)
		}
	}
}

func (c *Client) BoolVariation(flagKey string, ctx Context, defaultValue bool) bool {
	d := c.BoolVariationDetail(flagKey, ctx, defaultValue)
	if b, ok := d.Value.(bool); ok {
		return b
	}
	return defaultValue
}

func (c *Client) BoolVariationDetail(flagKey string, ctx Context, defaultValue bool) EvaluationDetail {
	if !c.cfg.Offline && !c.store.IsInitialized() {
		return errorDetail(defaultValue, "CLIENT_NOT_READY")
	}
	d := BoolVariation(c.store, flagKey, ctx, defaultValue)
	c.recordEval(flagKey, ctx, defaultValue, d)
	return d
}

func (c *Client) StringVariation(flagKey string, ctx Context, defaultValue string) string {
	d := c.StringVariationDetail(flagKey, ctx, defaultValue)
	if s, ok := d.Value.(string); ok {
		return s
	}
	return defaultValue
}

func (c *Client) StringVariationDetail(flagKey string, ctx Context, defaultValue string) EvaluationDetail {
	if !c.cfg.Offline && !c.store.IsInitialized() {
		return errorDetail(defaultValue, "CLIENT_NOT_READY")
	}
	d := StringVariation(c.store, flagKey, ctx, defaultValue)
	c.recordEval(flagKey, ctx, defaultValue, d)
	return d
}

func (c *Client) Float64Variation(flagKey string, ctx Context, defaultValue float64) float64 {
	d := c.Float64VariationDetail(flagKey, ctx, defaultValue)
	if f, ok := toFloat64(d.Value); ok {
		return f
	}
	return defaultValue
}

func (c *Client) Float64VariationDetail(flagKey string, ctx Context, defaultValue float64) EvaluationDetail {
	if !c.cfg.Offline && !c.store.IsInitialized() {
		return errorDetail(defaultValue, "CLIENT_NOT_READY")
	}
	d := NumberVariation(c.store, flagKey, ctx, defaultValue)
	c.recordEval(flagKey, ctx, defaultValue, d)
	return d
}

func (c *Client) JSONVariationDetail(flagKey string, ctx Context, defaultValue any) EvaluationDetail {
	if !c.cfg.Offline && !c.store.IsInitialized() {
		return errorDetail(defaultValue, "CLIENT_NOT_READY")
	}
	d := Evaluate(c.store, flagKey, ctx, defaultValue)
	c.recordEval(flagKey, ctx, defaultValue, d)
	return d
}

func (c *Client) recordEval(flagKey string, ctx Context, def any, d EvaluationDetail) {
	c.eventsMu.Lock()
	defer c.eventsMu.Unlock()
	var ver int64
	if f, ok := c.store.GetFlag(flagKey); ok {
		ver = f.Version
	}
	k := summaryKey{flagKey: flagKey, version: ver}
	if d.VariationIndex != nil {
		k.variation = *d.VariationIndex
		k.hasVar = true
	}
	agg, ok := c.summary[k]
	if !ok {
		agg = &summaryAgg{value: d.Value, def: def}
		c.summary[k] = agg
	}
	agg.count++

	kinds, ok := c.flagKinds[flagKey]
	if !ok {
		kinds = make(map[string]struct{})
		c.flagKinds[flagKey] = kinds
	}
	if ctx.Single != nil {
		kinds[ctx.Single.Kind] = struct{}{}
	} else {
		for ck := range ctx.Multi {
			kinds[ck] = struct{}{}
		}
	}
}

func (c *Client) Flush() error {
	c.eventsMu.Lock()
	if len(c.summary) == 0 || c.cfg.EventsURL == "" {
		c.eventsMu.Unlock()
		return nil
	}
	startMs := c.windowStartMs
	endMs := time.Now().UnixMilli()
	c.windowStartMs = endMs
	summary := c.summary
	kinds := c.flagKinds
	c.summary = make(map[summaryKey]*summaryAgg)
	c.flagKinds = make(map[string]map[string]struct{})
	c.eventsMu.Unlock()

	features := make(map[string]map[string]any)
	for k, agg := range summary {
		bucket, ok := features[k.flagKey]
		if !ok {
			var ckList []string
			for ck := range kinds[k.flagKey] {
				ckList = append(ckList, ck)
			}
			bucket = map[string]any{
				"default":      agg.def,
				"contextKinds": ckList,
				"counters":     []map[string]any{},
			}
			features[k.flagKey] = bucket
		}
		counter := map[string]any{
			"count": agg.count,
			"value": agg.value,
		}
		if k.version > 0 {
			counter["version"] = k.version
		} else {
			counter["unknown"] = true
		}
		if k.hasVar {
			counter["variation"] = k.variation
		}
		bucket["counters"] = append(bucket["counters"].([]map[string]any), counter)
	}

	batch := []map[string]any{{
		"kind":      "summary",
		"startDate": startMs,
		"endDate":   endMs,
		"features":  features,
	}}
	raw, err := json.Marshal(batch)
	if err != nil {
		return err
	}
	var gzBuf bytes.Buffer
	gz := gzip.NewWriter(&gzBuf)
	_, _ = gz.Write(raw)
	_ = gz.Close()

	url := strings.TrimRight(c.cfg.EventsURL, "/") + "/events/v1/bulk"
	req, err := http.NewRequest(http.MethodPost, url, &gzBuf)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", c.cfg.SDKKey)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Content-Encoding", "gzip")
	req.Header.Set("X-Skotos-Payload-ID", fmt.Sprintf("go-%d", endMs))

	resp, err := c.http.Do(req)
	if err != nil {
		return err
	}
	_ = resp.Body.Close()
	return nil
}

func (c *Client) Close() {
	c.cancel()
	_ = c.Flush()
}

func (c *Client) flushLoop(ctx context.Context) {
	ticker := time.NewTicker(c.cfg.FlushInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			_ = c.Flush()
		}
	}
}

func (c *Client) streamLoop(ctx context.Context) {
	url := strings.TrimRight(c.cfg.StreamURL, "/") + "/stream/v1/all"
	backoff := 250 * time.Millisecond

	for ctx.Err() == nil {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
		if err != nil {
			return
		}
		req.Header.Set("Authorization", c.cfg.SDKKey)
		req.Header.Set("Accept", "text/event-stream")

		resp, err := c.http.Do(req)
		if err == nil {
			if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
				_ = resp.Body.Close()
				return
			}
			if resp.StatusCode == http.StatusOK {
				backoff = 250 * time.Millisecond
				scanner := bufio.NewScanner(resp.Body)
				scanner.Buffer(make([]byte, 64*1024), 8*1024*1024)
				var evName, evData string
				for scanner.Scan() {
					line := scanner.Text()
					if line == "" {
						if evData != "" {
							c.applySSE(evName, evData)
						}
						evName = ""
						evData = ""
					} else if strings.HasPrefix(line, "event:") {
						evName = strings.TrimSpace(line[len("event:"):])
					} else if strings.HasPrefix(line, "data:") {
						if evData != "" {
							evData += "\n"
						}
						evData += strings.TrimSpace(line[len("data:"):])
					}
				}
			}
			_ = resp.Body.Close()
		}

		select {
		case <-ctx.Done():
			return
		case <-time.After(backoff):
			backoff *= 2
			if backoff > 30*time.Second {
				backoff = 30 * time.Second
			}
		}
	}
}

func (c *Client) applySSE(eventName, data string) {
	switch eventName {
	case "put":
		var env struct {
			Data struct {
				Flags    map[string]*EvalFlag    `json:"flags"`
				Segments map[string]*EvalSegment `json:"segments"`
			} `json:"data"`
		}
		if json.Unmarshal([]byte(data), &env) == nil {
			changed := c.store.Put(env.Data.Flags, env.Data.Segments)
			c.markReady()
			c.emitChanges(changed)
		}
	case "patch":
		var env struct {
			Path string          `json:"path"`
			Data json.RawMessage `json:"data"`
		}
		if json.Unmarshal([]byte(data), &env) == nil {
			if strings.HasPrefix(env.Path, "/flags/") {
				var flag EvalFlag
				if json.Unmarshal(env.Data, &flag) == nil {
					if c.store.UpsertFlag(&flag) {
						c.emitChanges([]string{flag.Key})
					}
				}
			} else if strings.HasPrefix(env.Path, "/segments/") {
				var seg EvalSegment
				if json.Unmarshal(env.Data, &seg) == nil {
					c.store.UpsertSegment(&seg)
				}
			}
		}
	case "delete":
		var env struct {
			Path    string `json:"path"`
			Version int64  `json:"version"`
		}
		if json.Unmarshal([]byte(data), &env) == nil {
			if strings.HasPrefix(env.Path, "/flags/") {
				key := strings.TrimPrefix(env.Path, "/flags/")
				if c.store.DeleteFlag(key, env.Version) {
					c.emitChanges([]string{key})
				}
			} else if strings.HasPrefix(env.Path, "/segments/") {
				key := strings.TrimPrefix(env.Path, "/segments/")
				c.store.DeleteSegment(key, env.Version)
			}
		}
	}
}

func (c *Client) pollLoop(ctx context.Context) {
	url := strings.TrimRight(c.cfg.PollURL, "/") + "/sdk/v1/latest-all"
	var etag string
	ticker := time.NewTicker(c.cfg.PollInterval)
	defer ticker.Stop()

	pollOnce := func() bool {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
		if err != nil {
			return false
		}
		req.Header.Set("Authorization", c.cfg.SDKKey)
		if etag != "" {
			req.Header.Set("If-None-Match", etag)
		}
		resp, err := c.http.Do(req)
		if err != nil {
			return true
		}
		defer func() { _ = resp.Body.Close() }()
		if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
			return false
		}
		if resp.StatusCode == http.StatusOK {
			if tag := resp.Header.Get("ETag"); tag != "" {
				etag = tag
			}
			var snap struct {
				Flags    map[string]*EvalFlag    `json:"flags"`
				Segments map[string]*EvalSegment `json:"segments"`
			}
			if json.NewDecoder(resp.Body).Decode(&snap) == nil {
				changed := c.store.Put(snap.Flags, snap.Segments)
				c.markReady()
				c.emitChanges(changed)
			}
		}
		return true
	}

	if !pollOnce() {
		return
	}
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if !pollOnce() {
				return
			}
		}
	}
}

// =============================================================================
// Official OpenFeature Go Provider (ADR-0004)
// =============================================================================

type OpenFeatureResolution[T any] struct {
	Value     T
	Variant   string
	Reason    string
	ErrorCode string
}

type OpenFeatureProvider struct {
	client *Client
}

func NewOpenFeatureProvider(client *Client) *OpenFeatureProvider {
	return &OpenFeatureProvider{client: client}
}

func (p *OpenFeatureProvider) MetadataName() string {
	return "Skotos Go Server OpenFeature Provider"
}

func (p *OpenFeatureProvider) BooleanEvaluation(flagKey string, defaultValue bool, evalCtx map[string]any) OpenFeatureResolution[bool] {
	d := p.client.BoolVariationDetail(flagKey, openFeatureMapToContext(evalCtx), defaultValue)
	val, _ := d.Value.(bool)
	return toOFResolution(val, d)
}

func (p *OpenFeatureProvider) StringEvaluation(flagKey string, defaultValue string, evalCtx map[string]any) OpenFeatureResolution[string] {
	d := p.client.StringVariationDetail(flagKey, openFeatureMapToContext(evalCtx), defaultValue)
	val, _ := d.Value.(string)
	return toOFResolution(val, d)
}

func (p *OpenFeatureProvider) FloatEvaluation(flagKey string, defaultValue float64, evalCtx map[string]any) OpenFeatureResolution[float64] {
	d := p.client.Float64VariationDetail(flagKey, openFeatureMapToContext(evalCtx), defaultValue)
	val, _ := toFloat64(d.Value)
	return toOFResolution(val, d)
}

func (p *OpenFeatureProvider) ObjectEvaluation(flagKey string, defaultValue any, evalCtx map[string]any) OpenFeatureResolution[any] {
	d := p.client.JSONVariationDetail(flagKey, openFeatureMapToContext(evalCtx), defaultValue)
	return toOFResolution(d.Value, d)
}

func openFeatureMapToContext(evalCtx map[string]any) Context {
	kind := "user"
	if k, ok := evalCtx["kind"].(string); ok && k != "" {
		kind = k
	}
	key, _ := evalCtx["targetingKey"].(string)
	if key == "" {
		key, _ = evalCtx["key"].(string)
	}
	sc := &SingleContext{
		Kind:       kind,
		Key:        key,
		Attributes: map[string]any{},
	}
	for k, v := range evalCtx {
		if k == "kind" || k == "key" || k == "targetingKey" {
			continue
		}
		if k == "name" {
			if s, ok := v.(string); ok {
				val := s
				sc.Name = &val
				continue
			}
		}
		if k == "anonymous" {
			if b, ok := v.(bool); ok {
				sc.Anonymous = b
				continue
			}
		}
		sc.Attributes[k] = v
	}
	return Context{Single: sc}
}

func toOFResolution[T any](val T, d EvaluationDetail) OpenFeatureResolution[T] {
	variant := ""
	if d.VariationIndex != nil {
		variant = fmt.Sprintf("%d", *d.VariationIndex)
	}
	reason := "DEFAULT"
	errCode := ""
	switch d.Reason.Kind {
	case "OFF", "PREREQUISITE_FAILED":
		reason = "DISABLED"
	case "TARGET_MATCH", "RULE_MATCH":
		reason = "TARGETING_MATCH"
	case "FALLTHROUGH":
		reason = "DEFAULT"
	case "ERROR":
		reason = "ERROR"
		switch d.Reason.ErrorKind {
		case "CLIENT_NOT_READY":
			errCode = "PROVIDER_NOT_READY"
		case "FLAG_NOT_FOUND":
			errCode = "FLAG_NOT_FOUND"
		case "MALFORMED_FLAG":
			errCode = "PARSE_ERROR"
		case "USER_NOT_SPECIFIED":
			errCode = "TARGETING_KEY_MISSING"
		case "WRONG_TYPE":
			errCode = "TYPE_MISMATCH"
		default:
			errCode = "GENERAL"
		}
	}
	return OpenFeatureResolution[T]{
		Value:     val,
		Variant:   variant,
		Reason:    reason,
		ErrorCode: errCode,
	}
}
