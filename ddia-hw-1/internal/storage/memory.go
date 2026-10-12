package storage

import (
	"errors"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"
)

var ErrNotFound = errors.New("item not found")

type Item struct {
	ID        string    `json:"id"`
	Name      string    `json:"name"`
	Value     string    `json:"value"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

type Memory struct {
	mu     sync.RWMutex
	nextID uint64
	items  map[string]Item
}

func NewMemory() *Memory {
	return &Memory{items: make(map[string]Item)}
}

func (m *Memory) Create(name, value string) Item {
	m.mu.Lock()
	defer m.mu.Unlock()

	m.nextID++
	now := time.Now().UTC()
	item := Item{
		ID:        strconv.FormatUint(m.nextID, 10),
		Name:      name,
		Value:     value,
		CreatedAt: now,
		UpdatedAt: now,
	}
	m.items[item.ID] = item
	return item
}

func (m *Memory) Get(id string) (Item, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	item, ok := m.items[id]
	if !ok {
		return Item{}, ErrNotFound
	}
	return item, nil
}

func (m *Memory) List() []Item {
	m.mu.RLock()
	items := make([]Item, 0, len(m.items))
	for _, item := range m.items {
		items = append(items, item)
	}
	m.mu.RUnlock()

	slices.SortFunc(items, func(a, b Item) int {
		if c := len(a.ID) - len(b.ID); c != 0 {
			return c
		}
		return strings.Compare(a.ID, b.ID)
	})
	return items
}

func (m *Memory) Update(id, name, value string) (Item, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	item, ok := m.items[id]
	if !ok {
		return Item{}, ErrNotFound
	}
	item.Name = name
	item.Value = value
	item.UpdatedAt = time.Now().UTC()
	m.items[id] = item
	return item, nil
}
