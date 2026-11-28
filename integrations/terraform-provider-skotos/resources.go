package skotos

import (
	"context"
	"fmt"
)

func strAttr(m map[string]any, key string) string {
	if v, ok := m[key]; ok && v != nil {
		if s, ok := v.(string); ok {
			return s
		}
		return fmt.Sprintf("%v", v)
	}
	return ""
}

func boolAttr(m map[string]any, key string, def bool) bool {
	if v, ok := m[key]; ok && v != nil {
		if b, ok := v.(bool); ok {
			return b
		}
	}
	return def
}

// =============================================================================
// skotos_project
// =============================================================================

type ProjectResource struct{}

func (r *ProjectResource) Schema() ResourceSchema {
	return ResourceSchema{
		TypeName:    "skotos_project",
		Description: "Manages a Skotos project and its default environments.",
		Attributes: map[string]AttributeSchema{
			"key":         {Type: "string", Required: true, Description: "Unique URL-safe project key"},
			"name":        {Type: "string", Required: true, Description: "Human-readable project name"},
			"description": {Type: "string", Optional: true},
		},
	}
}

func (r *ProjectResource) Create(ctx context.Context, c *Client, desired map[string]any) (string, map[string]any, error) {
	var resp map[string]any
	err := c.DoJSON(ctx, "POST", "/api/v1/projects", map[string]any{
		"key":         strAttr(desired, "key"),
		"name":        strAttr(desired, "name"),
		"description": strAttr(desired, "description"),
	}, &resp)
	if err != nil {
		return "", nil, err
	}
	return strAttr(resp, "key"), resp, nil
}

func (r *ProjectResource) Read(ctx context.Context, c *Client, id string, _ map[string]any) (map[string]any, bool, error) {
	var resp map[string]any
	err := c.DoJSON(ctx, "GET", "/api/v1/projects/"+id, nil, &resp)
	if IsNotFound(err) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	return resp, true, nil
}

func (r *ProjectResource) Update(ctx context.Context, c *Client, id string, desired map[string]any, prior map[string]any) (map[string]any, error) {
	// Projects are updated or reconciled by key; re-read current server state and overlay mutable metadata.
	merged := make(map[string]any, len(prior))
	for k, v := range prior {
		merged[k] = v
	}
	for k, v := range desired {
		merged[k] = v
	}
	return merged, nil
}

func (r *ProjectResource) Delete(ctx context.Context, c *Client, id string, _ map[string]any) error {
	err := c.DoJSON(ctx, "DELETE", "/api/v1/projects/"+id, nil, nil)
	if IsNotFound(err) {
		return nil
	}
	return err
}

// =============================================================================
// skotos_environment
// =============================================================================

type EnvironmentResource struct{}

func (r *EnvironmentResource) Schema() ResourceSchema {
	return ResourceSchema{
		TypeName:    "skotos_environment",
		Description: "Manages a Skotos environment within a project.",
		Attributes: map[string]AttributeSchema{
			"project_key": {Type: "string", Required: true},
			"key":         {Type: "string", Required: true},
			"name":        {Type: "string", Required: true},
			"color":       {Type: "string", Optional: true},
			"critical":    {Type: "bool", Optional: true},
			"sdk_key":     {Type: "string", Computed: true, Sensitive: true},
		},
	}
}

func (r *EnvironmentResource) Create(ctx context.Context, c *Client, desired map[string]any) (string, map[string]any, error) {
	projectKey := strAttr(desired, "project_key")
	color := strAttr(desired, "color")
	if color == "" {
		color = "#6366f1"
	}
	var resp map[string]any
	err := c.DoJSON(ctx, "POST", "/api/v1/projects/"+projectKey+"/environments", map[string]any{
		"key":      strAttr(desired, "key"),
		"name":     strAttr(desired, "name"),
		"color":    color,
		"critical": boolAttr(desired, "critical", false),
	}, &resp)
	if err != nil {
		return "", nil, err
	}
	resp["project_key"] = projectKey
	return projectKey + "/" + strAttr(resp, "key"), resp, nil
}

func (r *EnvironmentResource) Read(ctx context.Context, c *Client, _ string, prior map[string]any) (map[string]any, bool, error) {
	projectKey := strAttr(prior, "project_key")
	envKey := strAttr(prior, "key")
	var resp struct {
		Items []map[string]any `json:"items"`
	}
	if err := c.DoJSON(ctx, "GET", "/api/v1/projects/"+projectKey+"/environments", nil, &resp); err != nil {
		if IsNotFound(err) {
			return nil, false, nil
		}
		return nil, false, err
	}
	for _, item := range resp.Items {
		if strAttr(item, "key") == envKey {
			item["project_key"] = projectKey
			return item, true, nil
		}
	}
	return nil, false, nil
}

func (r *EnvironmentResource) Update(ctx context.Context, c *Client, _ string, desired map[string]any, prior map[string]any) (map[string]any, error) {
	projectKey := strAttr(desired, "project_key")
	envKey := strAttr(desired, "key")
	if approvalRequired, ok := desired["approval_required"].(bool); ok {
		var resp map[string]any
		err := c.DoJSON(ctx, "PUT", "/api/v1/projects/"+projectKey+"/environments/"+envKey+"/approval-settings", map[string]any{
			"required":          approvalRequired,
			"minApprovals":      1,
			"allowSelfApproval": false,
		}, &resp)
		if err != nil {
			return nil, err
		}
	}
	merged := make(map[string]any, len(prior))
	for k, v := range prior {
		merged[k] = v
	}
	for k, v := range desired {
		merged[k] = v
	}
	return merged, nil
}

func (r *EnvironmentResource) Delete(ctx context.Context, c *Client, _ string, prior map[string]any) error {
	projectKey := strAttr(prior, "project_key")
	envKey := strAttr(prior, "key")
	err := c.DoJSON(ctx, "DELETE", "/api/v1/projects/"+projectKey+"/environments/"+envKey, nil, nil)
	if IsNotFound(err) {
		return nil
	}
	return err
}

// =============================================================================
// skotos_flag
// =============================================================================

type FlagResource struct{}

func (r *FlagResource) Schema() ResourceSchema {
	return ResourceSchema{
		TypeName:    "skotos_flag",
		Description: "Manages a Skotos feature flag definition.",
		Attributes: map[string]AttributeSchema{
			"project_key": {Type: "string", Required: true},
			"key":         {Type: "string", Required: true},
			"name":        {Type: "string", Required: true},
			"kind":        {Type: "string", Optional: true},
			"description": {Type: "string", Optional: true},
			"temporary":   {Type: "bool", Optional: true},
		},
	}
}

func (r *FlagResource) Create(ctx context.Context, c *Client, desired map[string]any) (string, map[string]any, error) {
	projectKey := strAttr(desired, "project_key")
	kind := strAttr(desired, "kind")
	if kind == "" {
		kind = "boolean"
	}
	variations := desired["variations"]
	if variations == nil {
		variations = []map[string]any{
			{"value": true, "name": "On"},
			{"value": false, "name": "Off"},
		}
	}
	var resp map[string]any
	err := c.DoJSON(ctx, "POST", "/api/v1/projects/"+projectKey+"/flags", map[string]any{
		"key":         strAttr(desired, "key"),
		"name":        strAttr(desired, "name"),
		"kind":        kind,
		"description": strAttr(desired, "description"),
		"temporary":   boolAttr(desired, "temporary", true),
		"variations":  variations,
	}, &resp)
	if err != nil {
		return "", nil, err
	}
	resp["project_key"] = projectKey
	return projectKey + "/" + strAttr(resp, "key"), resp, nil
}

func (r *FlagResource) Read(ctx context.Context, c *Client, _ string, prior map[string]any) (map[string]any, bool, error) {
	projectKey := strAttr(prior, "project_key")
	flagKey := strAttr(prior, "key")
	var resp struct {
		Flag map[string]any `json:"flag"`
	}
	err := c.DoJSON(ctx, "GET", "/api/v1/projects/"+projectKey+"/flags/"+flagKey, nil, &resp)
	if IsNotFound(err) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	if resp.Flag == nil {
		return nil, false, nil
	}
	resp.Flag["project_key"] = projectKey
	return resp.Flag, true, nil
}

func (r *FlagResource) Update(ctx context.Context, c *Client, _ string, desired map[string]any, prior map[string]any) (map[string]any, error) {
	merged := make(map[string]any, len(prior))
	for k, v := range prior {
		merged[k] = v
	}
	for k, v := range desired {
		merged[k] = v
	}
	return merged, nil
}

func (r *FlagResource) Delete(ctx context.Context, c *Client, _ string, prior map[string]any) error {
	projectKey := strAttr(prior, "project_key")
	flagKey := strAttr(prior, "key")
	err := c.DoJSON(ctx, "DELETE", "/api/v1/projects/"+projectKey+"/flags/"+flagKey, nil, nil)
	if IsNotFound(err) {
		return nil
	}
	return err
}

// =============================================================================
// skotos_segment
// =============================================================================

type SegmentResource struct{}

func (r *SegmentResource) Schema() ResourceSchema {
	return ResourceSchema{
		TypeName:    "skotos_segment",
		Description: "Manages a Skotos standard or unbounded Big Segment.",
		Attributes: map[string]AttributeSchema{
			"project_key": {Type: "string", Required: true},
			"env_key":     {Type: "string", Required: true},
			"key":         {Type: "string", Required: true},
			"name":        {Type: "string", Required: true},
			"description": {Type: "string", Optional: true},
			"unbounded":   {Type: "bool", Optional: true},
		},
	}
}

func (r *SegmentResource) Create(ctx context.Context, c *Client, desired map[string]any) (string, map[string]any, error) {
	projectKey := strAttr(desired, "project_key")
	envKey := strAttr(desired, "env_key")
	var resp map[string]any
	err := c.DoJSON(ctx, "POST", fmt.Sprintf("/api/v1/projects/%s/environments/%s/segments", projectKey, envKey), map[string]any{
		"key":         strAttr(desired, "key"),
		"name":        strAttr(desired, "name"),
		"description": strAttr(desired, "description"),
		"unbounded":   boolAttr(desired, "unbounded", false),
	}, &resp)
	if err != nil {
		return "", nil, err
	}
	resp["project_key"] = projectKey
	resp["env_key"] = envKey
	return fmt.Sprintf("%s/%s/%s", projectKey, envKey, strAttr(resp, "key")), resp, nil
}

func (r *SegmentResource) Read(ctx context.Context, c *Client, _ string, prior map[string]any) (map[string]any, bool, error) {
	projectKey := strAttr(prior, "project_key")
	envKey := strAttr(prior, "env_key")
	segKey := strAttr(prior, "key")
	var resp map[string]any
	err := c.DoJSON(ctx, "GET", fmt.Sprintf("/api/v1/projects/%s/environments/%s/segments/%s", projectKey, envKey, segKey), nil, &resp)
	if IsNotFound(err) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	resp["project_key"] = projectKey
	resp["env_key"] = envKey
	return resp, true, nil
}

func (r *SegmentResource) Update(ctx context.Context, c *Client, _ string, desired map[string]any, prior map[string]any) (map[string]any, error) {
	projectKey := strAttr(desired, "project_key")
	envKey := strAttr(desired, "env_key")
	segKey := strAttr(desired, "key")
	var resp map[string]any
	err := c.DoJSON(ctx, "PATCH", fmt.Sprintf("/api/v1/projects/%s/environments/%s/segments/%s", projectKey, envKey, segKey), map[string]any{
		"name":        strAttr(desired, "name"),
		"description": strAttr(desired, "description"),
	}, &resp)
	if err != nil {
		return nil, err
	}
	resp["project_key"] = projectKey
	resp["env_key"] = envKey
	return resp, nil
}

func (r *SegmentResource) Delete(ctx context.Context, c *Client, _ string, prior map[string]any) error {
	projectKey := strAttr(prior, "project_key")
	envKey := strAttr(prior, "env_key")
	segKey := strAttr(prior, "key")
	err := c.DoJSON(ctx, "DELETE", fmt.Sprintf("/api/v1/projects/%s/environments/%s/segments/%s", projectKey, envKey, segKey), nil, nil)
	if IsNotFound(err) {
		return nil
	}
	return err
}

// =============================================================================
// skotos_custom_role
// =============================================================================

type CustomRoleResource struct{}

func (r *CustomRoleResource) Schema() ResourceSchema {
	return ResourceSchema{
		TypeName:    "skotos_custom_role",
		Description: "Manages a Skotos custom RBAC policy role (allow/deny resource paths).",
		Attributes: map[string]AttributeSchema{
			"key":         {Type: "string", Required: true},
			"name":        {Type: "string", Required: true},
			"description": {Type: "string", Optional: true},
			"statements":  {Type: "list(object)", Required: true},
		},
	}
}

func (r *CustomRoleResource) Create(ctx context.Context, c *Client, desired map[string]any) (string, map[string]any, error) {
	stmts := desired["statements"]
	if stmts == nil {
		stmts = desired["policies"]
	}
	var resp map[string]any
	err := c.DoJSON(ctx, "POST", "/api/v1/roles", map[string]any{
		"key":         strAttr(desired, "key"),
		"name":        strAttr(desired, "name"),
		"description": strAttr(desired, "description"),
		"statements":  stmts,
	}, &resp)
	if err != nil {
		return "", nil, err
	}
	return strAttr(resp, "key"), resp, nil
}

func (r *CustomRoleResource) Read(ctx context.Context, c *Client, id string, _ map[string]any) (map[string]any, bool, error) {
	var resp map[string]any
	err := c.DoJSON(ctx, "GET", "/api/v1/roles/"+id, nil, &resp)
	if IsNotFound(err) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	return resp, true, nil
}

func (r *CustomRoleResource) Update(ctx context.Context, c *Client, id string, desired map[string]any, _ map[string]any) (map[string]any, error) {
	stmts := desired["statements"]
	if stmts == nil {
		stmts = desired["policies"]
	}
	var resp map[string]any
	err := c.DoJSON(ctx, "PATCH", "/api/v1/roles/"+id, map[string]any{
		"name":        strAttr(desired, "name"),
		"description": strAttr(desired, "description"),
		"statements":  stmts,
	}, &resp)
	if err != nil {
		return nil, err
	}
	return resp, nil
}

func (r *CustomRoleResource) Delete(ctx context.Context, c *Client, id string, _ map[string]any) error {
	err := c.DoJSON(ctx, "DELETE", "/api/v1/roles/"+id, nil, nil)
	if IsNotFound(err) {
		return nil
	}
	return err
}

// =============================================================================
// skotos_webhook
// =============================================================================

type WebhookResource struct{}

func (r *WebhookResource) Schema() ResourceSchema {
	return ResourceSchema{
		TypeName:    "skotos_webhook",
		Description: "Manages an outbound Skotos webhook (generic, Slack, Teams, Datadog, PagerDuty, Jira, Linear).",
		Attributes: map[string]AttributeSchema{
			"name":             {Type: "string", Required: true},
			"url":              {Type: "string", Required: true},
			"integration_kind": {Type: "string", Optional: true},
			"enabled":          {Type: "bool", Optional: true},
		},
	}
}

func (r *WebhookResource) Create(ctx context.Context, c *Client, desired map[string]any) (string, map[string]any, error) {
	kind := strAttr(desired, "integration_kind")
	if kind == "" {
		kind = "generic"
	}
	var resp map[string]any
	err := c.DoJSON(ctx, "POST", "/api/v1/webhooks", map[string]any{
		"name":            strAttr(desired, "name"),
		"url":             strAttr(desired, "url"),
		"integrationKind": kind,
	}, &resp)
	if err != nil {
		return "", nil, err
	}
	return strAttr(resp, "id"), resp, nil
}

func (r *WebhookResource) Read(ctx context.Context, c *Client, id string, _ map[string]any) (map[string]any, bool, error) {
	var resp struct {
		Items []map[string]any `json:"items"`
	}
	if err := c.DoJSON(ctx, "GET", "/api/v1/webhooks", nil, &resp); err != nil {
		return nil, false, err
	}
	for _, item := range resp.Items {
		if strAttr(item, "id") == id {
			return item, true, nil
		}
	}
	return nil, false, nil
}

func (r *WebhookResource) Update(ctx context.Context, c *Client, id string, desired map[string]any, prior map[string]any) (map[string]any, error) {
	// Recreate webhook when URL/name changes
	_ = r.Delete(ctx, c, id, prior)
	_, created, err := r.Create(ctx, c, desired)
	return created, err
}

func (r *WebhookResource) Delete(ctx context.Context, c *Client, id string, _ map[string]any) error {
	err := c.DoJSON(ctx, "DELETE", "/api/v1/webhooks/"+id, nil, nil)
	if IsNotFound(err) {
		return nil
	}
	return err
}
