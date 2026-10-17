# HW1. Инструменты автоматизации в Grafana

CRUD-сервис на Go (chi, in-memory) с golden-signal метриками, VictoriaMetrics, Grafana,
генератором дашборда и фоновой нагрузкой k6.

## Запуск

```bash
cp .env.example .env
python3 scripts/configure.py
docker compose up --build -d
```

| Контейнер       | Адрес                                                                            |
| --------------- | -------------------------------------------------------------------------------- |
| Grafana         | http://127.0.0.1:3000 — логин `admin`, пароль: `GRAFANA_ADMIN_PASSWORD` в `.env` |
| Сервис (pod-1)  | http://127.0.0.1:8080                                                            |
| VictoriaMetrics | http://127.0.0.1:8428                                                            |

Дашборд: **Dashboards → Generated → items-service golden signals**.

Остановить и удалить данные: `docker compose down -v`.

## Что происходит при `docker compose up`

1. Поднимаются 5 подов сервиса: `dc-1` (pod-1..3) и `dc-2` (pod-4..5, тяжелее хвост и больше ошибок).
2. VictoriaMetrics каждые 5 с снимает `/metrics` со всех подов.
3. Grafana стартует с provisioned datasource на VictoriaMetrics.
4. `grafana-token` создаёт service account `dashgen` (Editor) и токен, кладёт его в общий том.
5. `dashgen` берёт список ручек из `GET /routes` и через Grafana API создаёт read-only дашборд.
6. k6 разгоняет нагрузку до 50 RPS за минуту и держит её 30 минут.

## Ручки

| Метод | Путь          | Ответы                                        |
| ----- | ------------- | --------------------------------------------- |
| POST  | `/items`      | 201, 400, 422                                 |
| GET   | `/items`      | 200, 400 (`?limit=1..1000`, по умолчанию 100) |
| GET   | `/items/{id}` | 200, 404                                      |
| PUT   | `/items/{id}` | 200, 400, 404, 422                            |

Тело `POST`/`PUT`: `{"name": "...", "value": "..."}`. Служебные: `/healthz`, `/metrics`, `/routes`.

## Метрики

Библиотека `pkg/metrics` — chi-middleware: `r.Use(m.Middleware)`.

| Метрика                                | Тип       | Метки                                |
| -------------------------------------- | --------- | ------------------------------------ |
| `http_server_requests_started_total`   | counter   | `method`, `route`                    |
| `http_server_requests_total`           | counter   | `method`, `route`, `status`          |
| `http_server_request_duration_seconds` | histogram | `method`, `route`, `status`          |
| `http_server_errors_total`             | counter   | `method`, `route`, `status`, `class` |
| `http_server_inflight_requests`        | gauge     | —                                    |

На всех метриках: `pod`, `datacenter`, `deployment` (из env) и `signal` (`latency` / `traffic` / `errors` / `saturation`).
Перцентили считаются в PromQL: `histogram_quantile(q, sum by (le) (rate(..._bucket[1m])))`.

## Нагрузка и сбои

Сбои в сервисе (env `FAULT_*` в `docker-compose.yaml`, только `/items`): задержка 25 мс + до 10 мс,
каждый N-й запрос +900 мс, каждый M-й — 500. Остальные ошибки (400/404/422) генерирует k6.

Параметры k6 переопределяются из shell:

```bash
LOAD_RATE=100 LOAD_HOLD=10m docker compose up -d k6
```

`LOAD_RATE`, `LOAD_RAMP`, `LOAD_HOLD`, `LOAD_VUS`, `LOAD_SEED_ITEMS`.
