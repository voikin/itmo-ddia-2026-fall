package api

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"

	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/internal/storage"
)

func NewRouter(store *storage.Memory, metrics func(http.Handler) http.Handler, metricsHandler http.Handler) *chi.Mux {
	h := NewHandlers(store)

	r := chi.NewRouter()
	r.Use(metrics)
	r.Use(middleware.RequestID)
	r.Use(middleware.Recoverer)

	r.Get("/healthz", Health)
	r.Method(http.MethodGet, "/metrics", metricsHandler)

	r.Route("/items", func(r chi.Router) {
		r.Post("/", h.CreateItem)
		r.Get("/", h.ListItems)
		r.Get("/{id}", h.GetItem)
		r.Put("/{id}", h.UpdateItem)
	})

	return r
}
