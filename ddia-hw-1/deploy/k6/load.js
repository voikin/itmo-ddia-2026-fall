import http from 'k6/http';
import { check } from 'k6';
import { Counter, Rate, Trend } from 'k6/metrics';

const rate = Number(__ENV.RATE || 50);
const vus = Number(__ENV.VUS || 100);
const seedItems = Number(__ENV.SEED_ITEMS || 20);
const ramp = __ENV.RAMP || '60s';
const hold = __ENV.HOLD || '30m';
if (!(Number.isInteger(rate) && rate > 0 && rate <= 500)) throw new Error('RATE must be an integer 1..500');
if (!(Number.isInteger(vus) && vus > 0 && vus <= 500)) throw new Error('VUS must be an integer 1..500');
if (!(Number.isInteger(seedItems) && seedItems > 0 && seedItems <= 1000)) throw new Error('SEED_ITEMS must be an integer 1..1000');
for (const [name, value] of [['RAMP', ramp], ['HOLD', hold]]) {
  if (!/^[1-9][0-9]{0,3}[smh]$/.test(value)) throw new Error(`${name} must look like 60s, 30m or 2h`);
}
const targets = (__ENV.TARGETS || 'http://app-dc1-1:8080').split(',').map((t) => t.trim()).filter(Boolean);
if (targets.length === 0 || targets.length > 20) throw new Error('TARGETS must list 1..20 endpoints');
for (const t of targets) {
  if (!/^http:\/\/(app-[a-z0-9-]+|127\.0\.0\.1|localhost)(:\d+)?$/.test(t)) {
    throw new Error('Use only private app endpoints; arbitrary public targets are forbidden');
  }
}

const started = new Counter('client_started');
const success = new Rate('client_success');
const good = new Rate('client_good');
const expected = new Rate('client_expected');
const injected = new Rate('client_injected_fault');
const wall = new Trend('client_wall_ms', true);

export const options = {
  scenarios: {
    background: {
      executor: 'ramping-arrival-rate',
      startRate: 1,
      timeUnit: '1s',
      stages: [
        { target: rate, duration: ramp },
        { target: rate, duration: hold },
      ],
      preAllocatedVUs: vus,
      maxVUs: vus,
      gracefulStop: '5s',
    },
  },
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
  thresholds: {
    dropped_iterations: ['count==0'],
    client_started: ['count>0'],
    client_expected: ['rate==1'],
  },
};

const json = { headers: { 'Content-Type': 'application/json' }, timeout: '3s', redirects: 0 };
const pick = (n) => 1 + Math.floor(Math.random() * n);

const scenarios = [
  { weight: 40, name: 'get_item', want: 200, run: (base, ids) =>
    http.get(`${base}/items/${ids[pick(ids.length) - 1]}`, { ...json, tags: { name: 'GET /items/{id}' } }) },
  { weight: 10, name: 'list_items', want: 200, run: (base) =>
    http.get(`${base}/items?limit=20`, { ...json, tags: { name: 'GET /items' } }) },
  { weight: 20, name: 'update_item', want: 200, run: (base, ids) =>
    http.put(`${base}/items/${ids[pick(ids.length) - 1]}`, JSON.stringify({ name: 'k6', value: `v${Date.now()}` }), { ...json, tags: { name: 'PUT /items/{id}' } }) },
  { weight: 10, name: 'create_item', want: 201, run: (base) =>
    http.post(`${base}/items`, JSON.stringify({ name: 'k6', value: 'new' }), { ...json, tags: { name: 'POST /items' } }) },
  { weight: 8, name: 'bad_json', want: 400, run: (base, ids) =>
    http.put(`${base}/items/${ids[0]}`, '{bad', { ...json, tags: { name: 'PUT /items/{id}' } }) },
  { weight: 5, name: 'missing_name', want: 422, run: (base) =>
    http.post(`${base}/items`, JSON.stringify({ value: 'x' }), { ...json, tags: { name: 'POST /items' } }) },
  { weight: 7, name: 'missing_item', want: 404, run: (base) =>
    http.get(`${base}/items/999999999`, { ...json, tags: { name: 'GET /items/{id}' } }) },
];
const totalWeight = scenarios.reduce((s, x) => s + x.weight, 0);

function choose() {
  let r = Math.random() * totalWeight;
  for (const s of scenarios) {
    r -= s.weight;
    if (r < 0) return s;
  }
  return scenarios[scenarios.length - 1];
}

export function setup() {
  const pods = [];
  for (const base of targets) {
    const ids = [];
    for (let attempt = 0; ids.length < seedItems && attempt < seedItems * 5; attempt++) {
      const r = http.post(`${base}/items`, JSON.stringify({ name: 'seed', value: String(attempt) }), { ...json, tags: { name: 'setup' } });
      if (r.status === 201) ids.push(r.json('id'));
    }
    if (ids.length === 0) throw new Error(`setup could not create any items on ${base}`);
    pods.push({ base, ids });
  }
  return { pods };
}

export default function (data) {
  const s = choose();
  const pod = data.pods[pick(data.pods.length) - 1];
  started.add(1, { scenario: s.name });
  const t = Date.now();
  const r = s.run(pod.base, pod.ids);
  const elapsed = Date.now() - t;

  let body = null;
  try { body = r.json(); } catch (_) {}
  const isInjected = r.status === 500 && body !== null && body.error === 'injected fault';
  const ok = r.status === s.want && body !== null;
  const is2xx = r.status >= 200 && r.status < 300;

  wall.add(elapsed, { scenario: s.name });
  injected.add(isInjected);
  expected.add(ok || isInjected, { scenario: s.name });
  success.add(is2xx && ok);
  good.add(is2xx && ok && elapsed <= 300);
  check(r, { [`${s.name}: HTTP ${s.want}`]: (x) => x.status === s.want });
}
