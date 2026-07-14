package main

import (
	"os"

	axiom "github.com/SupUnicornAlpha/axiom_kernal/sdks/go"
	"github.com/SupUnicornAlpha/axiom_kernal/sdks/go/workspace"
)

type codingPlanner struct{}

func (codingPlanner) Plan(string) ([]axiom.Decision, error)                { return deterministicPlan(), nil }
func (codingPlanner) Decide(obs axiom.Observation) (axiom.Decision, error) { return decideWithLLM(obs) }

func main() {
	tools := workspace.NewCodingRegistry([][]string{
		{"cargo", "test"}, {"cargo", "check"}, {"cargo", "build"},
		{"go", "test"}, {"go", "build"}, {"python", "-m", "pytest"}, {"python3", "-m", "pytest"},
	})
	sidecar := axiom.Sidecar{SystemPrompt: SystemPrompt, Planner: codingPlanner{}, Tools: tools}
	os.Exit(sidecar.Run(os.Args[1:], os.Stdin, os.Stdout, os.Stderr))
}
