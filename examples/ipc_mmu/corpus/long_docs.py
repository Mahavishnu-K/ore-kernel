"""Long documents: postmortems, runbooks, a design document, a capacity review,
and an on-call guide. Ten documents of 360-630 words each (about 4,700 words,
105 paragraphs), written the way teams write them: titled section paragraphs, timelines, action items.

They are stored chunked, not whole. A query is answered when a returned chunk's
text or context_payload contains its answer phrase, so the same queries compare
chunking strategies: a sliding window can cut the answer in half, while
sentence_window matches one sentence and returns its neighbours.

Some documents overlap on purpose (L01 and L08 are both credential incidents;
L02 and L09 are both cluster-maintenance runbooks), so long-document retrieval
has near misses too."""

from .model import LongDoc, Query

DOCS = [
    LongDoc(
        "L01",
        "Postmortem: payments gateway TLS outage (INC-4471)",
        "fact",
        """Summary. On the morning of the incident the payments gateway in eu-west rejected card authorizations for fourteen minutes. Customers saw a generic payment failure page and the error ERR_7731 appeared in gateway logs. No payments were charged twice and no data was lost.

Timeline. At 06:12 UTC the certificate on the public load balancer expired. At 06:13 UTC the synthetic checkout probe failed and paged the on-call engineer. At 06:17 UTC the engineer confirmed that the TLS handshake was failing for every client. At 06:21 UTC a new certificate was requested from the internal CA. At 06:26 UTC the listeners were reloaded one at a time and traffic recovered. At 06:40 UTC the incident commander declared the incident resolved after error rates stayed at baseline for ten minutes.

Detection and response. The on-call engineer first suspected a deployment, because a change to the fraud scoring service had shipped twenty minutes earlier. Rolling back that change cost four minutes and had no effect. The engineer then ran a handshake test from a laptop, saw the certificate expiry date in the output, and switched to the certificate rotation runbook. The incident commander joined at 06:18 UTC and took over communication so the engineer could focus on the fix.

Root cause. The certificate had been issued manually two years earlier and was never added to the renewal automation. The expiry warning email went to a distribution list that had been archived when the previous team was reorganized. Nobody received the thirty-day, seven-day, or one-day warnings.

Why the automation missed it. The renewal automation only manages certificates that it issued itself. It discovers them by reading its own database, not by scanning the load balancers. A certificate that was uploaded by hand is invisible to it, and there was no inventory that compared what the automation knew with what was actually deployed.

Impact. Roughly eleven thousand authorization attempts failed during the window. The retry queue absorbed about forty percent of them once the gateway recovered. Merchants on the enterprise plan received a credit according to the service agreement. Two large merchants opened support tickets asking whether customers had been charged, and support answered both within the hour.

Customer communication. The status page was updated six minutes after the first page, which is within our ten-minute target. The first message said that payments were failing in Europe and that a fix was in progress. A follow-up message after recovery explained that no customer was charged twice. Support received a short internal summary they could quote directly.

What went well. The synthetic probe detected the failure within one minute. The runbook for certificate rotation was accurate and the rotation itself took five minutes. The incident commander kept the response calm and the status page current.

What went poorly. Four minutes were spent rolling back an unrelated deployment because the first hypothesis was not checked against evidence. The expiry warnings had been silently failing for months. Nobody owned an inventory of public certificates.

Action items. The edge team will move every public certificate into the automated renewal pipeline by the end of the quarter. Expiry alerts will page the owning team directly instead of sending email. A weekly report will list every certificate that expires within forty-five days, with its owner. The on-call training will add a step to check certificate expiry whenever every TLS handshake fails at once.

Lessons learned. Manual exceptions to automation are where outages hide. Alerts that go to email are only as good as the mailing list behind them. A cheap handshake check would have shortened this incident by several minutes.""",
    ),
    LongDoc(
        "L02",
        "Runbook: replacing a failed Kafka broker",
        "skill",
        """Purpose. Use this runbook when a Kafka broker in the events cluster has failed hardware or a corrupted log directory and must be replaced. The cluster tolerates one broker loss without data loss because every topic uses a replication factor of three.

Scope. The events cluster has six brokers spread across three availability zones, two brokers in each zone. This runbook does not cover the analytics cluster, which uses a different storage layout and a replication factor of two. Do not use it for planned upgrades either, because rolling upgrades restart brokers in place and never replace hosts.

Before you start. Confirm that the broker is really gone by checking that its process is not running and that the controller lists it as offline. Check that no partition is under its minimum in-sync replica count, because replacing a broker while another one is degraded can make partitions unavailable. Announce the work in the incident channel.

Capacity check. Make sure the remaining brokers have at least thirty percent free disk before you begin. While the replacement catches up, the other brokers serve all reads and writes for the lost partitions, and their disk usage grows. If free disk is below thirty percent, ask the storage team to extend volumes first, because a full disk on a second broker turns a routine replacement into an outage.

Provision the replacement. Create the new host from the broker image and give it the same broker id as the failed host. Reusing the broker id lets the new broker take over the old partition assignments without a reassignment plan. Mount the data volume at the usual path and make sure it is empty.

Configuration details. The broker id is set in the server properties file and must match the failed host exactly. The rack setting must match the availability zone of the new host, otherwise replica placement ignores zone boundaries. Leave the number of network and I/O threads at the image defaults unless the platform team asks otherwise.

Start and watch replication. Start the Kafka service and watch the under-replicated partitions metric. The new broker fetches every partition it owns from the current leaders, which can take several hours for large topics. Throttle replication to one hundred megabytes per second during business hours so producers are not starved. Outside business hours the throttle can be raised to three hundred megabytes per second.

Rebalance leadership. When the under-replicated partition count reaches zero, run a preferred leader election. Without it the replacement broker stays a follower for every partition and the rest of the cluster carries extra load.

Verify. Produce and consume a test message on the canary topic. Compare broker request latency with the other brokers for thirty minutes. Close the incident only after consumer lag on the payments topics is back to normal.

Known issues. The first start of a replacement broker can log thousands of warnings about missing log segments. These are expected and stop once replication begins. If the broker exits immediately with a cluster id mismatch, the data volume was not empty, and you must wipe it before starting again.

Escalation. If replication has not started within fifteen minutes, page the streaming platform team. If more than one broker is offline at the same time, stop this runbook and declare a severity one incident, because the cluster can no longer guarantee durability.

Rollback. If the replacement broker keeps crashing, stop it, remove it from the rack, and open a ticket with the infrastructure team. The cluster keeps serving traffic on the remaining brokers.

Post-replacement checklist. Remove the replication throttle. Update the inventory with the new host name and serial number. Attach the controller logs from the failure to the hardware ticket so the vendor can analyze the fault.""",
    ),
    LongDoc(
        "L03",
        "Postmortem: search tokenizer regression (DEP-0971)",
        "fact",
        """Summary. Deployment DEP-0971 shipped version 4.2.8 of the search service with a new tokenizer. Searches containing accented characters returned no results for fifty-two minutes until the deployment was rolled back.

Timeline. At 13:05 UTC the canary stage started with internal traffic. At 13:20 UTC the canary was promoted automatically to all regions. At 13:41 UTC a support agent escalated reports of empty search results. At 13:58 UTC the search team confirmed that only queries with accented letters were affected. At 14:12 UTC the rollback began and at 14:17 UTC search results returned to normal.

Detection. The regression was not caught by automated tests because the test suite only contained English queries. A customer support agent noticed that searches for the word café returned nothing and escalated to the search team. The search dashboard showed a sharp increase in zero-result queries from French and Spanish speaking regions.

Customer impact. About eight percent of all searches during the window returned no results. In France, Spain, and Brazil the share was close to forty percent. Conversion from search fell by a third in those countries during the incident. No data was lost and the index did not need repair.

Root cause. The new tokenizer normalized text to ASCII before indexing, but the query path still sent the original Unicode string. Every query that contained an accented letter therefore searched for a token that no longer existed in the index. The change looked safe in code review because both functions were named normalize.

Why code review missed it. The pull request changed the indexing path in one file and left the query path untouched in another. Reviewers saw a small, well-tested change. Nobody asked whether the query path applied the same normalization, because the naming suggested that it already did.

Resolution. The on-call engineer rolled back to version 4.8.2 using the standard deployment rollback procedure. Indexing did not need to be rebuilt because the old tokenizer was still compatible with the index format.

Contributing factors. The canary stage only received internal traffic, which is almost entirely English. The zero-result rate was not part of the canary health checks, so the canary was promoted automatically.

What went well. Support escalated quickly and with a precise example. The rollback procedure worked on the first attempt. The search team found the root cause within an hour of the rollback by comparing the two normalization functions side by side.

Action items. The search team will add a multilingual query set covering French, Spanish, German, Japanese, and Korean to the test suite. The zero-result rate will become a canary health check with a threshold of two percentage points above baseline. Tokenizer changes will require review from a second search engineer. The indexing and query paths will share a single normalization function so they cannot drift apart again.

Open questions. The team has not yet decided whether the canary should receive a sample of real external traffic. Doing so would catch regressions like this one earlier, but it would also expose a small share of customers to every canary.""",
    ),
    LongDoc(
        "L04",
        "Runbook: Postgres primary failover",
        "skill",
        """When to fail over. Fail over the orders database only when the primary is unreachable for more than two minutes or its disk is failing. A slow primary is not a reason to fail over, because failover drops every open connection and the application needs about a minute to reconnect.

Roles. One engineer runs the commands and one engineer reads this runbook aloud and records the time of each step. Do not run a failover alone, even at night, because a skipped step is the most common cause of a failed failover. The database owner must be informed before the promotion step.

Pre-flight checklist. Confirm that the most recent base backup finished successfully within the last twenty-four hours. Confirm that write ahead log archiving is current. Confirm that the connection pooler is healthy and that you can edit its configuration. Write down the current primary and replica host names in the incident ticket.

Check the replica. Before promoting, confirm that the replica db-orders-02 is streaming and that its replication lag is below one second. If the lag is larger, the transactions that were not yet replicated will be lost, and you must get approval from the database owner first.

Fence the old primary. Stop the Postgres service on the old primary and remove its address from the connection pooler. Fencing prevents a split brain in which both servers accept writes after the old primary comes back. If you cannot reach the old primary at all, ask the infrastructure team to power it off from the management console.

Promote. Run the promote command on the replica and wait until it reports that it is accepting writes. Update the pooler so the primary endpoint points at the new server. Most services reconnect automatically within sixty seconds.

Communication. Post in the incident channel before fencing, after promotion, and after services have reconnected. Tell the payments team directly, because their workers hold long transactions and may need a manual restart.

Verifying data integrity. Compare the order count for the last hour on the new primary with the count reported by the order service metrics. A difference larger than the replication lag you measured before promotion means writes were lost and must be investigated before the incident is closed.

After failover. Rebuild a new replica from the latest base backup as soon as possible, because the cluster now runs without redundancy. Check that scheduled jobs such as the nightly billing reconciliation point at the new primary. Record the exact promotion time in the incident ticket so data analysts can explain any gap in metrics.

Common mistakes. Do not promote a replica that is still applying a large backlog of write ahead log, because the promotion will hang until replay finishes. Do not restart the old primary before it has been rebuilt as a replica.

Practice drills. The database team runs a failover drill on the staging cluster every month and on production twice a year during a maintenance window. Every on-call engineer should take part in at least one drill before their first shift.""",
    ),
    LongDoc(
        "L05",
        "Incident review: session-cache memory leak",
        "fact",
        """Summary. Over three weeks the session-cache service grew from four gigabytes to twelve gigabytes of resident memory and was restarted by the out of memory killer twice. Each restart logged out about thirty thousand users.

Timeline. In the first week, memory grew slowly and nobody noticed. In the second week, the first out of memory restart happened at 03:10 UTC on a Tuesday. In the third week, a second restart happened during the evening peak and users complained on social media. The investigation started the next morning and the fix shipped four days later.

Customer impact. Users who were logged out had to sign in again and lost the contents of their shopping basket. Support received about nine hundred contacts across both restarts. Revenue loss was estimated as small but not zero.

Investigation. Heap profiles showed that memory grew by sixty-four kilobytes every time a client reconnected. The growth stopped when reconnections were disabled on a test node. Engineers traced the allocation to the TLS session buffer, which was allocated on connect but never released on disconnect.

Why it took three weeks. The memory alert was configured on the container limit of sixteen gigabytes, so it never fired before the out of memory killer acted. The restarts were classified as infrastructure noise because they happened at night.

Detection gaps. There was no dashboard panel for resident memory over a multi-week window, only for the last twenty-four hours. A slow leak looks flat on a one-day graph. Restart counts were collected but no alert used them.

Fix. Release 1.8.4 frees the TLS buffer when a connection closes and adds a counter for open buffers. After the release, resident memory stayed flat at four gigabytes for ten days.

Testing. The bug was not caught in testing because the load tests opened connections once and kept them open. Real mobile clients reconnect every time the network changes, which happens dozens of times a day on a phone. A new load test now drops and reopens ten percent of connections every minute.

Follow-up. Memory alerts will fire at seventy percent of the container limit. Every out of memory restart will open a ticket automatically. The reconnect storm test will run in the nightly performance suite.

Related work. Sessions will move to a replicated store so a restart of one node no longer logs users out. The design review for that change is scheduled for next month.""",
    ),
    LongDoc(
        "L06",
        "Guide: your first on-call week",
        "directive",
        """Before your shift. Make sure you can log in to the paging tool, the incident channel, the dashboards, and the production bastion. Read the last two weeks of incident reports so you know which systems have been unstable.

Tools. The paging tool sends alerts to your phone and expects an acknowledgement. The incident channel is where all coordination happens, including during the night. Dashboards for every service are linked from the service catalog, and every alert links to the runbook that explains it.

Responding to a page. Acknowledge every page within five minutes, even if you cannot fix the problem yet. If you do not acknowledge in time, the page escalates to the secondary on-call engineer and then to the engineering manager.

Severity levels. A severity one incident means customers cannot pay, log in, or use the core product. Declare severity one early, because it is cheaper to downgrade an incident than to gather people late. Severity two means a degraded experience for some customers, and severity three means an internal problem with no customer impact.

Escalation paths. If a problem is outside your team's systems, page the owning team through the service catalog rather than messaging individuals. For security problems, page the security on-call engineer immediately, even if you are unsure. Never wait for business hours to escalate a severity one incident.

Communication. Post an update in the incident channel at least every thirty minutes during a severity one incident. Name a single incident commander, who coordinates people and does not debug. Customer support needs a plain-language summary they can share with customers.

Using runbooks. Follow the runbook step by step and write down what you did. If a step is wrong or missing, finish the incident first and fix the runbook afterwards. A runbook that nobody corrects becomes dangerous.

Handing over. At the end of your shift, write a handover note listing open incidents, silenced alerts, and anything you are watching. Never leave an alert silenced without an expiry time.

Postmortems. Every severity one and severity two incident gets a written postmortem within five working days. Postmortems are blameless: they describe what the system allowed to happen, not who made a mistake.

Taking care of yourself. If you were paged during the night, start late the next day. Ask for help early, because nobody expects a new on-call engineer to know every system.

Compensation. On-call shifts are paid as a fixed weekly allowance, plus time off in lieu for every night with more than one page. Record night pages in the time tracking tool by the end of the week.""",
    ),
    LongDoc(
        "L07",
        "Design: memory retention for agents",
        "directive",
        """Problem. Agents write facts, skills, and directives into shared memory every day, and nothing is ever removed. Old facts compete with new ones in search, stale procedures get retrieved after they were replaced, and storage grows without bound. This document defines how long each kind of memory is kept and how it is removed.

Goals. Keep memory small enough that search stays fast and relevant. Make sure a superseded fact is never returned ahead of the fact that replaced it. Meet legal requirements for deleting personal data on request. Keep enough history that incidents can be investigated months later.

Non-goals. This design does not cover conversation history inside a single agent, which is already compacted automatically. It does not change how memory is searched, only what is kept.

Retention tiers. Facts are kept for ninety days after their last retrieval. Skills are kept until they are explicitly replaced, because a procedure that is rarely used can still be critical. Directives are kept forever and can only be removed by a reviewer. General notes are kept for thirty days.

Superseding facts. When an agent writes a fact that contradicts an existing one, it must reference the old fact by id. The old fact is then marked as superseded and excluded from search, but kept in the archive for audits. Facts without a reference are treated as new, even if they look similar.

Deletion requests. A request to delete personal data must be completed within thirty days. Deletion removes the chunk from every pipe and branch, removes its vector, and removes it from the persisted pipe files on disk. A deletion is recorded in the audit log with the request id but without the deleted content.

Legal hold. A pipe under legal hold keeps everything regardless of retention tiers until the hold is lifted. Only the legal team can place or lift a hold, and every hold is reviewed every six months.

Archive. Expired and superseded chunks move to a compressed archive that is not searchable by agents. Investigators can restore an archived pipe into a separate read-only pipe for a limited time.

Rollout. Retention will first run in report-only mode for one month, listing what would have been removed. After the report is reviewed, removal will be enabled for general notes, then facts, then skills.

Risks. Removing a fact that is still needed would make an agent forget something important. Report-only mode and the archive are the safeguards. A second risk is that agents do not reference the facts they replace, which would leave contradictions in memory; the reviewer will reject merges that add contradicting facts without a reference.

Open questions. The team has not decided whether retrieval by a human counts as a retrieval for the ninety-day rule. It is also unclear whether skills learned during an incident should expire if the incident was a one-off.""",
    ),
    LongDoc(
        "L08",
        "Postmortem: leaked deployment credential (SEC-2291)",
        "fact",
        """Summary. A deployment credential for the staging environment was committed to a public repository by mistake. An automated scanner operated by a third party found it within eleven minutes. The credential was revoked twenty-six minutes after the commit, and the investigation found no evidence that it was used by anyone else.

Timeline. At 10:02 UTC an engineer pushed a commit that included a local configuration file. At 10:13 UTC the code hosting provider sent an automated secret scanning alert to the security mailbox. At 10:19 UTC the security on-call engineer acknowledged the alert. At 10:28 UTC the credential was revoked and replaced. At 10:45 UTC the commit was removed from the repository history.

What the credential allowed. The credential could deploy new versions of three staging services and read their configuration. It could not access production, customer data, or the build signing keys. Staging configuration contained connection details for staging databases, which hold only synthetic data.

Investigation. The security team reviewed the access logs for the credential for the thirty days before the leak and the period after it. Every use came from the build system's address range. No deployment happened outside the normal pipeline, and no configuration was read from an unknown address.

Root cause. The repository had been made public for an open source release the week before. The local configuration file was listed in the ignore file of the old private repository but not in the new public one. The engineer did not notice that the file was included, because it was created automatically by a development tool.

Why detection was fast. The code hosting provider scans every public commit for known credential formats. Our deployment credentials use a recognizable prefix, which is why the scanner identified the leak within minutes rather than hours.

Why revocation took longer than it should. The alert went to a mailbox, and the security on-call engineer saw it six minutes later by chance. Revocation itself required finding the owner of the credential, because the credential inventory listed a team that no longer existed.

Action items. Secret scanning alerts will page the security on-call engineer instead of going to a mailbox. Every credential will have a named team owner that is checked every quarter. Repositories that become public will go through a checklist that includes comparing ignore files. Development tools will write local configuration outside the repository directory.

Lessons learned. A prefix on every credential is cheap and makes leaks detectable. Ownership records rot when teams change. Alerts that matter must page a person, not wait in a mailbox.""",
    ),
    LongDoc(
        "L09",
        "Runbook: resharding the Redis session cluster",
        "skill",
        """Purpose. Use this runbook to add shards to the Redis session cluster when memory usage stays above seventy percent for a week, or to remove shards after a sustained drop in traffic. Resharding moves hash slots between nodes while the cluster keeps serving traffic.

Before you start. Check that every node is healthy and that no slot is currently being migrated. Check that the cluster has a recent backup. Schedule resharding outside the evening peak, because slot migration adds latency to keys that are being moved.

Adding a node. Start the new node with the same configuration as the existing nodes and join it to the cluster as an empty master. Then add a replica for it in a different availability zone before moving any slots, so the new data is protected from the start.

Moving slots. Move slots in batches of one hundred rather than all at once. After each batch, check the client error rate and the latency of the session service. If either rises above its normal range, pause and wait for the cluster to settle before the next batch.

Client behaviour. Clients follow redirection responses automatically while a slot is moving, so sessions are not lost. Older clients that do not understand redirection will log errors for keys in moving slots. The mobile backend still uses one such client, so tell its team before you start.

Removing a node. Move all of the node's slots to other masters first, then remove its replica, then remove the node itself. Never remove a master that still owns slots, because every key in those slots becomes unreachable.

Verification. Run the cluster check command and confirm that all sixteen thousand three hundred eighty-four slots are assigned and that no node reports an open slot. Compare memory usage across masters; after a good reshard no master holds more than fifteen percent above the average.

Rollback. Slot moves can be reversed by moving the same slots back. If a node fails during resharding, stop moving slots, let failover promote its replica, and finish the migration on the promoted node.

Known issues. Very large keys, such as sorted sets with millions of members, can make a slot move block the node for several seconds. Find large keys before resharding and move them to a dedicated instance first.""",
    ),
    LongDoc(
        "L10",
        "Capacity review: third quarter",
        "fact",
        """Summary. Traffic grew by twenty-two percent over the quarter, mostly from the new markets in Latin America. Compute and storage kept up, but two systems are close to their limits: the orders database and the events cluster. This review lists what was used, what will be needed next quarter, and what we will buy.

Compute. Average CPU utilization across the application fleet was forty-eight percent, with peaks of seventy-one percent during sales events. Autoscaling handled every peak without manual help. The application fleet does not need new capacity next quarter.

Orders database. Storage on the orders shards grew by eighteen percent. At the current rate, the largest shard reaches its disk limit in about five months. The storage team will add two shards next quarter and rebalance customers across them, which is cheaper than moving every shard to larger disks.

Events cluster. Ingress on the events cluster doubled because product analytics now records every search interaction. Broker disks are at sixty-four percent. The streaming platform team will add two brokers and shorten retention for the analytics topics from seven days to three days.

Session cache. After the memory leak fix, session cache memory is stable at four gigabytes per node. No change is needed until the move to a replicated store.

GPU inference. Inference for internal agents now runs on four GPU nodes with an average utilization of thirty-five percent. Most of the load arrives during working hours in Europe. Instead of buying more GPUs, the team will schedule batch summarization jobs at night to use idle capacity.

Costs. Cloud spend rose by fifteen percent, less than traffic, because of the committed spend discount and better autoscaling. Storage is now the fastest-growing cost line. The finance team asked for a forecast that separates storage growth from compute growth.

Risks. A sales event larger than last year's could push the orders database past safe write latency before the new shards are ready. If that happens, the fallback is to pause non-essential batch jobs on the orders database during the event.

Decisions. Add two orders shards and two event brokers. Shorten analytics retention. Do not buy new GPU capacity this quarter. Review again in three months.""",
    ),
]

QUERIES = [
    # ─── L01 ───
    Query("lq-01", "Why did nobody receive the certificate expiry warnings?", "long", answers=("went to a distribution list that had been archived",)),
    Query("lq-02", "How many authorization attempts failed during the TLS outage?", "long", answers=("roughly eleven thousand authorization attempts failed",)),
    Query("lq-13", "Why didn't the certificate renewal automation know about the expired certificate?", "long", answers=("only manages certificates that it issued itself",)),
    Query("lq-14", "What wrong first hypothesis slowed the TLS outage response?", "long", answers=("rolling back that change cost four minutes",)),
    # ─── L02 ───
    Query("lq-03", "Why should a replacement Kafka broker reuse the old broker id?", "long", answers=("reusing the broker id lets the new broker take over",)),
    Query("lq-04", "How fast should Kafka replication be throttled during business hours?", "long", answers=("throttle replication to one hundred megabytes per second",)),
    Query("lq-15", "How much free disk do the other brokers need before a Kafka broker replacement?", "long", answers=("at least thirty percent free disk",)),
    Query("lq-16", "What does a cluster id mismatch mean when the new broker starts?", "long", answers=("the data volume was not empty",)),
    # ─── L03 ───
    Query("lq-05", "Why did searches with accented characters return nothing?", "long", answers=("normalized text to ascii before indexing",)),
    Query("lq-06", "Why was the bad search canary promoted automatically?", "long", answers=("zero-result rate was not part of the canary health checks",)),
    Query("lq-17", "How badly did the tokenizer bug hit searches in France and Spain?", "long", answers=("the share was close to forty percent",)),
    Query("lq-18", "How will the search team stop indexing and query normalization from drifting apart?", "long", answers=("share a single normalization function",)),
    # ─── L04 ───
    Query("lq-07", "What replication lag is acceptable before promoting the Postgres replica?", "long", answers=("replication lag is below one second",)),
    Query("lq-08", "Why must the old Postgres primary be fenced?", "long", answers=("fencing prevents a split brain",)),
    Query("lq-19", "Why should a database failover never be run by one person alone?", "long", answers=("a skipped step is the most common cause of a failed failover",)),
    Query("lq-20", "How often does the database team practise failover?", "long", answers=("every month and on production twice a year",)),
    # ─── L05 ───
    Query("lq-09", "How much memory did the session cache leak per reconnect?", "long", answers=("grew by sixty-four kilobytes every time a client reconnected",)),
    Query("lq-10", "Why did the memory alert never fire before the OOM killer?", "long", answers=("configured on the container limit of sixteen gigabytes",)),
    Query("lq-21", "Why didn't load tests catch the session cache leak?", "long", answers=("opened connections once and kept them open",)),
    # ─── L06 ───
    Query("lq-11", "How quickly must an on-call engineer acknowledge a page?", "long", answers=("acknowledge every page within five minutes",)),
    Query("lq-12", "How often should updates be posted during a severity one incident?", "long", answers=("at least every thirty minutes during a severity one incident",)),
    Query("lq-22", "How are on-call engineers compensated for night pages?", "long", answers=("time off in lieu for every night with more than one page",)),
    # ─── L07 ───
    Query("lq-23", "How long are facts kept in agent memory?", "long", answers=("facts are kept for ninety days after their last retrieval",)),
    Query("lq-24", "What happens to a fact when an agent writes a contradicting one?", "long", answers=("marked as superseded and excluded from search",)),
    Query("lq-25", "Who can place a legal hold on a memory pipe?", "long", answers=("only the legal team can place or lift a hold",)),
    # ─── L08 ───
    Query("lq-26", "How did the deployment credential end up in a public repository?", "long", answers=("listed in the ignore file of the old private repository",)),
    Query("lq-27", "What could the leaked staging credential access?", "long", answers=("could deploy new versions of three staging services",)),
    Query("lq-28", "Why was the leaked credential detected so quickly?", "long", answers=("deployment credentials use a recognizable prefix",)),
    # ─── L09 ───
    Query("lq-29", "In what batch size should Redis hash slots be moved?", "long", answers=("move slots in batches of one hundred",)),
    Query("lq-30", "Why must a Redis master not be removed while it owns slots?", "long", answers=("every key in those slots becomes unreachable",)),
    # ─── L10 ───
    Query("lq-31", "When will the largest orders shard run out of disk?", "long", answers=("reaches its disk limit in about five months",)),
    Query("lq-32", "How will idle GPU capacity be used instead of buying more GPUs?", "long", answers=("schedule batch summarization jobs at night",)),
]
