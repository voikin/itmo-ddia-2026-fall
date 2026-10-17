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
	"strconv"
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
	gs := metrics.New(labels, reg, metrics.WithSkip("/healthz", "/metrics", "/routes"))
	metricsHandler := promhttp.HandlerFor(reg, promhttp.HandlerOpts{Registry: reg})

	faults, err := faultsFromEnv()
	if err != nil {
		return err
	}
	slog.Info("fault injection", "base_latency", faults.BaseLatency, "jitter", faults.Jitter,
		"tail_every", faults.TailEvery, "tail_extra", faults.TailExtra, "error_every", faults.ErrorEvery)

	router := api.NewRouter(storage.NewMemory(), gs.Middleware, faults.Middleware, metricsHandler, api.Routes(gs.Routes))
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

func faultsFromEnv() (*api.Faults, error) {
	f := &api.Faults{}
	var err error
	if f.BaseLatency, err = durationEnv("FAULT_BASE_LATENCY", 0); err != nil {
		return nil, err
	}
	if f.Jitter, err = durationEnv("FAULT_JITTER", 0); err != nil {
		return nil, err
	}
	if f.TailExtra, err = durationEnv("FAULT_TAIL_EXTRA", 0); err != nil {
		return nil, err
	}
	if f.TailEvery, err = uintEnv("FAULT_TAIL_EVERY"); err != nil {
		return nil, err
	}
	if f.ErrorEvery, err = uintEnv("FAULT_ERROR_EVERY"); err != nil {
		return nil, err
	}
	return f, f.Validate()
}

func durationEnv(key string, def time.Duration) (time.Duration, error) {
	v := os.Getenv(key)
	if v == "" {
		return def, nil
	}
	d, err := time.ParseDuration(v)
	if err != nil {
		return 0, fmt.Errorf("%s: %w", key, err)
	}
	return d, nil
}

func uintEnv(key string) (uint64, error) {
	v := os.Getenv(key)
	if v == "" {
		return 0, nil
	}
	n, err := strconv.ParseUint(v, 10, 64)
	if err != nil {
		return 0, fmt.Errorf("%s: %w", key, err)
	}
	return n, nil
}

func envOr(key, def string) string {
	if v, ok := os.LookupEnv(key); ok && v != "" {
		return v
	}
	return def
}
