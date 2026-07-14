package main

import (
	"encoding/json"
	"fmt"
	"strings"

	axiom "github.com/SupUnicornAlpha/axiom_kernal/sdks/go"
)

type Decision = axiom.Decision
type Observation = axiom.Observation

const decisionSchema = `Return ONLY one JSON object with one of these shapes:
{"kind":"invoke","capability_id":"coding/list|coding/read|coding/grep|coding/edit|coding/write|coding/bash","input":"..."}
{"kind":"respond","content":"final summary for the user"}
{"kind":"finish"}

Tool input formats:
- coding/list: relative path string, e.g. "."
- coding/read: relative path string, e.g. "src/lib.rs"
- coding/grep: JSON string {"path":"...","pattern":"..."}
- coding/edit: JSON string {"path":"...","old":"...","new":"..."} (old must match exactly once)
- coding/write: JSON string {"path":"...","content":"..."}
- coding/bash: JSON string {"argv":["cargo","test","--offline"]} (argv allowlisted; no shell)

Rules:
- Prefer inspect (list/read/grep) before edit.
- After a successful verification, emit respond then finish on a later turn — or respond now if certainty is high, then finish next.
- Never invent capability ids outside the list above.
- If a tool fails, revise the approach; do not repeat the identical failing call blindly.`

func decideWithLLM(obs Observation) (Decision, error) {
	cfg, err := loadLLMConfig()
	if err != nil {
		return Decision{}, err
	}
	user := buildObservationPrompt(obs)
	content, err := chatCompletion(cfg, []chatMessage{
		{Role: "system", Content: SystemPrompt + "\n\n" + decisionSchema},
		{Role: "user", Content: user},
	})
	if err != nil {
		return Decision{}, err
	}
	decision, err := parseDecision(content)
	if err != nil {
		// One repair attempt with the parse error.
		repair, repairErr := chatCompletion(cfg, []chatMessage{
			{Role: "system", Content: decisionSchema},
			{Role: "user", Content: "Your previous reply was invalid:\n" + content + "\n\nError: " + err.Error() + "\nReturn a single valid decision JSON object."},
		})
		if repairErr != nil {
			return Decision{}, fmt.Errorf("parse decision: %w; repair failed: %v", err, repairErr)
		}
		return parseDecision(repair)
	}
	return decision, nil
}

func buildObservationPrompt(obs Observation) string {
	var b strings.Builder
	b.WriteString("TASK\n")
	b.WriteString(obs.Task)
	b.WriteString("\n\n")
	if obs.WorkspaceRoot != "" {
		b.WriteString("WORKSPACE_ROOT (tools already scoped): ")
		b.WriteString(obs.WorkspaceRoot)
		b.WriteString("\n\n")
	}
	b.WriteString(fmt.Sprintf("TURN next_step_index=%d\n\n", obs.NextStepIndex))
	if len(obs.VisibleCapabilities) > 0 {
		b.WriteString("VISIBLE_CAPABILITIES\n")
		b.WriteString(strings.Join(obs.VisibleCapabilities, ", "))
		b.WriteString("\n\n")
	}
	if len(obs.Messages) > 0 {
		b.WriteString("MESSAGES (assistant summaries already committed)\n")
		for _, m := range obs.Messages {
			b.WriteString(fmt.Sprintf("- %s: %s\n", m.Role, truncate(m.Content, 2000)))
		}
		b.WriteString("\n")
	}
	if len(obs.Outputs) > 0 {
		b.WriteString("TOOL_OUTPUTS (chronological; each item is one prior invoke result)\n")
		for i, out := range obs.Outputs {
			b.WriteString(fmt.Sprintf("--- output[%d] ---\n%s\n", i, truncate(out, 4000)))
		}
		b.WriteString("\n")
	} else {
		b.WriteString("TOOL_OUTPUTS\n(none yet — start by listing or reading the workspace)\n\n")
	}
	if len(obs.DeniedActions) > 0 {
		b.WriteString("DENIED_ACTIONS\n")
		for _, d := range obs.DeniedActions {
			b.WriteString("- ")
			b.WriteString(d)
			b.WriteString("\n")
		}
		b.WriteString("\n")
	}
	b.WriteString("Decide the single next action now.")
	return b.String()
}

func parseDecision(content string) (Decision, error) {
	content = strings.TrimSpace(content)
	if content == "" {
		return Decision{}, fmt.Errorf("empty model content")
	}
	// Strip optional markdown fences.
	if strings.HasPrefix(content, "```") {
		content = strings.TrimPrefix(content, "```json")
		content = strings.TrimPrefix(content, "```")
		content = strings.TrimSuffix(content, "```")
		content = strings.TrimSpace(content)
	}
	var d Decision
	if err := json.Unmarshal([]byte(content), &d); err != nil {
		return Decision{}, fmt.Errorf("json unmarshal: %w", err)
	}
	if err := axiom.ValidateDecision(d, "coding/"); err != nil {
		return Decision{}, err
	}
	return d, nil
}

// deterministicPlan keeps CI validation offline and reproducible.
func deterministicPlan() []Decision {
	return []Decision{
		{Kind: "invoke", CapabilityID: "coding/list", Input: "."},
		{Kind: "invoke", CapabilityID: "coding/read", Input: "src/lib.rs"},
		{Kind: "invoke", CapabilityID: "coding/grep", Input: `{"path":"src/lib.rs","pattern":"left - right"}`},
		{Kind: "invoke", CapabilityID: "coding/edit", Input: `{"path":"src/lib.rs","old":"left - right","new":"left + right"}`},
		{Kind: "invoke", CapabilityID: "coding/bash", Input: `{"argv":["cargo","test","--offline"]}`},
		{Kind: "respond", Content: "Fixed the calculator implementation and verified the test suite passes."},
		{Kind: "finish"},
	}
}
