package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"
)

const (
	defaultDeepSeekBaseURL = "https://api.deepseek.com"
	defaultDeepSeekModel   = "deepseek-chat"
)

type chatMessage struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

type chatRequest struct {
	Model          string          `json:"model"`
	Messages       []chatMessage   `json:"messages"`
	Temperature    float64         `json:"temperature"`
	ResponseFormat *responseFormat `json:"response_format,omitempty"`
}

type responseFormat struct {
	Type string `json:"type"`
}

type chatResponse struct {
	Choices []struct {
		Message chatMessage `json:"message"`
	} `json:"choices"`
	Error *struct {
		Message string `json:"message"`
	} `json:"error,omitempty"`
}

type llmConfig struct {
	APIKey  string
	BaseURL string
	Model   string
}

func loadLLMConfig() (llmConfig, error) {
	key := strings.TrimSpace(os.Getenv("DEEPSEEK_API_KEY"))
	if key == "" {
		return llmConfig{}, fmt.Errorf("DEEPSEEK_API_KEY is required for live decide; set it to your DeepSeek API token")
	}
	base := strings.TrimSpace(os.Getenv("DEEPSEEK_BASE_URL"))
	if base == "" {
		base = defaultDeepSeekBaseURL
	}
	base = strings.TrimRight(base, "/")
	model := strings.TrimSpace(os.Getenv("DEEPSEEK_MODEL"))
	if model == "" {
		model = defaultDeepSeekModel
	}
	return llmConfig{APIKey: key, BaseURL: base, Model: model}, nil
}

func chatCompletion(cfg llmConfig, messages []chatMessage) (string, error) {
	body, err := json.Marshal(chatRequest{
		Model:       cfg.Model,
		Messages:    messages,
		Temperature: 0.2,
		ResponseFormat: &responseFormat{
			Type: "json_object",
		},
	})
	if err != nil {
		return "", err
	}
	req, err := http.NewRequest(http.MethodPost, cfg.BaseURL+"/chat/completions", bytes.NewReader(body))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+cfg.APIKey)

	client := &http.Client{Timeout: 120 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", fmt.Errorf("deepseek http %d: %s", resp.StatusCode, truncate(string(raw), 800))
	}
	var parsed chatResponse
	if err := json.Unmarshal(raw, &parsed); err != nil {
		return "", fmt.Errorf("decode deepseek response: %w; body=%s", err, truncate(string(raw), 400))
	}
	if parsed.Error != nil {
		return "", fmt.Errorf("deepseek api error: %s", parsed.Error.Message)
	}
	if len(parsed.Choices) == 0 {
		return "", fmt.Errorf("deepseek returned no choices")
	}
	return strings.TrimSpace(parsed.Choices[0].Message.Content), nil
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "..."
}
