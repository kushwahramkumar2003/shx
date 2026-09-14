# eval

T6 scoring harness (task T-506). This directory currently holds the intent
fixture seed used by the T5 live harness.

- Fixtures: [`fixtures/translate.json`](fixtures/translate.json) (20 cases)
- Live run: see [docs/08-TESTING.md](../../docs/08-TESTING.md) § T5

The fixtures are scored for *structure* by T5 (non-empty command + a printed
results table). Exact/regex rates, risk-misclassification, latency percentiles,
and cache-hit rate are T6's job.
