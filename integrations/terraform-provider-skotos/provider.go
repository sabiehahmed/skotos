package skotos

import (
	"context"
	"fmt"
	"sort"
	"sync"
)

// AttributeSchema describes a single Terraform/OpenTofu resource attribute.
type AttributeSchema struct {
	Type        string `json:"type"`
	Required    bool   `json:"required,omitempty"`
	Optional    bool   `json:"optional,omitempty"`
	Computed    bool   `json:"computed,omitempty"`
	Sensitive   bool   `json:"sensitive,omitempty"`
	Description string `json:"description,omitempty"`
}

// ResourceSchema describes a provider-managed Skotos resource.
type ResourceSchema struct {
	TypeName    string                     `json:"typeName"`
	Description string                     `json:"description"`
	Attributes  map[string]AttributeSchema `json:"attributes"`
}

// ResourceState holds materialized state for a managed resource instance.
type ResourceState struct {
	Type       string         `json:"type"`
	Name       string         `json:"name"`
	ID         string         `json:"id"`
	Attributes map[string]any `json:"attributes"`
}

// Resource defines the CRUD lifecycle implemented by each Skotos Terraform resource.
type Resource interface {
	Schema() ResourceSchema
	Create(ctx context.Context, client *Client, desired map[string]any) (string, map[string]any, error)
	Read(ctx context.Context, client *Client, id string, prior map[string]any) (map[string]any, bool, error)
	Update(ctx context.Context, client *Client, id string, desired map[string]any, prior map[string]any) (map[string]any, error)
	Delete(ctx context.Context, client *Client, id string, prior map[string]any) error
}

// PlanAction represents the change operation calculated during planning.
type PlanAction string

const (
	ActionNoop    PlanAction = "noop"
	ActionCreate  PlanAction = "create"
	ActionUpdate  PlanAction = "update"
	ActionDestroy PlanAction = "destroy"
)

// PlannedChange summarizes a single planned operation.
type PlannedChange struct {
	Type   string         `json:"type"`
	Name   string         `json:"name"`
	Action PlanAction     `json:"action"`
	Before map[string]any `json:"before,omitempty"`
	After  map[string]any `json:"after,omitempty"`
}

// DesiredResource declares a resource block in a Skotos IaC configuration.
type DesiredResource struct {
	Type       string         `json:"type"`
	Name       string         `json:"name"`
	Attributes map[string]any `json:"attributes"`
}

// Provider manages all registered Skotos Terraform/OpenTofu resources.
type Provider struct {
	mu        sync.RWMutex
	client    *Client
	resources map[string]Resource
}

// NewProvider creates a configured Skotos Terraform provider with all standard resources.
func NewProvider(client *Client) *Provider {
	p := &Provider{
		client:    client,
		resources: make(map[string]Resource),
	}
	p.Register(&ProjectResource{})
	p.Register(&EnvironmentResource{})
	p.Register(&FlagResource{})
	p.Register(&SegmentResource{})
	p.Register(&CustomRoleResource{})
	p.Register(&WebhookResource{})
	return p
}

// Register binds a resource implementation by its schema TypeName.
func (p *Provider) Register(r Resource) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.resources[r.Schema().TypeName] = r
}

// Schemas returns deterministic sorted schemas for all supported resources.
func (p *Provider) Schemas() []ResourceSchema {
	p.mu.RLock()
	defer p.mu.RUnlock()
	keys := make([]string, 0, len(p.resources))
	for k := range p.resources {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	out := make([]ResourceSchema, 0, len(keys))
	for _, k := range keys {
		out = append(out, p.resources[k].Schema())
	}
	return out
}

func (p *Provider) getResource(typeName string) (Resource, error) {
	p.mu.RLock()
	defer p.mu.RUnlock()
	r, ok := p.resources[typeName]
	if !ok {
		return nil, fmt.Errorf("unknown skotos resource type %q", typeName)
	}
	return r, nil
}

// Apply reconciles desired resources against prior state (Create / Refresh + Update / Destroy)
// and returns the new state map keyed by "type.name" plus the applied change log.
func (p *Provider) Apply(
	ctx context.Context,
	desired []DesiredResource,
	priorState map[string]ResourceState,
) (map[string]ResourceState, []PlannedChange, error) {
	nextState := make(map[string]ResourceState, len(desired))
	var changes []PlannedChange
	desiredKeys := make(map[string]struct{}, len(desired))

	for _, d := range desired {
		addr := d.Type + "." + d.Name
		desiredKeys[addr] = struct{}{}
		res, err := p.getResource(d.Type)
		if err != nil {
			return nil, nil, err
		}

		prior, exists := priorState[addr]
		if exists {
			live, found, err := res.Read(ctx, p.client, prior.ID, prior.Attributes)
			if err != nil {
				return nil, nil, fmt.Errorf("read %s: %w", addr, err)
			}
			if !found {
				exists = false
			} else {
				prior.Attributes = live
			}
		}

		if !exists {
			id, created, err := res.Create(ctx, p.client, d.Attributes)
			if err != nil {
				return nil, nil, fmt.Errorf("create %s: %w", addr, err)
			}
			nextState[addr] = ResourceState{
				Type:       d.Type,
				Name:       d.Name,
				ID:         id,
				Attributes: created,
			}
			changes = append(changes, PlannedChange{
				Type:   d.Type,
				Name:   d.Name,
				Action: ActionCreate,
				After:  created,
			})
			continue
		}

		if attrsMatchDesired(prior.Attributes, d.Attributes) {
			nextState[addr] = prior
			changes = append(changes, PlannedChange{
				Type:   d.Type,
				Name:   d.Name,
				Action: ActionNoop,
				Before: prior.Attributes,
				After:  prior.Attributes,
			})
			continue
		}

		updated, err := res.Update(ctx, p.client, prior.ID, d.Attributes, prior.Attributes)
		if err != nil {
			return nil, nil, fmt.Errorf("update %s: %w", addr, err)
		}
		nextState[addr] = ResourceState{
			Type:       d.Type,
			Name:       d.Name,
			ID:         prior.ID,
			Attributes: updated,
		}
		changes = append(changes, PlannedChange{
			Type:   d.Type,
			Name:   d.Name,
			Action: ActionUpdate,
			Before: prior.Attributes,
			After:  updated,
		})
	}

	// Destroy resources removed from desired configuration (in deterministic order)
	var toDestroy []string
	for addr := range priorState {
		if _, keep := desiredKeys[addr]; !keep {
			toDestroy = append(toDestroy, addr)
		}
	}
	sort.Strings(toDestroy)
	for _, addr := range toDestroy {
		prior := priorState[addr]
		res, err := p.getResource(prior.Type)
		if err != nil {
			return nil, nil, err
		}
		if err := res.Delete(ctx, p.client, prior.ID, prior.Attributes); err != nil {
			return nil, nil, fmt.Errorf("delete %s: %w", addr, err)
		}
		changes = append(changes, PlannedChange{
			Type:   prior.Type,
			Name:   prior.Name,
			Action: ActionDestroy,
			Before: prior.Attributes,
		})
	}

	return nextState, changes, nil
}

func attrsMatchDesired(current, desired map[string]any) bool {
	for k, dv := range desired {
		cv, ok := current[k]
		if !ok {
			return false
		}
		if fmt.Sprintf("%v", cv) != fmt.Sprintf("%v", dv) {
			return false
		}
	}
	return true
}
