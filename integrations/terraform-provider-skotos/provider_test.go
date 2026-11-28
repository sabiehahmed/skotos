package skotos

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
)

func TestProviderLifecycleAndDriftDetection(t *testing.T) {
	var mu sync.Mutex
	projects := map[string]map[string]any{}
	flags := map[string]map[string]any{}
	roles := map[string]map[string]any{}
	webhooks := map[string]map[string]any{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer test-token" {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		mu.Lock()
		defer mu.Unlock()
		w.Header().Set("Content-Type", "application/json")

		switch {
		case r.Method == "POST" && r.URL.Path == "/api/v1/projects":
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			projects[strAttr(body, "key")] = body
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(body)

		case r.Method == "GET" && strings.HasPrefix(r.URL.Path, "/api/v1/projects/") && !strings.Contains(strings.TrimPrefix(r.URL.Path, "/api/v1/projects/"), "/"):
			key := strings.TrimPrefix(r.URL.Path, "/api/v1/projects/")
			p, ok := projects[key]
			if !ok {
				w.WriteHeader(http.StatusNotFound)
				return
			}
			_ = json.NewEncoder(w).Encode(p)

		case r.Method == "DELETE" && strings.HasPrefix(r.URL.Path, "/api/v1/projects/") && !strings.Contains(strings.TrimPrefix(r.URL.Path, "/api/v1/projects/"), "/"):
			key := strings.TrimPrefix(r.URL.Path, "/api/v1/projects/")
			delete(projects, key)
			w.WriteHeader(http.StatusNoContent)

		case r.Method == "POST" && r.URL.Path == "/api/v1/projects/web/flags":
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			flags[strAttr(body, "key")] = body
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(body)

		case r.Method == "GET" && strings.HasPrefix(r.URL.Path, "/api/v1/projects/web/flags/"):
			key := strings.TrimPrefix(r.URL.Path, "/api/v1/projects/web/flags/")
			f, ok := flags[key]
			if !ok {
				w.WriteHeader(http.StatusNotFound)
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"flag": f})

		case r.Method == "DELETE" && strings.HasPrefix(r.URL.Path, "/api/v1/projects/web/flags/"):
			key := strings.TrimPrefix(r.URL.Path, "/api/v1/projects/web/flags/")
			delete(flags, key)
			w.WriteHeader(http.StatusNoContent)

		case r.Method == "POST" && r.URL.Path == "/api/v1/roles":
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			roles[strAttr(body, "key")] = body
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(body)

		case r.Method == "GET" && strings.HasPrefix(r.URL.Path, "/api/v1/roles/"):
			key := strings.TrimPrefix(r.URL.Path, "/api/v1/roles/")
			role, ok := roles[key]
			if !ok {
				w.WriteHeader(http.StatusNotFound)
				return
			}
			_ = json.NewEncoder(w).Encode(role)

		case r.Method == "PATCH" && strings.HasPrefix(r.URL.Path, "/api/v1/roles/"):
			key := strings.TrimPrefix(r.URL.Path, "/api/v1/roles/")
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			body["key"] = key
			roles[key] = body
			_ = json.NewEncoder(w).Encode(body)

		case r.Method == "DELETE" && strings.HasPrefix(r.URL.Path, "/api/v1/roles/"):
			key := strings.TrimPrefix(r.URL.Path, "/api/v1/roles/")
			delete(roles, key)
			w.WriteHeader(http.StatusNoContent)

		case r.Method == "POST" && r.URL.Path == "/api/v1/webhooks":
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			body["id"] = "wh-1"
			webhooks["wh-1"] = body
			w.WriteHeader(http.StatusCreated)
			_ = json.NewEncoder(w).Encode(body)

		case r.Method == "GET" && r.URL.Path == "/api/v1/webhooks":
			items := make([]map[string]any, 0, len(webhooks))
			for _, wh := range webhooks {
				items = append(items, wh)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"items": items})

		case r.Method == "DELETE" && strings.HasPrefix(r.URL.Path, "/api/v1/webhooks/"):
			id := strings.TrimPrefix(r.URL.Path, "/api/v1/webhooks/")
			delete(webhooks, id)
			w.WriteHeader(http.StatusNoContent)

		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer srv.Close()

	provider := NewProvider(NewClient(srv.URL, "test-token"))
	if len(provider.Schemas()) != 6 {
		t.Fatalf("expected 6 provider resource schemas, got %d", len(provider.Schemas()))
	}

	desired := []DesiredResource{
		{
			Type: "skotos_project",
			Name: "web",
			Attributes: map[string]any{
				"key":  "web",
				"name": "Web Platform",
			},
		},
		{
			Type: "skotos_flag",
			Name: "checkout_v2",
			Attributes: map[string]any{
				"project_key": "web",
				"key":         "checkout-v2",
				"name":        "Checkout V2",
			},
		},
		{
			Type: "skotos_custom_role",
			Name: "prod_guard",
			Attributes: map[string]any{
				"key":  "prod-guard",
				"name": "Production Guard",
				"statements": []map[string]any{
					{
						"effect":    "deny",
						"actions":   []string{"*"},
						"resources": []string{"proj/*:env/production:flag/*"},
					},
				},
			},
		},
		{
			Type: "skotos_webhook",
			Name: "slack_alerts",
			Attributes: map[string]any{
				"name":             "Slack Alerts",
				"url":              "https://hooks.slack.com/services/T00/B00/XXX",
				"integration_kind": "slack",
			},
		},
	}

	ctx := context.Background()
	state1, changes1, err := provider.Apply(ctx, desired, map[string]ResourceState{})
	if err != nil {
		t.Fatalf("initial apply failed: %v", err)
	}
	if len(state1) != 4 || len(changes1) != 4 {
		t.Fatalf("expected 4 created resources, got state=%d changes=%d", len(state1), len(changes1))
	}
	for _, ch := range changes1 {
		if ch.Action != ActionCreate {
			t.Fatalf("expected ActionCreate for %s.%s, got %s", ch.Type, ch.Name, ch.Action)
		}
	}

	// Update custom role name & verify in-place PATCH
	desired[2].Attributes["name"] = "Production Guard v2"
	state2, changes2, err := provider.Apply(ctx, desired, state1)
	if err != nil {
		t.Fatalf("update apply failed: %v", err)
	}
	if changes2[2].Action != ActionUpdate {
		t.Fatalf("expected ActionUpdate for custom role, got %s", changes2[2].Action)
	}
	if strAttr(state2["skotos_custom_role.prod_guard"].Attributes, "name") != "Production Guard v2" {
		t.Fatalf("expected updated role name in state")
	}

	// Destroy all resources cleanly
	state3, changes3, err := provider.Apply(ctx, nil, state2)
	if err != nil {
		t.Fatalf("destroy apply failed: %v", err)
	}
	if len(state3) != 0 || len(changes3) != 4 {
		t.Fatalf("expected 0 remaining resources and 4 destroys, got state=%d changes=%d", len(state3), len(changes3))
	}
	for _, ch := range changes3 {
		if ch.Action != ActionDestroy {
			t.Fatalf("expected ActionDestroy, got %s", ch.Action)
		}
	}
}
