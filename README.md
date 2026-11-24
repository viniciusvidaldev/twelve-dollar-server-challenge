# $12 Server Challenge — Rust + io_uring

My entry for [The $12 Server Challenge](https://github.com/arjaythedev/twelve-dollar-server-challenge):
how many users can one $12/month server (1 vCPU, 2 GB RAM) handle with a SQLite-backed API?

Submitted as [`rust-iouring-viniciusvidaldev`](https://github.com/arjaythedev/twelve-dollar-server-challenge/pull/548).

## Stack

- Rust, no framework and no async runtime
- io_uring through direct syscalls, no liburing or io_uring crate
- SQLite 3.54.1, vendored and compiled statically, called through hand-written FFI
- Serves HTTP/1.1 directly on port 80, no Nginx

## Features

- Single thread driving 5 io_uring rings; one ring blocks, the others wake it through eventfd
- Multishot accept into fixed file slots, 65,535 per ring
- Multishot recv with a provided buffer ring per ring (16,384 × 1 KiB)
- ENOBUFS handling: starved connections are queued and rearmed after each drain, capped by the buffers recycled
- Completion queue sized so it can't overflow
- Listen backlog of 4,096
- Hand-written HTTP/1.1 parser, keep-alive and pipelined requests
- SQLite built with `THREADSAFE=0`, `DEFAULT_MEMSTATUS=0`, `OMIT_SHARED_CACHE`, `OMIT_LOAD_EXTENSION`, among others
- WAL with `synchronous=NORMAL`
- Prepared statements reused for every request, never re-prepared
- Every token's HS256 signature verified on every request
- Workaround for Ubuntu 6.8 kernels that reject a zeroed buffer ring registration (Launchpad #2162843)

## Results

Measured on my machine: a Linux VM on an Apple M3 Pro, with the server pinned to **one core and capped at 2 GB**.
The load comes from [mload](https://github.com/arjaythedev/million-users-challenge/tree/main/bench/mload), the
challenge's user model, running on the VM's other cores over loopback. Each run ramps up for 60 s, then holds for 2 minutes.

| Users | req/s | p95 | p99 | Errors | Server CPU (1 core) | Server RAM |
|---:|---:|---:|---:|---:|---:|---:|
| 20,000 | 1,820 | 0.5 ms | 1.1 ms | 0% | 9% | 95 MB |
| 60,000 | 5,466 | 0.6 ms | 2.1 ms | 0% | 19% | 99 MB |
| 100,000 | 9,109 | 0.8 ms | 4.1 ms | 0% | 28% | 100 MB |
| 150,000 | 13,657 | 2.0 ms | 93 ms | 0% | 46% | 106 MB |
| 200,000 | 18,105 | 119 ms | 924 ms | 0% | 52% | 108 MB |

At 200,000 users the server used about half of its core. It could go higher, but the environment didn't let me
measure it: at 300,000 users the load generator needed more CPU and memory than the laptop could give it while
keeping the server's core free, so the client became the bottleneck before the server did.

A single Apple M3 core is several times faster than a DigitalOcean vCPU, so these numbers don't carry over to the
$12 droplet.

## Layout

```
src/server/   event loop: accept, recv, send, close per ring
src/uring/    io_uring setup, submission and completion queues, buffer ring
src/app/      routing, handlers, JWT auth, JSON responses
src/db/       SQLite connection, prepared statements, queries
src/sys/      raw syscall and FFI declarations
src/http.rs   HTTP/1.1 request parser
vendor/       SQLite amalgamation
```

## Running it

Linux only, kernel 6.6 or newer (io_uring with `IORING_SETUP_NO_SQARRAY`).

```bash
bash seed/make-seed.sh            # builds seed/feed.db and seed/tokens.json; needs node >= 18 and sqlite3
./build.sh

cp seed/feed.db /tmp/feed.db
JWT_SECRET=twelve-dollar-challenge SQLITE_PATH=/tmp/feed.db HOST=127.0.0.1 PORT=3000 ./start.sh
```

Run the challenge's test suite (builds, starts on a fresh database copy, runs 42 checks):

```bash
bash test/run.sh .                # needs curl, jq and openssl
```

[`SPEC.md`](SPEC.md) has the API, [`schema.sql`](schema.sql) the fixed schema.
