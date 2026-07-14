package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
)

func main() {
	if len(os.Args) < 2 {
		fail(errors.New("usage: prompt | plan [task] | decide [observation.json|-] | tool <name> <root> <permission> <input>"))
	}
	switch os.Args[1] {
	case "prompt":
		fmt.Print(SystemPrompt)
	case "plan":
		// Offline deterministic script for axiom_validate regression.
		if err := json.NewEncoder(os.Stdout).Encode(deterministicPlan()); err != nil {
			fail(err)
		}
	case "decide":
		raw := readObservationInput(os.Args[2:])
		emitDecisionFromObservation(raw)
	case "tool":
		if len(os.Args) != 6 {
			fail(errors.New("tool requires: tool <name> <root> <permission> <input>"))
		}
		output, err := runTool(os.Args[2], os.Args[3], os.Args[4], os.Args[5])
		if err != nil {
			fail(err)
		}
		fmt.Print(output)
	default:
		fail(fmt.Errorf("unknown command %q", os.Args[1]))
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
