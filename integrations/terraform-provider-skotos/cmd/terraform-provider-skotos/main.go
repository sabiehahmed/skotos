package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"

	skotos "github.com/sabiehahmed/skotos/integrations/terraform-provider-skotos"
)

func main() {
	if len(os.Args) < 2 || os.Args[1] == "schema" {
		p := skotos.NewProvider(skotos.NewClient("", ""))
		enc := json.NewEncoder(os.Stdout)
		enc.SetIndent("", "  ")
		if err := enc.Encode(map[string]any{
			"provider":  "terraform-provider-skotos",
			"version":   "0.1.0",
			"resources": p.Schemas(),
		}); err != nil {
			fmt.Fprintf(os.Stderr, "encode schema: %v\n", err)
			os.Exit(1)
		}
		return
	}

	if os.Args[1] == "apply" && len(os.Args) >= 3 {
		raw, err := os.ReadFile(os.Args[2])
		if err != nil {
			fmt.Fprintf(os.Stderr, "read config: %v\n", err)
			os.Exit(1)
		}
		var desired []skotos.DesiredResource
		if err := json.Unmarshal(raw, &desired); err != nil {
			fmt.Fprintf(os.Stderr, "parse config: %v\n", err)
			os.Exit(1)
		}
		baseURL := os.Getenv("SKOTOS_API_URL")
		if baseURL == "" {
			baseURL = "http://127.0.0.1:8080"
		}
		p := skotos.NewProvider(skotos.NewClient(baseURL, os.Getenv("SKOTOS_API_TOKEN")))
		nextState, changes, err := p.Apply(context.Background(), desired, map[string]skotos.ResourceState{})
		if err != nil {
			fmt.Fprintf(os.Stderr, "apply failed: %v\n", err)
			os.Exit(1)
		}
		enc := json.NewEncoder(os.Stdout)
		enc.SetIndent("", "  ")
		_ = enc.Encode(map[string]any{"state": nextState, "changes": changes})
		return
	}

	fmt.Fprintf(os.Stderr, "usage: terraform-provider-skotos [schema | apply <config.json>]\n")
	os.Exit(2)
}
