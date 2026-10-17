package main

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"time"

	goapi "github.com/grafana/grafana-openapi-client-go/client"
	"github.com/grafana/grafana-openapi-client-go/client/service_accounts"
	"github.com/grafana/grafana-openapi-client-go/models"

	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/internal/grafana"
)

func main() {
	slog.SetDefault(slog.New(slog.NewJSONHandler(os.Stdout, nil)))
	if err := run(); err != nil {
		slog.Error("grafana-token failed", "err", err)
		os.Exit(1)
	}
}

func run() error {
	passwordFile, err := grafana.MustEnv("GRAFANA_ADMIN_PASSWORD_FILE")
	if err != nil {
		return err
	}
	tokenFile, err := grafana.MustEnv("GRAFANA_TOKEN_FILE")
	if err != nil {
		return err
	}
	password, err := grafana.ReadSecretFile(passwordFile)
	if err != nil {
		return err
	}

	c, err := grafana.NewClient(grafana.Env("GRAFANA_URL", "http://grafana:3000"), grafana.Auth{
		User:     grafana.Env("GRAFANA_ADMIN_USER", "admin"),
		Password: password,
	})
	if err != nil {
		return err
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	if err := grafana.WaitHealthy(ctx, c); err != nil {
		return err
	}

	name := grafana.Env("GRAFANA_SA_NAME", "dashgen")
	saID, err := ensureServiceAccount(c, name)
	if err != nil {
		return err
	}
	if err := revokeTokens(c, saID); err != nil {
		return err
	}

	token, err := c.ServiceAccounts.CreateToken(service_accounts.NewCreateTokenParams().
		WithServiceAccountID(saID).
		WithBody(&models.AddServiceAccountTokenCommand{
			Name:          fmt.Sprintf("%s-%d", name, time.Now().Unix()),
			SecondsToLive: int64((24 * time.Hour).Seconds()),
		}))
	if err != nil {
		return fmt.Errorf("create token: %w", err)
	}
	if err := writeSecret(tokenFile, token.Payload.Key); err != nil {
		return err
	}

	slog.Info("service account token issued", "service_account", name, "id", saID, "file", tokenFile)
	return nil
}

func ensureServiceAccount(c *goapi.GrafanaHTTPAPI, name string) (int64, error) {
	found, err := c.ServiceAccounts.SearchOrgServiceAccountsWithPaging(
		service_accounts.NewSearchOrgServiceAccountsWithPagingParams().WithQuery(&name))
	if err != nil {
		return 0, fmt.Errorf("search service accounts: %w", err)
	}
	for _, sa := range found.Payload.ServiceAccounts {
		if sa.Name == name {
			return sa.ID, nil
		}
	}

	created, err := c.ServiceAccounts.CreateServiceAccount(service_accounts.NewCreateServiceAccountParams().
		WithBody(&models.CreateServiceAccountForm{Name: name, Role: "Editor"}))
	if err != nil {
		return 0, fmt.Errorf("create service account: %w", err)
	}
	return created.Payload.ID, nil
}

func revokeTokens(c *goapi.GrafanaHTTPAPI, saID int64) error {
	tokens, err := c.ServiceAccounts.ListTokens(saID)
	if err != nil {
		return fmt.Errorf("list tokens: %w", err)
	}
	for _, t := range tokens.Payload {
		if _, err := c.ServiceAccounts.DeleteToken(t.ID, saID); err != nil {
			return fmt.Errorf("delete token %d: %w", t.ID, err)
		}
	}
	return nil
}

func writeSecret(path, value string) error {
	tmp, err := os.CreateTemp(filepath.Dir(path), ".token-*")
	if err != nil {
		return fmt.Errorf("create temp token file: %w", err)
	}
	defer os.Remove(tmp.Name())
	if err := tmp.Chmod(0o600); err != nil {
		tmp.Close()
		return err
	}
	if _, err := tmp.WriteString(value); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmp.Name(), path)
}
