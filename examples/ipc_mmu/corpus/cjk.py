"""Non-Latin and mixed-script text.

Two known limits show up here:
  - BM25: Chinese and Japanese have no spaces, and every CJK character counts as
    alphanumeric, so a whole clause becomes one token. A query only matches if it
    repeats that clause exactly. Korean and Cyrillic use spaces and tokenize fine,
    apart from inflection.
  - Chunking: the chunkers count words by whitespace and split sentences on
    `.`, `!`, or `?` followed by a space. A Chinese paragraph is one "word" and one
    "sentence" (see CJK_SENTENCES).
The embedder ([system].embedder in ore.toml) decides dense recall here. With a
mostly English-trained model, dense recall on these queries largely comes from
shared ASCII tokens such as INC-7788."""

from .model import Doc, Query

DOCS = [
    Doc("c01", "fact", "事故 INC-7788：华东区域的支付网关在证书过期后返回错误，持续了二十分钟。运维团队更换了证书并重新加载了负载均衡器。", "zh"),
    Doc("c02", "fact", "数据库分片 pg-users-03 的磁盘使用率达到百分之九十五，存储团队计划在周末扩容。", "zh"),
    Doc("c03", "fact", "インシデント INC-8812：東京リージョンの検索サービスで、新しいトークナイザーが全角文字を落としたため、検索結果が空になりました。", "ja"),
    Doc("c04", "directive", "当番エンジニアは、認証情報の漏洩を十五分以内にセキュリティチームへ報告しなければなりません。", "ja"),
    Doc("c05", "fact", "장애 INC-9031: 서울 리전의 결제 서비스가 레디스 캐시 재시작 중에 지연 시간이 3초까지 증가했습니다.", "ko"),
    Doc("c06", "fact", "배포 DEP-2207은 추천 모델을 롤백했습니다. 클릭률이 4퍼센트 감소했기 때문입니다.", "ko"),
    Doc("c07", "skill", "Runbook 更新：使用 kubectl rollout undo deployment/ledger-api 回滚 ledger-api，然后检查 /healthz 是否返回 200。", "zh-en"),
    Doc("c08", "fact", "Le service de facturation à Montréal a échoué après la mise à jour 2.3.1 ; l'équipe a restauré la version précédente en vingt minutes.", "fr"),
    Doc("c09", "fact", "Сбой INC-9150: сервис уведомлений в регионе Франкфурт перестал отправлять письма после смены SMTP-пароля.", "ru"),
]

QUERIES = [
    Query("cjk-01", "INC-7788", "cjk", ("c01",), note="ASCII id inside Chinese text"),
    Query("cjk-02", "支付网关证书过期", "cjk", ("c01",), note="Chinese clause; not a whole BM25 token in the doc"),
    Query("cjk-03", "磁盘使用率", "cjk", ("c02",)),
    Query("cjk-04", "検索サービス トークナイザー", "cjk", ("c03",)),
    Query("cjk-05", "레디스 캐시 재시작 지연", "cjk", ("c05",), note="Korean uses spaces, so BM25 can match words"),
    Query("cjk-06", "payment gateway certificate expired in east China", "cjk", ("c01",), note="cross-lingual"),
    Query("cjk-07", "kubectl rollout undo ledger-api", "cjk", ("c07",), note="ASCII command inside Chinese"),
    Query("cjk-08", "facturation Montréal mise à jour 2.3.1", "cjk", ("c08",)),
    Query("cjk-09", "SMTP пароль уведомления", "cjk", ("c09",), note="Russian inflection: пароль vs пароля"),
]

# Six Chinese sentences ending in "。" with no spaces. A sentence-aware chunker
# should produce six chunks; today's chunkers see one sentence.
CJK_SENTENCES = [
    "华东区域的支付网关在凌晨两点开始返回错误。",
    "值班工程师在五分钟内确认了告警。",
    "根本原因是负载均衡器上的证书已经过期。",
    "团队更换了证书并逐个重新加载了监听器。",
    "服务在二十分钟后完全恢复。",
    "后续行动是自动化证书续期并增加过期告警。",
]
CJK_PARAGRAPH = "".join(CJK_SENTENCES)
