package api

import (
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"

	"github.com/go-chi/chi/v5"

	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/internal/storage"
)

const maxBodyBytes = 1 << 20

type itemRequest struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}

type errorResponse struct {
	Error string `json:"error"`
}

type Handlers struct {
	store *storage.Memory
}

func NewHandlers(store *storage.Memory) *Handlers {
	return &Handlers{store: store}
}

func (h *Handlers) CreateItem(w http.ResponseWriter, r *http.Request) {
	req, ok := decodeItemRequest(w, r)
	if !ok {
		return
	}
	writeJSON(w, http.StatusCreated, h.store.Create(req.Name, req.Value))
}

func (h *Handlers) ListItems(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, h.store.List())
}

func (h *Handlers) GetItem(w http.ResponseWriter, r *http.Request) {
	item, err := h.store.Get(chi.URLParam(r, "id"))
	if err != nil {
		writeStorageError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, item)
}

func (h *Handlers) UpdateItem(w http.ResponseWriter, r *http.Request) {
	req, ok := decodeItemRequest(w, r)
	if !ok {
		return
	}
	item, err := h.store.Update(chi.URLParam(r, "id"), req.Name, req.Value)
	if err != nil {
		writeStorageError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, item)
}

func Health(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func decodeItemRequest(w http.ResponseWriter, r *http.Request) (itemRequest, bool) {
	var req itemRequest
	dec := json.NewDecoder(http.MaxBytesReader(w, r.Body, maxBodyBytes))
	dec.DisallowUnknownFields()
	if err := dec.Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON body: "+err.Error())
		return req, false
	}
	if req.Name == "" {
		writeError(w, http.StatusUnprocessableEntity, "field 'name' is required")
		return req, false
	}
	return req, true
}

func writeStorageError(w http.ResponseWriter, err error) {
	if errors.Is(err, storage.ErrNotFound) {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}
	writeError(w, http.StatusInternalServerError, "internal error")
}

func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, errorResponse{Error: msg})
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if err := json.NewEncoder(w).Encode(body); err != nil {
		slog.Error("failed to encode response", "err", err)
	}
}
