"""Core operations-team memory: incident facts with exact identifiers, procedures
(skills), policies (directives), and general notes. Every topic is distinct, so
these documents test behaviour; near_miss.py adds the hard cases."""

from .model import Doc, Query

DOCS = [
    # ─── Facts: incidents, deployments, systems ───
    Doc("f01", "fact", "Incident INC-4471: the payments gateway in region eu-west returned error ERR_7731 for fourteen minutes after a TLS certificate expired on the load balancer."),
    Doc("f02", "fact", "Incident INC-5102: checkout latency rose to 3.2 seconds when the inventory cache in us-east was evicted during a rolling restart of the Redis cluster."),
    Doc("f03", "fact", "Incident INC-6230: a misconfigured feature flag named FF_BULK_EXPORT disabled CSV exports for enterprise tenants for two hours."),
    Doc("f04", "fact", "Deployment DEP-0917 shipped version 4.8.2 of the search service, which moved query parsing to a new tokenizer and cut p99 latency by eighteen percent."),
    Doc("f05", "fact", "Deployment DEP-1044 rolled back the recommendations model after click-through rate dropped by six percent in the experiment cohort B7."),
    Doc("f06", "fact", "The warehouse sensor SKU-99812-B reports humidity every thirty seconds and raises alert HUM_HIGH above seventy percent relative humidity."),
    Doc("f07", "fact", "The nightly billing reconciliation job runs at 02:00 UTC, compares ledger totals against the payment processor, and files discrepancies in queue BILL-RECON for review."),
    Doc("f08", "fact", "Vendor contract VND-3381 with the logistics provider renews every March and caps shipping surcharges at four percent per quarter."),
    Doc("f09", "fact", "The mobile app crash rate on Android 14 doubled after release 7.3.0 because a background worker accessed the camera without a foreground service."),
    Doc("f10", "fact", "Database shard pg-orders-07 reached ninety-one percent disk usage, so the storage team scheduled a vacuum and an online volume resize."),
    Doc("f11", "fact", "Customer churn in the education segment fell after the onboarding checklist was shortened from twelve steps to five steps."),
    Doc("f12", "fact", "The internal wiki search returned stale pages because the crawler skipped any page that was updated more than once per day."),
    # ─── Skills: procedures ───
    Doc("k01", "skill", "To rotate an expired TLS certificate, request a new certificate from the internal CA, upload it to the load balancer, then drain and reload each listener one at a time."),
    Doc("k02", "skill", "To roll back a deployment, pin the previous release tag in the deploy manifest, run the canary stage first, and promote only after error rates stay flat for ten minutes."),
    Doc("k03", "skill", "To free disk space on a Postgres shard, run VACUUM on the largest tables, archive partitions older than ninety days, and resize the volume if usage stays above eighty percent."),
    Doc("k04", "skill", "To debug a slow API endpoint, capture a flame graph under load, check the database query plan, and compare cache hit rates before and after the regression."),
    Doc("k05", "skill", "To onboard a new tenant, create the tenant record, provision a dedicated encryption key, and send the welcome email with the administrator invitation link."),
    Doc("k06", "skill", "To investigate an Android crash, symbolicate the stack trace with the release mapping file, reproduce it on the same OS version, and check foreground service requirements."),
    # ─── Directives: policies ───
    Doc("d01", "directive", "Never share customer personal data with third-party analytics tools unless a signed data processing agreement is on file."),
    Doc("d02", "directive", "All production changes require a reviewed change ticket and must not be deployed on Fridays after 15:00 local time."),
    Doc("d03", "directive", "Agents must cite the source pipe and chunk for every factual claim they include in an incident report."),
    Doc("d04", "directive", "Escalate any security incident involving leaked credentials to the on-call security engineer within fifteen minutes."),
    # ─── General ───
    Doc("g01", "general", "The engineering offsite will take place in Lisbon this autumn and include a hands-on workshop about observability tooling."),
    Doc("g02", "general", "The office coffee machine on the third floor is descaled every Monday morning before nine o'clock."),
]

QUERIES = [
    Query("core-x1", "ERR_7731", "exact", ("f01",)),
    Query("core-x2", "INC-6230", "exact", ("f03",)),
    Query("core-x3", "FF_BULK_EXPORT", "exact", ("f03",)),
    Query("core-x4", "DEP-1044", "exact", ("f05",)),
    Query("core-x5", "SKU-99812-B", "exact", ("f06",)),
    Query("core-x6", "BILL-RECON", "exact", ("f07",)),
    Query("core-x7", "VND-3381", "exact", ("f08",)),
    Query("core-x8", "pg-orders-07", "exact", ("f10",)),
    Query("core-s1", "Why did checkout get slow while the cache cluster restarted?", "semantic", ("f02",)),
    Query("core-s2", "Which release made the search service faster?", "semantic", ("f04",)),
    Query("core-s3", "How do I replace a certificate that has expired?", "semantic", ("k01",)),
    Query("core-s4", "What are the steps to undo a bad release?", "semantic", ("k02",)),
    Query("core-s5", "Are we allowed to ship changes at the end of the week?", "semantic", ("d02",)),
    Query("core-s6", "Who has to be told quickly when passwords leak?", "semantic", ("d04",)),
    Query("core-s7", "Why did the phone app start crashing?", "semantic", ("f09",)),
    Query("core-s8", "How can I make a sluggish endpoint faster?", "semantic", ("k04",)),
]
