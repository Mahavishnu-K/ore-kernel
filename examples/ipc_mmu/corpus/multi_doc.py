"""Questions whose full answer spans several documents. Recall@k counts how many
of the relevant documents land in the top k, so finding one of two scores 0.5.
The linking documents tie facts together the way postmortem summaries do."""

from .model import Doc, Query

DOCS = [
    Doc("m01", "fact", "Postmortem review PMR-12 grouped INC-4471 and INC-4417 under certificate management and assigned the edge team to automate certificate renewal."),
    Doc("m02", "fact", "Postmortem review PMR-14 found that INC-5102 and INC-5120 both came from cache sizing, and asked the platform team to alert on cache eviction rates."),
    Doc("m03", "fact", "The release review for the search service compared DEP-0917 and DEP-0971 and made tokenizer changes require a multilingual test suite."),
    Doc("m04", "fact", "Mobile quality review MQR-7 tracked the Android 7.3.0 and iOS 7.3.1 crash spikes together and blocked the 7.4 release until both were fixed."),
    Doc("m05", "fact", "The feature flag audit FFA-3 listed FF_BULK_EXPORT and FF_BULK_IMPORT as high-risk flags that now need a second approver."),
    Doc("m06", "fact", "The storage capacity plan for the quarter covers pg-orders-07 disk growth and the pg-orders-70 replica rebuild."),
]

QUERIES = [
    Query("md-01", "What happened in INC-4471 and how should the certificate be rotated?", "multi_doc", ("f01", "k01")),
    Query("md-02", "How do we fix the disk pressure on pg-orders-07?", "multi_doc", ("f10", "k03")),
    Query("md-03", "What caused the Android crash in 7.3.0 and how do we investigate it?", "multi_doc", ("f09", "k06")),
    Query("md-04", "Which certificate incidents did the edge team review?", "multi_doc", ("m01", "f01", "n01")),
    Query("md-05", "Which incidents were caused by cache problems?", "multi_doc", ("m02", "f02", "n02")),
    Query("md-06", "Which search service deployments changed the tokenizer?", "multi_doc", ("m03", "f04", "n04")),
    Query("md-07", "Which mobile releases had crash spikes?", "multi_doc", ("m04", "f09", "n09")),
    Query("md-08", "Which feature flags are high risk and why?", "multi_doc", ("m05", "f03", "n03")),
    Query("md-09", "What storage work is planned for the orders shards?", "multi_doc", ("m06", "f10", "n10")),
    Query("md-10", "What are all the rules for making production changes?", "multi_doc", ("d02", "n16")),
    Query("md-11", "Which vendor contracts renew, and in which months?", "multi_doc", ("f08", "n08")),
    Query("md-12", "Which billing jobs reconcile against the ledger?", "multi_doc", ("f07", "n07")),
]
