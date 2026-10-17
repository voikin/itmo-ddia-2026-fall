package api

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"

	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/internal/storage"
)

func NewRouter(store *storage.Memory, metrics, faults func(http.Handler) http.Handler, metricsHandler, routesHandler http.Handler) *chi.Mux {
	h := NewHandlers(store)

	r := chi.NewRouter()
	r.Use(metrics)
	r.Use(middleware.RequestID)
	r.Use(middleware.Recoverer)

	r.Get("/healthz", Health)
	r.Method(http.MethodGet, "/metrics", metricsHandler)
	r.Method(http.MethodGet, "/routes", routesHandler)

	r.Route("/items", func(r chi.Router) {
		r.Use(faults)
		r.Post("/", h.CreateItem)
		r.Get("/", h.ListItems)
		r.Get("/{id}", h.GetItem)
		r.Put("/{id}", h.UpdateItem)
	})

	return r
}
