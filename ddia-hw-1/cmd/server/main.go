package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/collectors"
	"github.com/prometheus/client_golang/prometheus/promhttp"

	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/internal/api"
	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/internal/storage"
	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/pkg/metrics"
)

func main() {
	addr := envOr("HTTP_ADDR", ":8080")

	if len(os.Args) > 1 && os.Args[1] == "healthcheck" {
		if err := healthcheck(addr); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}

	slog.SetDefault(slog.New(slog.NewJSONHandler(os.Stdout, nil)))

	if err := run(addr); err != nil {
		slog.Error("server stopped with error", "err", err)
		os.Exit(1)
	}
}

func run(addr string) error {
	labels := metrics.LabelsFromEnv()
	slog.Info("instance labels", "pod", labels.Pod, "datacenter", labels.Datacenter, "deployment", labels.Deployment)

	reg := prometheus.NewRegistry()
	reg.MustRegister(
		collectors.NewGoCollector(),
		collectors.NewProcessCollector(collectors.ProcessCollectorOpts{}),
	)
	gs := metrics.New(labels, reg, metrics.WithSkip("/healthz", "/metrics"))
	metricsHandler := promhttp.HandlerFor(reg, promhttp.HandlerOpts{Registry: reg})

	router := api.NewRouter(storage.NewMemory(), gs.Middleware, metricsHandler)
	if err := gs.Init(router); err != nil {
		return fmt.Errorf("init metrics: %w", err)
	}
	slog.Info("instrumented routes", "routes", gs.Routes())

	srv := &http.Server{
		Addr:              addr,
		Handler:           router,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      10 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	errCh := make(chan error, 1)
	go func() {
		slog.Info("starting http server", "addr", addr)
		errCh <- srv.ListenAndServe()
	}()

	select {
	case err := <-errCh:
		if !errors.Is(err, http.ErrServerClosed) {
			return err
		}
		return nil
	case <-ctx.Done():
	}

	slog.Info("shutting down")
	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return srv.Shutdown(shutdownCtx)
}

func healthcheck(addr string) error {
	client := http.Client{Timeout: 2 * time.Second}
	_, port, err := net.SplitHostPort(addr)
	if err != nil {
		return fmt.Errorf("parse HTTP_ADDR %q: %w", addr, err)
	}
	resp, err := client.Get("http://" + net.JoinHostPort("127.0.0.1", port) + "/healthz")
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("unhealthy: status %d", resp.StatusCode)
	}
	return nil
}

func envOr(key, def string) string {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		return v
	}
	return def
}
