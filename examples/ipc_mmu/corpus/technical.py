"""Technical text: stack traces, versions, addresses, ports, hashes, config keys.

The BM25 tokenizer keeps `_` and `-` inside tokens but splits on `.`, `:`, `/`,
quotes, and brackets. So `idx_invoices_customer_created` and
`reports-api-7d9f8c6b5-x2kqp` stay whole, while `4.8.2`, `10.20.4.18:5432`, and
`src/parser/frame.rs:88` become several short tokens. These queries measure what
that does to exact lookups, the bread and butter of coding agents."""

from .model import Doc, Query

DOCS = [
    Doc("t01", "fact", "Python stack trace from the billing worker: Traceback (most recent call last): File \"/srv/billing/reconcile.py\", line 214, in match_ledger: KeyError: 'invoice_id'. It happens when the processor omits invoice_id on partial refunds."),
    Doc("t02", "fact", "Rust panic in ingest-svc: thread 'tokio-runtime-worker' panicked at src/parser/frame.rs:88:31 with 'called Option::unwrap() on a None value', triggered by zero-length frames from legacy sensors."),
    Doc("t03", "fact", "The Go service auth-gw crashed with panic: runtime error: invalid memory address or nil pointer dereference [signal SIGSEGV code=0x1 addr=0x18 pc=0x4a1f2c] when the token cache was empty."),
    Doc("t04", "fact", "Java OutOfMemoryError: Java heap space in pod reports-api-7d9f8c6b5-x2kqp after the export endpoint loaded 2.1 million rows into memory at once."),
    Doc("t05", "fact", "The Postgres primary db-orders-01 listens on 10.20.4.17:5432 and the read replica db-orders-02 listens on 10.20.4.18:5432 with replication lag under 200 ms."),
    Doc("t06", "fact", "The Redis cluster cache-sessions uses ports 7000-7005 on host 10.20.9.31 with maxmemory 12gb and the allkeys-lru eviction policy."),
    Doc("t07", "fact", "Kafka broker kafka-3 advertises PLAINTEXT://10.20.6.43:9092 and keeps its logs under /var/lib/kafka/data with retention.ms=604800000."),
    Doc("t08", "fact", "nginx returned 502 Bad Gateway when the upstream api_backend at 127.0.0.1:8081 closed keepalive connections; setting keepalive_timeout 65s and proxy_http_version 1.1 fixed it."),
    Doc("t09", "fact", "Security advisory SEC-ADV-0117 upgraded OpenSSL from 3.0.7 to 3.0.13 across the edge fleet and rotated every certificate issued before the upgrade."),
    Doc("t10", "fact", "Kubernetes node pool np-gpu-a100 runs kubelet v1.29.4, and pods requesting nvidia.com/gpu are scheduled only on nodes labelled accelerator=a100."),
    Doc("t11", "fact", "Migration 0042_add_invoice_index.sql creates index idx_invoices_customer_created on invoices(customer_id, created_at) CONCURRENTLY so writes are never locked."),
    Doc("t12", "fact", "Git commit 9f2c1ab4e7 reverted the connection pool change from commit 3e8d0f71c2 after p99 latency in the checkout service doubled."),
    Doc("t13", "fact", "Container image registry.internal/ledger-api:2.14.0-rc.3 failed the smoke test because /healthz returned 503 while migrations were still running."),
    Doc("t14", "fact", "The memory leak in session-cache 1.8.3 was traced to a buffer at 0x7ffe3a91c000 growing by 64 KiB per reconnect; release 1.8.4 frees the TLS buffer on disconnect."),
    Doc("t15", "fact", "ledger-api requires DATABASE_POOL_SIZE=48 and STATEMENT_TIMEOUT_MS=15000; when they are missing it falls back to 10 connections and a 30000 ms timeout."),
    Doc("t16", "fact", "TypeScript build error TS2345 in web-dashboard: Argument of type 'string | undefined' is not assignable to parameter of type 'string' in src/charts/latency.ts on line 57."),
    Doc("t17", "fact", "Prometheus alert HighErrorRate fires when the ratio of http_requests_total with status 5xx to all requests stays above 0.02 for ten minutes."),
    Doc("t18", "fact", "Terraform module vpc-core v3.2.1 creates subnets 10.40.0.0/20 and 10.40.16.0/20 in availability zones eu-west-1a and eu-west-1b."),
    Doc("t19", "fact", "Batch id 6f1c2e9a-3b7d-4c55-9e0f-2a8b7c1d4e93 identifies the reconciliation batch that stayed stuck in the PENDING state for nine hours."),
    Doc("t20", "fact", "The systemd unit ore-agent.service uses Restart=on-failure and RestartSec=5; journalctl -u ore-agent showed exit code 137 after the OOM killer stopped it."),
]

QUERIES = [
    Query("tq-01", "KeyError 'invoice_id'", "technical", ("t01",)),
    Query("tq-02", "src/parser/frame.rs:88", "technical", ("t02",), note="path and line split into tokens"),
    Query("tq-03", "nil pointer dereference in auth-gw", "technical", ("t03",)),
    Query("tq-04", "reports-api-7d9f8c6b5-x2kqp", "technical", ("t04",), note="pod name stays one token"),
    Query("tq-05", "10.20.4.18:5432", "technical", ("t05",), note="IP and port split into five tokens"),
    Query("tq-06", "PLAINTEXT://10.20.6.43:9092", "technical", ("t07",)),
    Query("tq-07", "502 Bad Gateway from nginx keepalive", "technical", ("t08",)),
    Query("tq-08", "SEC-ADV-0117", "technical", ("t09",)),
    Query("tq-09", "kubelet v1.29.4", "technical", ("t10",), note="version splits into v1, 29, 4"),
    Query("tq-10", "idx_invoices_customer_created", "technical", ("t11",)),
    Query("tq-11", "9f2c1ab4e7", "technical", ("t12",), note="short commit hash"),
    Query("tq-12", "ledger-api:2.14.0-rc.3", "technical", ("t13",)),
    Query("tq-13", "0x7ffe3a91c000", "technical", ("t14",), note="hex address"),
    Query("tq-14", "STATEMENT_TIMEOUT_MS", "technical", ("t15",)),
    Query("tq-15", "TS2345", "technical", ("t16",)),
    Query("tq-16", "6f1c2e9a-3b7d-4c55-9e0f-2a8b7c1d4e93", "technical", ("t19",), note="UUID stays one token"),
    Query("tq-17", "exit code 137 OOM killer", "technical", ("t20",)),
    Query("tq-18", "Why did the Java export endpoint run out of memory?", "technical", ("t04",), note="paraphrase over a stack trace"),
]
