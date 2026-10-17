package api

import (
	"fmt"
	"math/rand/v2"
	"net/http"
	"sync/atomic"
	"time"
)

type Faults struct {
	BaseLatency time.Duration
	Jitter      time.Duration
	TailEvery   uint64
	TailExtra   time.Duration
	ErrorEvery  uint64

	seq atomic.Uint64
}

func (f *Faults) Validate() error {
	if f.BaseLatency < 0 || f.Jitter < 0 || f.TailExtra < 0 {
		return fmt.Errorf("fault durations must be non-negative")
	}
	if f.BaseLatency > 5*time.Second || f.TailExtra > 5*time.Second {
		return fmt.Errorf("fault durations must not exceed 5s")
	}
	return nil
}

func (f *Faults) Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seq := f.seq.Add(1)

		delay := f.BaseLatency
		if f.Jitter > 0 {
			delay += rand.N(f.Jitter)
		}
		if f.TailEvery > 0 && seq%f.TailEvery == 0 {
			delay += f.TailExtra
		}
		if delay > 0 {
			t := time.NewTimer(delay)
			select {
			case <-t.C:
			case <-r.Context().Done():
				t.Stop()
				return
			}
		}

		if f.ErrorEvery > 0 && (seq+1)%f.ErrorEvery == 0 {
			writeError(w, http.StatusInternalServerError, "injected fault")
			return
		}
		next.ServeHTTP(w, r)
	})
}
