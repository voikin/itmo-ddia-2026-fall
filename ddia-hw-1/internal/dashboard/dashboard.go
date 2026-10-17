package dashboard

import (
	"fmt"
	"strings"

	"github.com/grafana/grafana-foundation-sdk/go/cog"
	"github.com/grafana/grafana-foundation-sdk/go/common"
	"github.com/grafana/grafana-foundation-sdk/go/dashboard"
	"github.com/grafana/grafana-foundation-sdk/go/prometheus"
	"github.com/grafana/grafana-foundation-sdk/go/timeseries"
	"github.com/grafana/grafana-foundation-sdk/go/units"

	"github.com/voikin/itmo-ddia-2026-fall/ddia-hw-1/pkg/metrics"
)

const (
	window        = "[1m]"
	goodThreshold = "0.3"
	successStatus = `status=~"2.."`
	instanceSel   = `deployment="$deployment",datacenter=~"$datacenter",pod=~"$pod"`
)

var percentiles = []struct{ q, name string }{
	{"0.5", "p50"}, {"0.75", "p75"}, {"0.9", "p90"}, {"0.95", "p95"}, {"0.99", "p99"},
}

type Config struct {
	UID           string
	Title         string
	DatasourceUID string
	Deployment    string
}

func Build(cfg Config, routes []metrics.Route) (dashboard.Dashboard, error) {
	ds := common.DataSourceRef{Type: cog.ToPtr("prometheus"), Uid: cog.ToPtr(cfg.DatasourceUID)}

	b := dashboard.NewDashboardBuilder(cfg.Title).
		Uid(cfg.UID).
		Tags([]string{"generated", "golden-signals", cfg.Deployment}).
		Readonly().
		Refresh("5s").
		Time("now-15m", "now").
		Timezone(common.TimeZoneBrowser).
		Tooltip(dashboard.DashboardCursorSyncCrosshair).
		WithVariable(queryVar(ds, "deployment", "Deployment",
			fmt.Sprintf("label_values(%s, deployment)", metrics.MetricFinished), false).
			Current(dashboard.VariableOption{
				Text:  dashboard.StringOrArrayOfString{String: cog.ToPtr(cfg.Deployment)},
				Value: dashboard.StringOrArrayOfString{String: cog.ToPtr(cfg.Deployment)},
			})).
		WithVariable(queryVar(ds, "datacenter", "Datacenter",
			fmt.Sprintf(`label_values(%s{deployment="$deployment"}, datacenter)`, metrics.MetricFinished), true)).
		WithVariable(queryVar(ds, "pod", "Pod",
			fmt.Sprintf(`label_values(%s{deployment="$deployment",datacenter=~"$datacenter"}, pod)`, metrics.MetricFinished), true))

	b.WithRow(overviewRow(ds))
	for i, r := range routes {
		b.WithRow(routeRow(ds, i+1, r))
	}
	return b.Build()
}

func queryVar(ds common.DataSourceRef, name, label, query string, multi bool) *dashboard.QueryVariableBuilder {
	v := dashboard.NewQueryVariableBuilder(name).
		Label(label).
		Datasource(ds).
		Query(dashboard.StringOrMap{String: cog.ToPtr(query)}).
		Definition(query).
		Refresh(dashboard.VariableRefreshOnTimeRangeChanged).
		Sort(dashboard.VariableSortAlphabeticalAsc)
	if multi {
		v = v.Multi(true).IncludeAll(true).AllValue(".*")
	}
	return v
}

func overviewRow(ds common.DataSourceRef) *dashboard.RowBuilder {
	byRoute := "method, route"
	return dashboard.NewRowBuilder("Обзор: все ручки").
		WithPanel(panel(ds, "00.1 | RPS по ручкам", units.RequestsPerSecond,
			"Завершившиеся запросы в секунду: сначала rate отдельных counters, затем sum.",
			target(fmt.Sprintf(`sum by (%s) (rate(%s{%s}%s))`, byRoute, metrics.MetricFinished, instanceSel, window), "{{method}} {{route}}"))).
		WithPanel(panel(ds, "00.2 | Доля ошибок по ручкам", units.PercentUnit,
			"4xx/5xx / все завершившиеся запросы.",
			target(fmt.Sprintf(`sum by (%[1]s) (rate(%[2]s{%[4]s}%[5]s)) / sum by (%[1]s) (rate(%[3]s{%[4]s}%[5]s))`,
				byRoute, metrics.MetricErrors, metrics.MetricFinished, instanceSel, window), "{{method}} {{route}}")).Min(0)).
		WithPanel(panel(ds, "00.3 | p99 успешных ответов по ручкам", "s",
			"histogram_quantile по объединённым распределениям: rate(bucket) → sum by (le) → квантиль.",
			target(quantile("0.99", byRoute, successStatus), "{{method}} {{route}}"))).
		WithPanel(panel(ds, "00.4 | Запросы в обработке", units.Short,
			"In-flight gauge: вошли в обработчик и ещё не завершились.",
			target(fmt.Sprintf(`sum by (datacenter, pod) (%s{%s})`, metrics.MetricInflight, instanceSel), "{{datacenter}} / {{pod}}")))
}

func routeRow(ds common.DataSourceRef, n int, r metrics.Route) *dashboard.RowBuilder {
	sel := fmt.Sprintf(`%s,method="%s",route="%s"`, instanceSel, r.Method, escape(r.Route))
	id := fmt.Sprintf("%02d", n)

	flow := panel(ds, id+".1 | Поток: started / finished / 2xx / good", units.RequestsPerSecond,
		"good = 2xx и ≤ 300 мс (бакет le=\"0.3\"). finished включает ответы с ошибкой.",
		target(fmt.Sprintf(`sum(rate(%s{%s}%s))`, metrics.MetricStarted, sel, window), "started"),
		target(fmt.Sprintf(`sum(rate(%s{%s}%s))`, metrics.MetricFinished, sel, window), "finished"),
		target(fmt.Sprintf(`sum(rate(%s{%s,%s}%s))`, metrics.MetricFinished, sel, successStatus, window), "2xx"),
		target(fmt.Sprintf(`sum(rate(%s_bucket{%s,%s,le="%s"}%s))`, metrics.MetricDuration, sel, successStatus, goodThreshold, window), "good"),
	)

	latencyTargets := []*prometheus.DataqueryBuilder{
		target(fmt.Sprintf(`sum(rate(%[1]s_sum{%[2]s,%[3]s}%[4]s)) / sum(rate(%[1]s_count{%[2]s,%[3]s}%[4]s))`,
			metrics.MetricDuration, sel, successStatus, window), "mean"),
	}
	for _, p := range percentiles {
		latencyTargets = append(latencyTargets, target(quantileSel(p.q, "", sel+","+successStatus), p.name))
	}
	latency := panel(ds, id+".2 | Латентность успешных ответов: mean, p50, p75, p90, p95, p99", "s",
		"Только 2xx: быстрые ошибки не должны «улучшать» задержку.", latencyTargets...)

	errorsByStatus := panel(ds, id+".3 | Ошибки по статусам", units.RequestsPerSecond,
		"Ошибочные ответы в секунду по HTTP-статусу (499 — клиент ушёл до ответа).",
		target(fmt.Sprintf(`sum by (status) (rate(%s{%s}%s))`, metrics.MetricErrors, sel, window), "{{status}}"),
	).Stacking(common.NewStackingConfigBuilder().Mode(common.StackingModeNormal)).FillOpacity(30)

	errorRatio := panel(ds, id+".4 | Доля ошибок 4xx / 5xx", units.PercentUnit,
		"Ошибки класса / все завершившиеся запросы ручки.",
		target(fmt.Sprintf(`sum by (class) (rate(%[1]s{%[3]s}%[4]s)) / scalar(sum(rate(%[2]s{%[3]s}%[4]s)))`,
			metrics.MetricErrors, metrics.MetricFinished, sel, window), "{{class}}"),
	).Min(0)

	return dashboard.NewRowBuilder(fmt.Sprintf("%s | %s %s", id, r.Method, r.Route)).
		WithPanel(flow).
		WithPanel(latency).
		WithPanel(errorsByStatus).
		WithPanel(errorRatio)
}

func panel(ds common.DataSourceRef, title, unit, description string, targets ...*prometheus.DataqueryBuilder) *timeseries.PanelBuilder {
	p := timeseries.NewPanelBuilder().
		Title(title).
		Description(description).
		Datasource(ds).
		Unit(unit).
		Span(12).
		Height(8).
		Legend(common.NewVizLegendOptionsBuilder().
			DisplayMode(common.LegendDisplayModeList).
			Placement(common.LegendPlacementBottom).
			ShowLegend(true))
	for i, t := range targets {
		p = p.WithTarget(t.RefId(string(rune('A' + i))))
	}
	return p
}

func target(expr, legend string) *prometheus.DataqueryBuilder {
	return prometheus.NewDataqueryBuilder().Expr(expr).LegendFormat(legend).Range()
}

func quantile(q, by, extra string) string {
	return quantileSel(q, by, instanceSel+","+extra)
}

func quantileSel(q, by, sel string) string {
	if by != "" {
		by += ", "
	}
	return fmt.Sprintf(`histogram_quantile(%s, sum by (%sle) (rate(%s_bucket{%s}%s)))`, q, by, metrics.MetricDuration, sel, window)
}

func escape(s string) string {
	return strings.NewReplacer(`\`, `\\`, `"`, `\"`).Replace(s)
}
