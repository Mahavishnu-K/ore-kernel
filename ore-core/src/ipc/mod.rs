pub mod bm25;
pub mod chunking;
pub mod message_bus;
pub mod quant;
pub mod rate_limiter;
pub mod semantic;

pub use bm25::{Bm25Index, tokenize_bm25};
pub use chunking::{ChunkPayload, MemoryChunk, chunk_text};
pub use message_bus::{AgentMessage, DEFAULT_MESSAGE_BACKLOG, MessageBus};
pub use quant::{DiskVectorStore, PqChunk, PqCodebook, Sq8Chunk, Sq8Vector};
pub use rate_limiter::RateLimiter;
pub use semantic::{PipeState, ScoredChunk, SearchOutcome, SemanticBus, Sq8Candidate};

#[cfg(test)]
mod tests {
    use super::*;
    use ore_common::protocol::MemoryCategory;

    #[test]
    fn test_fast_dot_product_unrolled() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let b = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];

        let expected: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let actual = SemanticBus::dot_product(&a, &b);

        assert!((expected - actual).abs() < 1e-6);
    }

    #[test]
    fn test_bounded_message_bus_backpressure() {
        let bus = MessageBus::with_capacity(2);

        let msg = |i: usize| AgentMessage {
            from_app: "sender".to_string(),
            to_app: "receiver".to_string(),
            payload: format!("msg_{}", i),
            timestamp: 100,
        };

        assert!(bus.send_message(msg(1)).is_ok());
        assert!(bus.send_message(msg(2)).is_ok());

        let err = bus.send_message(msg(3));
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("queue is full"));
    }

    #[test]
    fn test_sentence_aware_chunking() {
        let text = "Hello world! This is a test of the sentence chunker. It preserves boundaries properly? Absolutely.";
        let chunks = chunk_text(text, "sentence_aware", 8, 2);

        assert!(!chunks.is_empty());
        assert!(
            chunks
                .iter()
                .any(|c| c.context_payload.contains("Hello world!"))
        );

        for c in &chunks {
            assert!(!c.probe_text.trim().is_empty());
            assert!(
                c.context_payload.trim_end().ends_with(['.', '!', '?']),
                "chunk ends mid-sentence: {:?}",
                c.context_payload
            );
        }

        assert!(chunks.iter().any(|c| {
            c.context_payload
                .contains("It preserves boundaries properly? Absolutely.")
        }));
    }

    #[test]
    fn test_bm25_exact_keyword_matching() {
        let mut bm25 = Bm25Index::new();
        bm25.add_chunk(1, "Kernel panic: SIGSEGV memory corruption at 0xDEADBEEF");
        bm25.add_chunk(2, "Normal user login success on port 8080");

        let scores = bm25.score_query("0xDEADBEEF");
        assert!(scores.get(&1).unwrap() > &0.0);
        assert!(!scores.contains_key(&2));
    }

    #[test]
    fn test_bm25_eviction_removes_postings() {
        let mut bm25 = Bm25Index::new();
        let chunk_text = "SIGSEGV memory corruption at 0xDEADBEEF";
        bm25.add_chunk(1, chunk_text);
        assert!(bm25.score_query("0xDEADBEEF").contains_key(&1));

        bm25.remove_chunk(1, chunk_text);
        assert!(!bm25.score_query("0xDEADBEEF").contains_key(&1));
        assert_eq!(bm25.doc_lengths.len(), 0);
        assert_eq!(bm25.total_tokens, 0);
    }

    #[test]
    fn test_cognitive_tier_filtering_in_search() {
        let bus = SemanticBus::new(24, 24);
        let vec = vec![1.0; 8];

        bus.write_chunk(
            "test_pipe",
            "This is a verified fact".to_string(),
            "This is a verified fact".to_string(),
            vec.clone(),
            "agent_a",
            false,
            MemoryCategory::Fact,
        );

        bus.write_chunk(
            "test_pipe",
            "This is a procedural rule".to_string(),
            "This is a procedural rule".to_string(),
            vec.clone(),
            "agent_a",
            false,
            MemoryCategory::Directive,
        );

        let results = bus
            .search_pipe(
                "test_pipe",
                &vec,
                "procedural rule",
                10,
                None,
                Some(MemoryCategory::Directive),
                false,
                true,
            )
            .results;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.category, MemoryCategory::Directive);
        assert!(results[0].1.text.contains("procedural rule"));
    }

    #[test]
    fn test_sentence_window_decoupled_chunking() {
        let text = "First sentence of the log. Second sentence with the critical error. Third sentence has recovery.";
        let chunks = chunk_text(text, "sentence_window", 100, 20);

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[1].probe_text,
            "Second sentence with the critical error."
        );
        assert!(chunks[1].context_payload.contains("First sentence"));
        assert!(chunks[1].context_payload.contains("Third sentence"));
    }

    #[test]
    fn test_cow_pipe_branching_and_isolation() {
        let bus = SemanticBus::new(24, 24);
        let vec = vec![1.0; 8];

        bus.write_chunk(
            "production_kb",
            "Production fact A".to_string(),
            "Production fact A context".to_string(),
            vec.clone(),
            "admin",
            false,
            MemoryCategory::Fact,
        );

        assert!(
            bus.branch_pipe("production_kb", "production_kb:branch/alpha")
                .is_ok()
        );

        bus.write_chunk(
            "production_kb:branch/alpha",
            "Experimental patch proposal".to_string(),
            "Experimental patch proposal context".to_string(),
            vec.clone(),
            "agent_alpha",
            false,
            MemoryCategory::Skill,
        );

        let base_results = bus
            .search_pipe(
                "production_kb",
                &vec,
                "Production",
                10,
                None,
                None,
                false,
                false,
            )
            .results;
        assert_eq!(base_results.len(), 1);
        assert_eq!(base_results[0].1.text.as_str(), "Production fact A");

        let branch_results = bus
            .search_pipe(
                "production_kb:branch/alpha",
                &vec,
                "Production",
                10,
                None,
                None,
                false,
                false,
            )
            .results;
        assert_eq!(branch_results.len(), 2);
    }

    #[test]
    fn test_cow_pipe_diff_and_merge() {
        let bus = SemanticBus::new(24, 24);
        let vec = vec![1.0; 8];

        bus.write_chunk(
            "base_doc",
            "Initial base doc".to_string(),
            "Initial base doc context".to_string(),
            vec.clone(),
            "admin",
            false,
            MemoryCategory::Fact,
        );

        assert!(bus.branch_pipe("base_doc", "base_doc:branch/task").is_ok());

        bus.write_chunk(
            "base_doc:branch/task",
            "Verified bugfix".to_string(),
            "Verified bugfix context".to_string(),
            vec.clone(),
            "worker",
            false,
            MemoryCategory::Skill,
        );

        let diff = bus.diff_pipe("base_doc", "base_doc:branch/task").unwrap();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].text.as_str(), "Verified bugfix");

        let merged_count = bus
            .merge_pipe("base_doc", "base_doc:branch/task", true)
            .unwrap();
        assert_eq!(merged_count, 1);

        let base_results = bus
            .search_pipe("base_doc", &vec, "bugfix", 10, None, None, false, false)
            .results;
        assert_eq!(base_results.len(), 2);

        assert!(bus.diff_pipe("base_doc", "base_doc:branch/task").is_err());
    }

    #[test]
    fn test_sq8_quantization_and_dot_product() {
        let v1 = vec![0.5, -0.5, 0.8, -0.2, 0.1, -0.9, 0.3, 0.4];
        let v2 = vec![0.2, 0.1, -0.5, 0.3, -0.4, 0.7, -0.2, 0.1];

        let sq8_1 = Sq8Vector::quantize(&v1);
        let sq8_2 = Sq8Vector::quantize(&v2);

        let expected_dot: f32 = v1.iter().zip(v2.iter()).map(|(&a, &b)| a * b).sum();
        let approx_dot = Sq8Vector::dot_product(&sq8_1.data, &sq8_2.data);

        assert!((expected_dot - approx_dot).abs() < 0.05);
    }
}
