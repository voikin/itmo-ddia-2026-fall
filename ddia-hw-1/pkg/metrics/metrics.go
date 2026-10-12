package metrics

import (
	"context"
	"errors"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
	"github.com/prometheus/client_golang/prometheus"
)

const (
	LabelPod        = "pod"
	LabelDatacenter = "datacenter"
	LabelDeployment = "deployment"
	LabelMethod     = "method"
	LabelRoute      = "route"
	LabelStatus     = "status"
	LabelClass      = "class"
	LabelSignal     = "signal"

	SignalLatency    = "latency"
	SignalTraffic    = "traffic"
	SignalErrors     = "errors"
	SignalSaturation = "saturation"

	MetricStarted  = "http_server_requests_started_total"
	MetricFinished = "http_server_requests_total"
	MetricDuration = "http_server_request_duration_seconds"
	MetricErrors   = "http_server_errors_total"
	MetricInflight = "http_server_inflight_requests"
	
	UnmatchedRoute = "unmatched"
	
	StatusClientClosed = 499
)

var DefaultBuckets = []float64{
	.001, .0025, .005, .01, .025, .05, .1, .2, .3, .5, .75, 1, 1.5, 2.5, 5,
}

type Labels struct {
	Pod        string
	Datacenter string
	Deployment string
}

func LabelsFromEnv() Labels {
	return Labels{
		Pod:        envOr("POD", "unknown"),
		Datacenter: envOr("DATACENTER", "unknown"),
		Deployment: envOr("DEPLOYMENT", "unknown"),
	}
}

type Option func(*config)

type config struct {
	buckets []float64
	skip    map[string]struct{}
}

func WithBuckets(buckets []float64) Option {
	return func(c *config) { c.buckets = buckets }
}

func WithSkip(routes ...string) Option {
	return func(c *config) {
		for _, r := range routes {
			c.skip[r] = struct{}{}
		}
	}
}

type Route struct {
	Method string `json:"method"`
	Route  string `json:"route"`
}

type Metrics struct {
	started  *prometheus.CounterVec
	finished *prometheus.CounterVec
	duration *prometheus.HistogramVec
	errors   *prometheus.CounterVec
	inflight prometheus.Gauge
	skip     map[string]struct{}
	routes   []Route
}

func New(labels Labels, reg prometheus.Registerer, opts ...Option) *Metrics {
	cfg := config{buckets: DefaultBuckets, skip: map[string]struct{}{}}
	for _, opt := range opts {
		opt(&cfg)
	}

	reg = prometheus.WrapRegistererWith(prometheus.Labels{
		LabelPod:        labels.Pod,
		LabelDatacenter: labels.Datacenter,
		LabelDeployment: labels.Deployment,
	}, reg)

	signal := func(s string) prometheus.Labels { return prometheus.Labels{LabelSignal: s} }

	m := &Metrics{
		started: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name:        MetricStarted,
			Help:        "HTTP requests that entered the handler chain.",
			ConstLabels: signal(SignalTraffic),
		}, []string{LabelMethod, LabelRoute}),
		finished: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name:        MetricFinished,
			Help:        "HTTP requests that finished, by status; 499 means the client went away.",
			ConstLabels: signal(SignalTraffic),
		}, []string{LabelMethod, LabelRoute, LabelStatus}),
		duration: prometheus.NewHistogramVec(prometheus.HistogramOpts{
			Name:        MetricDuration,
			Help:        "HTTP handler lifetime in seconds, excluding network transmission.",
			ConstLabels: signal(SignalLatency),
			Buckets:     cfg.buckets,
		}, []string{LabelMethod, LabelRoute, LabelStatus}),
		errors: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name:        MetricErrors,
			Help:        "HTTP requests that finished with a 4xx/5xx status, by status and class.",
			ConstLabels: signal(SignalErrors),
		}, []string{LabelMethod, LabelRoute, LabelStatus, LabelClass}),
		inflight: prometheus.NewGauge(prometheus.GaugeOpts{
			Name:        MetricInflight,
			Help:        "HTTP requests that entered the handler chain and have not finished yet.",
			ConstLabels: signal(SignalSaturation),
		}),
		skip: cfg.skip,
	}
	reg.MustRegister(m.started, m.finished, m.duration, m.errors, m.inflight)
	return m
}

func (m *Metrics) Init(routes chi.Routes) error {
	m.routes = m.routes[:0]
	return chi.Walk(routes, func(method, route string, _ http.Handler, _ ...func(http.Handler) http.Handler) error {
		route = normalize(route)
		if _, ok := m.skip[route]; ok {
			return nil
		}
		m.routes = append(m.routes, Route{Method: method, Route: route})
		m.started.WithLabelValues(method, route)
		for _, status := range []int{http.StatusOK, http.StatusInternalServerError} {
			code := strconv.Itoa(status)
			m.finished.WithLabelValues(method, route, code)
			m.duration.WithLabelValues(method, route, code)
		}
		m.errors.WithLabelValues(method, route, "500", "5xx")
		return nil
	})
}

func (m *Metrics) Routes() []Route {
	return append([]Route(nil), m.routes...)
}

func (m *Metrics) Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		route := routeOf(r)
		if _, ok := m.skip[route]; ok {
			next.ServeHTTP(w, r)
			return
		}

		m.started.WithLabelValues(r.Method, route).Inc()
		m.inflight.Inc()
		ww := middleware.NewWrapResponseWriter(w, r.ProtoMajor)

		defer func() {
			status := ww.Status()
			if errors.Is(r.Context().Err(), context.Canceled) {
				status = StatusClientClosed
			} else if status == 0 {
				status = http.StatusOK
			}
			m.observe(r.Method, route, status, time.Since(start))
			m.inflight.Dec()
		}()

		next.ServeHTTP(ww, r)
	})
}

func (m *Metrics) observe(method, route string, status int, elapsed time.Duration) {
	code := strconv.Itoa(status)
	m.duration.WithLabelValues(method, route, code).Observe(elapsed.Seconds())
	m.finished.WithLabelValues(method, route, code).Inc()
	if status >= 400 {
		m.errors.WithLabelValues(method, route, code, code[:1]+"xx").Inc()
	}
}

func routeOf(r *http.Request) string {
	rctx := chi.RouteContext(r.Context())
	if rctx == nil || rctx.Routes == nil {
		return UnmatchedRoute
	}
	pattern := rctx.Routes.Find(chi.NewRouteContext(), r.Method, r.URL.Path)
	if pattern == "" {
		return UnmatchedRoute
	}
	return normalize(pattern)
}

func normalize(pattern string) string {
	pattern = strings.ReplaceAll(pattern, "/*/", "/")
	if len(pattern) > 1 {
		pattern = strings.TrimSuffix(pattern, "/")
	}
	return pattern
}

func envOr(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}
