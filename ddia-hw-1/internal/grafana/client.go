package grafana

import (
	"context"
	"fmt"
	"net/url"
	"os"
	"strings"
	"time"

	goapi "github.com/grafana/grafana-openapi-client-go/client"
	"github.com/go-openapi/strfmt"
)

type Auth struct {
	Token    string
	User     string
	Password string
}

func NewClient(rawURL string, auth Auth) (*goapi.GrafanaHTTPAPI, error) {
	u, err := url.Parse(rawURL)
	if err != nil {
		return nil, fmt.Errorf("parse grafana url %q: %w", rawURL, err)
	}
	cfg := &goapi.TransportConfig{
		Host:       u.Host,
		BasePath:   strings.TrimSuffix(u.Path, "/") + "/api",
		Schemes:    []string{u.Scheme},
		NumRetries: 3,
	}
	switch {
	case auth.Token != "":
		cfg.APIKey = auth.Token
	case auth.User != "":
		cfg.BasicAuth = url.UserPassword(auth.User, auth.Password)
	default:
		return nil, fmt.Errorf("grafana credentials are not set")
	}
	return goapi.NewHTTPClientWithConfig(strfmt.Default, cfg), nil
}

func WaitHealthy(ctx context.Context, c *goapi.GrafanaHTTPAPI) error {
	for {
		resp, err := c.Health.GetHealth()
		if err == nil && resp.Payload != nil && resp.Payload.Database == "ok" {
			return nil
		}
		select {
		case <-ctx.Done():
			return fmt.Errorf("grafana is not healthy: %w (last error: %v)", ctx.Err(), err)
		case <-time.After(2 * time.Second):
		}
	}
}

func ReadSecretFile(path string) (string, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("read secret file: %w", err)
	}
	v := strings.TrimSpace(string(b))
	if v == "" {
		return "", fmt.Errorf("secret file %s is empty", path)
	}
	return v, nil
}

func Env(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}

func MustEnv(key string) (string, error) {
	v := os.Getenv(key)
	if v == "" {
		return "", fmt.Errorf("environment variable %s is required", key)
	}
	return v, nil
}
