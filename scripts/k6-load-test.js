import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter, Trend } from 'k6/metrics';

const baseUrl = (__ENV.BASE_URL || 'http://127.0.0.1:3000').replace(/\/+$/, '');
const userId = Number.parseInt(__ENV.USER_ID || '1', 10);
const apiEndpoint = `${baseUrl}/api/users`;
const userEndpoint = `${baseUrl}/api/users/${userId}`;
const indexEndpoint = `${baseUrl}/`;
const userPageEndpoint = `${baseUrl}/users/${userId}`;
const healthEndpoint = `${baseUrl}/healthz`;
const readinessEndpoint = `${baseUrl}/readyz`;

export const options = {
  scenarios: {
    health_checks: {
      executor: 'constant-vus',
      vus: Number.parseInt(__ENV.HEALTH_VUS || '1', 10),
      duration: __ENV.HEALTH_DURATION || '30s',
      exec: 'healthChecks',
    },
    api_reads: {
      executor: 'constant-vus',
      vus: Number.parseInt(__ENV.API_VUS || '10', 10),
      duration: __ENV.API_DURATION || '1m',
      exec: 'apiReads',
    },
    ssr_reads: {
      executor: 'constant-vus',
      vus: Number.parseInt(__ENV.SSR_VUS || '5', 10),
      duration: __ENV.SSR_DURATION || '1m',
      exec: 'ssrReads',
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<500'],
    'checks{scenario:health_checks}': ['rate>0.99'],
    'checks{scenario:api_reads}': ['rate>0.99'],
    'checks{scenario:ssr_reads}': ['rate>0.99'],
  },
};

export const apiRequests = new Counter('api_requests');
export const ssrRequests = new Counter('ssr_requests');
export const requestLatency = new Trend('request_latency', true);

function recordLatency(response) {
  requestLatency.add(response.timings.duration);
}

export function healthChecks() {
  const health = http.get(healthEndpoint);
  const ready = http.get(readinessEndpoint);

  recordLatency(health);
  recordLatency(ready);

  check(health, {
    'healthz returns 204': (res) => res.status === 204,
  });

  check(ready, {
    'readyz returns 204': (res) => res.status === 204,
  });
}

export function apiReads() {
  const list = http.get(apiEndpoint);
  const detail = http.get(userEndpoint);

  apiRequests.add(2);
  recordLatency(list);
  recordLatency(detail);

  check(list, {
    'GET /api/users returns 200': (res) => res.status === 200,
    'GET /api/users returns JSON array': (res) => Array.isArray(res.json()),
  });

  check(detail, {
    'GET /api/users/:id returns 200': (res) => res.status === 200,
    'GET /api/users/:id returns matching user': (res) => {
      const body = res.json();
      return body && body.id === userId && typeof body.name === 'string';
    },
  });
}

export function ssrReads() {
  const index = http.get(indexEndpoint);
  const userPage = http.get(userPageEndpoint);

  ssrRequests.add(2);
  recordLatency(index);
  recordLatency(userPage);

  check(index, {
    'GET / returns 200': (res) => res.status === 200,
    'GET / returns HTML': (res) =>
      ((res.headers['Content-Type'] || res.headers['content-type']) || '').includes('text/html'),
    'GET / renders seeded users': (res) =>
      res.body.includes('Alice') && res.body.includes('Bob'),
  });

  check(userPage, {
    'GET /users/:id returns 200': (res) => res.status === 200,
    'GET /users/:id returns HTML': (res) =>
      ((res.headers['Content-Type'] || res.headers['content-type']) || '').includes('text/html'),
    'GET /users/:id renders the user': (res) =>
      res.body.includes('Alice') || res.body.includes('Bob'),
  });
}

export function setup() {
  const response = http.get(apiEndpoint);

  check(response, {
    'setup can reach the API': (res) => res.status === 200,
  });

  return {
    baseUrl,
    userId,
  };
}

export default function () {
  sleep(1);
}
