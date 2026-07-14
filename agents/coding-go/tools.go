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

type Request struct {
	Path    string   `json:"path"`
	Pattern string   `json:"pattern"`
	Old     string   `json:"old"`
	New     string   `json:"new"`
	Content string   `json:"content"`
	Argv    []string `json:"argv"`
}

// Allowed bash argv prefixes for verification. Commands are argv-based (no shell).
var bashAllowlist = [][]string{
	{"cargo", "test"},
	{"cargo", "check"},
	{"cargo", "build"},
	{"go", "test"},
	{"go", "build"},
	{"python", "-m", "pytest"},
	{"python3", "-m", "pytest"},
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
		if !bashAllowed(r.Argv) {
			return "", fmt.Errorf("bash command denied: %s", strings.Join(r.Argv, " "))
		}
		cmd := exec.Command(r.Argv[0], r.Argv[1:]...)
		cmd.Dir = root
		b, err := cmd.CombinedOutput()
		// Return stdout/stderr to the model even on non-zero exit so ReAct can recover.
		if err != nil {
			return fmt.Sprintf("exit_error: %v\n%s", err, b), nil
		}
		return string(b), nil
	default:
		return "", errors.New("unknown tool")
	}
}

func bashAllowed(argv []string) bool {
	if len(argv) == 0 {
		return false
	}
	for _, prefix := range bashAllowlist {
		if len(argv) < len(prefix) {
			continue
		}
		ok := true
		for i, part := range prefix {
			if argv[i] != part {
				ok = false
				break
			}
		}
		if ok {
			return true
		}
	}
	return false
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
