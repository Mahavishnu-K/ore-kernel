"""Near misses: each document is a deliberate look-alike of a core document.
Transposed identifiers (INC-4471 vs INC-4417), the same symptom in another
region, the same procedure for another system. Retrieval has to rank the right
twin first, so these measure precision, not just recall."""

from .model import Doc, Query

DOCS = [
    Doc("n01", "fact", "Incident INC-4417: the payments gateway in region us-east returned error ERR_7713 for six minutes because an intermediate certificate was missing from the bundle."),
    Doc("n02", "fact", "Incident INC-5120: checkout latency rose to 1.4 seconds in eu-west when the session cache hit its memory limit and began evicting keys."),
    Doc("n03", "fact", "Incident INC-6203: the feature flag FF_BULK_IMPORT was enabled too early and let malformed CSV imports corrupt three tenant catalogs."),
    Doc("n04", "fact", "Deployment DEP-0971 shipped version 4.2.8 of the search service and was rolled back within an hour because the new tokenizer dropped accented characters."),
    Doc("n05", "fact", "Deployment DEP-1404 rolled the recommendations model forward again after the B7 cohort recovered its click-through rate."),
    Doc("n06", "fact", "The warehouse sensor SKU-99821-B reports temperature every sixty seconds and raises alert TEMP_HIGH above thirty degrees Celsius."),
    Doc("n07", "fact", "The weekly billing audit job runs on Sundays at 04:00 UTC, reconciles refunds against the ledger, and files mismatches in queue BILL-AUDIT."),
    Doc("n08", "fact", "Vendor contract VND-3318 with the cloud provider renews every September and includes a committed spend discount of twelve percent."),
    Doc("n09", "fact", "The mobile app crash rate on iOS 18 rose after release 7.3.1 because a widget extension exceeded its memory budget."),
    Doc("n10", "fact", "Database shard pg-orders-70 lost its replica during a failed failover test, so writes ran without redundancy for forty minutes."),
    Doc("n11", "fact", "Customer churn in the healthcare segment rose after the pricing page removed the monthly plan."),
    Doc("n12", "fact", "The internal wiki search ranked outdated pages first because the freshness boost was disabled during the index migration."),
    Doc("n13", "skill", "To rotate an SSH host key, generate a new key pair on the server, publish its fingerprint to the inventory, and remove the old key from every known_hosts file."),
    Doc("n14", "skill", "To roll back a database schema migration, restore the pre-migration snapshot to a staging replica, verify row counts, and promote it during a maintenance window."),
    Doc("n15", "skill", "To relieve memory pressure on a Redis node, expire stale keys, lower maxmemory-samples, and move large sorted sets to a dedicated instance."),
    Doc("n16", "directive", "Production database changes require two reviewers and must not run during the end-of-month billing window."),
    Doc("n17", "directive", "Report a lost or stolen company laptop to the IT service desk within one business day."),
    Doc("n18", "directive", "Never paste customer personal data into public chat channels, even when the message is meant for a colleague."),
    Doc("n19", "general", "The engineering offsite in Porto moved to the spring and will include a workshop about incident response."),
    Doc("n20", "general", "The office espresso machine on the fifth floor is serviced on the last Friday of each month."),
]

QUERIES = [
    Query("nm-01", "INC-4417", "near_miss", ("n01",), note="transposed digits of INC-4471"),
    Query("nm-02", "ERR_7713", "near_miss", ("n01",), note="transposed digits of ERR_7731"),
    Query("nm-03", "Which payments incident was caused by a missing intermediate certificate?", "near_miss", ("n01",)),
    Query("nm-04", "Checkout slowed down because the session cache ran out of memory", "near_miss", ("n02",)),
    Query("nm-05", "FF_BULK_IMPORT", "near_miss", ("n03",), note="vs FF_BULK_EXPORT"),
    Query("nm-06", "Which feature flag corrupted tenant catalogs?", "near_miss", ("n03",)),
    Query("nm-07", "DEP-0971", "near_miss", ("n04",), note="vs DEP-0917"),
    Query("nm-08", "Which search release dropped accented characters and was rolled back?", "near_miss", ("n04",)),
    Query("nm-09", "Which deployment rolled the recommendations model forward again?", "near_miss", ("n05",)),
    Query("nm-10", "SKU-99821-B", "near_miss", ("n06",), note="vs SKU-99812-B"),
    Query("nm-11", "When does the refund reconciliation run?", "near_miss", ("n07",)),
    Query("nm-12", "Which vendor contract includes a committed spend discount?", "near_miss", ("n08",)),
    Query("nm-13", "Why did the iPhone app crash more after 7.3.1?", "near_miss", ("n09",)),
    Query("nm-14", "pg-orders-70", "near_miss", ("n10",), note="vs pg-orders-07"),
    Query("nm-15", "Why did healthcare customers leave?", "near_miss", ("n11",)),
    Query("nm-16", "How do I rotate SSH host keys?", "near_miss", ("n13",), note="vs TLS certificate rotation"),
    Query("nm-17", "How do I undo a failed schema migration?", "near_miss", ("n14",), note="vs deployment rollback"),
    Query("nm-18", "How do I reduce memory pressure on a Redis node?", "near_miss", ("n15",)),
    Query("nm-19", "What are the rules for changing the production database?", "near_miss", ("n16",), note="vs production deploy rules"),
    Query("nm-20", "What should I do if I lose my work laptop?", "near_miss", ("n17",)),
]
