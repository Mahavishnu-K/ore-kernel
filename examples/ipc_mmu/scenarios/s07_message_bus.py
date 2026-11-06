"""s07 - Message Bus: FIFO delivery, round-trip latency, bounded queues."""

import json
import time

from harness import scenario, summarize

from .common import COORDINATOR, WORKER_A, WORKER_B

BACKLOG = 1024  # ore-core DEFAULT_MESSAGE_BACKLOG


@scenario("s07_message_bus", "Message Bus: FIFO order, RTT, 1,024-message backpressure")
def run(ctx):
    c = ctx.client
    c.drain(WORKER_A)
    c.drain(WORKER_B)

    # FIFO: one sender, one receiver, strictly ordered.
    for i in range(50):
        reply = c.send(COORDINATOR, WORKER_A, {"run": ctx.run_id, "seq": i})
        ctx.expect(reply.startswith("SUCCESS"), f"send {i} failed: {reply}")
    got = [json.loads(m["payload"])["seq"] for m in c.drain(WORKER_A)]
    ctx.expect(got == list(range(50)), f"messages arrived out of order or were lost: {got[:10]}…")

    # Round trip: send, then the receiver polls it back.
    rtt = []
    for i in range(100):
        start = time.perf_counter()
        c.send(COORDINATOR, WORKER_A, {"run": ctx.run_id, "ping": i})
        msg = c.listen(WORKER_A)
        rtt.append((time.perf_counter() - start) * 1000)
        ctx.expect(msg is not None and json.loads(msg["payload"])["ping"] == i, f"ping {i} not delivered")
    stats = summarize(rtt)
    ctx.metric("rtt_p50_ms", stats["p50_ms"])
    ctx.metric("rtt_p99_ms", stats["p99_ms"])

    # Backpressure: the queue holds BACKLOG messages, then the kernel refuses.
    accepted = 0
    refusal = ""
    for i in range(BACKLOG + 5):
        reply = c.send(COORDINATOR, WORKER_B, {"run": ctx.run_id, "fill": i})
        if reply.startswith("SUCCESS"):
            accepted += 1
        else:
            refusal = reply
            break
    drained = len(c.drain(WORKER_B))
    ctx.expect(accepted == BACKLOG, f"queue accepted {accepted} messages, expected {BACKLOG}")
    ctx.expect("queue is full" in refusal, f"overflow should be refused with backpressure, got: {refusal}")
    ctx.expect(drained == BACKLOG, f"drained {drained} messages, expected {BACKLOG}")
    reply = c.send(COORDINATOR, WORKER_B, {"run": ctx.run_id, "after": "drain"})
    ctx.expect(reply.startswith("SUCCESS"), f"queue should accept again after draining: {reply}")
    c.drain(WORKER_B)
