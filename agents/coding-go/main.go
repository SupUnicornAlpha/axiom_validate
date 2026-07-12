package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

type Decision struct {
	Kind         string `json:"kind"`
	CapabilityID string `json:"capability_id,omitempty"`
	Input        string `json:"input,omitempty"`
	Content      string `json:"content,omitempty"`
}
type Request struct {
	Path    string   `json:"path"`
	Pattern string   `json:"pattern"`
	Old     string   `json:"old"`
	New     string   `json:"new"`
	Content string   `json:"content"`
	Argv    []string `json:"argv"`
}

func main() {
	if len(os.Args) < 2 {
		fail(errors.New("usage: plan|prompt|tool"))
	}
	switch os.Args[1] {
	case "prompt":
		fmt.Print(SystemPrompt)
	case "plan":
		emitPlan()
	case "tool":
		if len(os.Args) != 6 {
			fail(errors.New("tool requires name root input"))
		}
		output, err := runTool(os.Args[2], os.Args[3], os.Args[4], os.Args[5])
		if err != nil {
			fail(err)
		}
		fmt.Print(output)
	default:
		fail(errors.New("unknown command"))
	}
}

func emitPlan() {
	plan := []Decision{
		{Kind: "invoke", CapabilityID: "coding/list", Input: "."},
		{Kind: "invoke", CapabilityID: "coding/read", Input: "src/lib.rs"},
		{Kind: "invoke", CapabilityID: "coding/grep", Input: `{"path":"src/lib.rs","pattern":"left - right"}`},
		{Kind: "invoke", CapabilityID: "coding/edit", Input: `{"path":"src/lib.rs","old":"left - right","new":"left + right"}`},
		{Kind: "invoke", CapabilityID: "coding/bash", Input: `{"argv":["cargo","test","--offline"]}`},
		{Kind: "respond", Content: "Fixed the calculator implementation and verified the test suite passes."},
		{Kind: "finish"},
	}
	json.NewEncoder(os.Stdout).Encode(plan)
}

func runTool(name, root, permission, input string) (string, error) {
	if permission != "invoke" {
		return "", errors.New("permission denied")
	}
	root, err := filepath.EvalSymlinks(root)
	if err != nil {
		return "", err
	}
	switch name {
	case "list":
		p, err := secure(root, input, true)
		if err != nil {
			return "", err
		}
		entries, err := os.ReadDir(p)
		if err != nil {
			return "", err
		}
		names := []string{}
		for _, e := range entries {
			names = append(names, e.Name())
		}
		sort.Strings(names)
		return strings.Join(names, "\n"), nil
	case "read":
		p, err := secure(root, input, true)
		if err != nil {
			return "", err
		}
		b, err := os.ReadFile(p)
		return string(b), err
	case "grep":
		var r Request
		if err := json.Unmarshal([]byte(input), &r); err != nil {
			return "", err
		}
		p, err := secure(root, r.Path, true)
		if err != nil {
			return "", err
		}
		b, err := os.ReadFile(p)
		if err != nil {
			return "", err
		}
		out := []string{}
		for i, line := range strings.Split(string(b), "\n") {
			if strings.Contains(line, r.Pattern) {
				out = append(out, fmt.Sprintf("%d:%s", i+1, line))
			}
		}
		return strings.Join(out, "\n"), nil
	case "edit":
		var r Request
		if err := json.Unmarshal([]byte(input), &r); err != nil {
			return "", err
		}
		p, err := secure(root, r.Path, true)
		if err != nil {
			return "", err
		}
		b, err := os.ReadFile(p)
		if err != nil {
			return "", err
		}
		if strings.Count(string(b), r.Old) != 1 {
			return "", errors.New("edit requires exactly one match")
		}
		return p, os.WriteFile(p, []byte(strings.Replace(string(b), r.Old, r.New, 1)), 0644)
	case "write":
		var r Request
		if err := json.Unmarshal([]byte(input), &r); err != nil {
			return "", err
		}
		p, err := secure(root, r.Path, false)
		if err != nil {
			return "", err
		}
		if err := os.MkdirAll(filepath.Dir(p), 0755); err != nil {
			return "", err
		}
		return p, os.WriteFile(p, []byte(r.Content), 0644)
	case "bash":
		var r Request
		if err := json.Unmarshal([]byte(input), &r); err != nil {
			return "", err
		}
		if strings.Join(r.Argv, " ") != "cargo test --offline" {
			return "", errors.New("bash command denied")
		}
		cmd := exec.Command(r.Argv[0], r.Argv[1:]...)
		cmd.Dir = root
		b, err := cmd.CombinedOutput()
		if err != nil {
			return "", fmt.Errorf("%w: %s", err, b)
		}
		return string(b), nil
	default:
		return "", errors.New("unknown tool")
	}
}

func secure(root, relative string, existing bool) (string, error) {
	if filepath.IsAbs(relative) {
		return "", errors.New("absolute path denied")
	}
	clean := filepath.Clean(relative)
	if clean == ".." || strings.HasPrefix(clean, ".."+string(os.PathSeparator)) {
		return "", errors.New("workspace escape denied")
	}
	candidate := filepath.Join(root, clean)
	check := candidate
	if !existing {
		check = filepath.Dir(candidate)
	}
	resolved, err := filepath.EvalSymlinks(check)
	if err != nil {
		return "", err
	}
	rel, err := filepath.Rel(root, resolved)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
		return "", errors.New("symlink escape denied")
	}
	return candidate, nil
}
func fail(err error) { fmt.Fprintln(os.Stderr, err); os.Exit(1) }
