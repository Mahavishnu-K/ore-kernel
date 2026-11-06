"""Questions the corpus cannot answer. A good CRAG gate falls back for these and
not for answerable ones; scoring both sets gives the threshold calibration.

Two flavours:
  off_domain   - nothing like it is stored; dense scores should be low
  near_domain  - plausible for this team and sharing tokens with stored text
                 (INC-, ERR_, pg-orders-, vpc-), so BM25 finds partial matches.
                 These are the hard negatives for any confidence gate.
"""

from .model import Query

_OFF_DOMAIN = [
    "What is the capital of Mongolia?",
    "How do I keep a sourdough starter alive?",
    "Who won the 1998 football world cup?",
    "How many moons does Jupiter have?",
    "What are the best hiking trails in Patagonia?",
    "How do I tune a classical guitar?",
    "What is the boiling point of water at high altitude?",
]

_NEAR_DOMAIN = [
    "What caused incident INC-9999?",
    "What does error ERR_4040 mean?",
    "Who owns the pg-orders-12 shard?",
    "Which team maintains the Terraform module vpc-edge?",
    "Which deployment shipped version 9.0.0 of the billing service?",
    "What is the on-call rotation for the data platform team?",
    "Where is the postmortem for the March DNS outage?",
    "How do I rotate the TLS certificates on Kafka brokers?",
    "What is the SLA for the analytics API?",
    "Which vendor provides our CDN?",
    "When is the security training deadline?",
    "What is the retention period for audit logs?",
    "How do I request more GPU quota?",
    "Why did the Windows desktop client crash?",
    "What is the policy for working from abroad?",
    "Which region hosts the disaster recovery site?",
    "How do we migrate from MySQL 5.7 to 8.0?",
    "How much does the logistics provider charge per pallet?",
]

QUERIES = [Query(f"un-off-{i + 1:02d}", t, "unanswerable", note="off_domain") for i, t in enumerate(_OFF_DOMAIN)] + [
    Query(f"un-near-{i + 1:02d}", t, "unanswerable", note="near_domain") for i, t in enumerate(_NEAR_DOMAIN)
]
