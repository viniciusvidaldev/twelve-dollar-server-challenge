# $12 Server Challenge — Rust

My entry for [The $12 Server Challenge](https://github.com/arjaythedev/twelve-dollar-server-challenge):
how many users can one $12/month server (1 vCPU, 2 GB RAM) handle with a SQLite-backed API?

- [`SPEC.md`](SPEC.md) — the API to implement
- [`schema.sql`](schema.sql) — the fixed SQLite schema
- [`seed/`](seed) — builds the seed database (`bash seed/make-seed.sh`)
- [`test/`](test) — the challenge's test suite
- [`bench/`](bench) — the k6 load test and Nginx config
