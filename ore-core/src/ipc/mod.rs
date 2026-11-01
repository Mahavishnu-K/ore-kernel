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
pub use semantic::{
    HybridFusion, PipeState, ScoredChunk, SearchOutcome, SemanticBus, Sq8Candidate,
};

#[cfg(test)]
mod tests {
    use super::*;
    use ore_common::protocol::MemoryCategory;
    use std::sync::Arc;

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

    #[test]
    fn test_cow_pipe_merge_without_delete_drains_deltas() {
        let bus = SemanticBus::new(24, 24);
        let vec = vec![1.0; 8];

        bus.write_chunk(
            "base_doc_v2",
            "Initial base doc".to_string(),
            "Initial base doc context".to_string(),
            vec.clone(),
            "admin",
            false,
            MemoryCategory::Fact,
        );

        assert!(
            bus.branch_pipe("base_doc_v2", "base_doc_v2:branch/task")
                .is_ok()
        );

        bus.write_chunk(
            "base_doc_v2:branch/task",
            "Delta chunk 1".to_string(),
            "Delta context 1".to_string(),
            vec.clone(),
            "worker",
            false,
            MemoryCategory::Skill,
        );

        // Merge with delete_branch = false
        let merged_count = bus
            .merge_pipe("base_doc_v2", "base_doc_v2:branch/task", false)
            .unwrap();
        assert_eq!(merged_count, 1);

        // Verify base now has both chunks
        let base_contents = bus.get_pipe_contents("base_doc_v2").unwrap();
        assert_eq!(base_contents.len(), 2);

        // Verify the branch DOES NOT duplicate chunks (it sees exactly the base chunks, delta is drained)
        let branch_contents = bus.get_pipe_contents("base_doc_v2:branch/task").unwrap();
        assert_eq!(branch_contents.len(), 2);

        // Verify diff is now empty
        let diff = bus
            .diff_pipe("base_doc_v2", "base_doc_v2:branch/task")
            .unwrap();
        assert_eq!(diff.len(), 0);

        // A second merge operation must merge 0 items (no duplicate appends)
        let second_merge_count = bus
            .merge_pipe("base_doc_v2", "base_doc_v2:branch/task", false)
            .unwrap();
        assert_eq!(second_merge_count, 0);

        let final_base_contents = bus.get_pipe_contents("base_doc_v2").unwrap();
        assert_eq!(final_base_contents.len(), 2);
    }

    #[test]
    fn test_gc_preserves_idle_fresh_branches() {
        let bus = SemanticBus::new(24, 24);
        let vec = vec![1.0; 8];

        bus.write_chunk(
            "base_gc_pipe",
            "Base document".to_string(),
            "Context".to_string(),
            vec,
            "admin",
            false,
            MemoryCategory::Fact,
        );

        // Create an empty branch
        assert!(
            bus.branch_pipe("base_gc_pipe", "base_gc_pipe:branch/idle")
                .is_ok()
        );

        // Run GC immediately
        bus.run_garbage_collection();

        // Branch must NOT be pruned by GC
        let diff = bus.diff_pipe("base_gc_pipe", "base_gc_pipe:branch/idle");
        assert!(
            diff.is_ok(),
            "Fresh empty branch must survive GC within its TTL"
        );
    }

    #[test]
    fn test_bm25_coupled_eviction_under_capacity_cap() {
        let bus = SemanticBus::new(24, 24);
        let vec = vec![1.0; 8];

        // Write initial chunk with a unique signature keyword
        bus.write_chunk(
            "test_cap_pipe",
            "UniqueMarkerAlpha keyword inside chunk 1".to_string(),
            "Context 1".to_string(),
            vec.clone(),
            "agent",
            false,
            MemoryCategory::Fact,
        );

        // Verify BM25 index contains the document
        let outcome1 = bus.search_pipe(
            "test_cap_pipe",
            &vec,
            "UniqueMarkerAlpha",
            5,
            None,
            None,
            false,
            true,
        );
        assert!(!outcome1.results.is_empty());
        assert!(outcome1.results[0].1.text.contains("UniqueMarkerAlpha"));

        // Simulate reaching capacity and popping the first chunk
        if let Some(mut pipe_guard) = bus.memory_pipes.get_mut("test_cap_pipe") {
            if let PipeState::Active { chunks, bm25, .. } = pipe_guard.value_mut() {
                if let Some(evicted) = chunks.pop_front() {
                    bm25.remove_chunk(evicted.id, &evicted.text);
                }
            }
        }

        // Querying for the evicted term must yield 0 results in BM25
        let outcome2 = bus.search_pipe(
            "test_cap_pipe",
            &vec,
            "UniqueMarkerAlpha",
            5,
            None,
            None,
            false,
            true,
        );
        assert!(
            outcome2.results.is_empty(),
            "Evicted document must not appear in BM25 results"
        );
    }

    #[test]
    fn test_hybrid_rrf_normalization_scale() {
        let chunk1 = Arc::new(MemoryChunk {
            id: 1,
            text: Arc::new("Doc 1".to_string()),
            context_payload: Arc::new("Context 1".to_string()),
            vector: Arc::new(vec![1.0; 8]),
            source_app: "app".to_string(),
            timestamp: 1000,
            category: MemoryCategory::Fact,
        });

        let chunk2 = Arc::new(MemoryChunk {
            id: 2,
            text: Arc::new("Doc 2".to_string()),
            context_payload: Arc::new("Context 2".to_string()),
            vector: Arc::new(vec![0.5; 8]),
            source_app: "app".to_string(),
            timestamp: 1000,
            category: MemoryCategory::Fact,
        });

        // Case 1: Ranked #1 in both Dense and Sparse
        let dense_ranked = vec![(0.95, Arc::clone(&chunk1)), (0.70, Arc::clone(&chunk2))];
        let sparse_ranked = vec![(12.5, Arc::clone(&chunk1)), (4.2, Arc::clone(&chunk2))];

        let (results, top_score) =
            HybridFusion::compute_rrf_and_normalize(dense_ranked, sparse_ranked, 60.0, 5);

        assert_eq!(results.len(), 2);
        // Perfect dual #1 rank must normalize exactly to 1.0
        assert!((top_score - 1.0).abs() < 1e-5);
        assert!((results[0].0 - 1.0).abs() < 1e-5);
        assert!(results[1].0 < 1.0 && results[1].0 > 0.0);

        // Case 2: Present only in Dense (no lexical hits in sparse)
        let single_dense = vec![(0.95, Arc::clone(&chunk1))];
        let (single_results, single_top) =
            HybridFusion::compute_rrf_and_normalize(single_dense, vec![], 60.0, 5);

        // Single list rank 1 must also calibrate cleanly to 1.0 rather than being penalized to 0.5
        assert_eq!(single_results.len(), 1);
        assert!((single_top - 1.0).abs() < 1e-5);
    }

    #[tokio::test]
    async fn test_raptor_multi_level_recursion_and_centroids() {
        let bus = SemanticBus::new(24, 24);

        // 4 orthogonal/distinct vectors
        let v1 = vec![1.0, 0.0, 0.0, 0.0];
        let v2 = vec![0.9, 0.1, 0.0, 0.0];
        let v3 = vec![0.0, 1.0, 0.0, 0.0];
        let v4 = vec![0.0, 0.9, 0.1, 0.0];

        bus.write_chunk(
            "raptor_pipe",
            "Chunk 1".into(),
            "Ctx 1".into(),
            v1.clone(),
            "app",
            false,
            MemoryCategory::Fact,
        );
        bus.write_chunk(
            "raptor_pipe",
            "Chunk 2".into(),
            "Ctx 2".into(),
            v2.clone(),
            "app",
            false,
            MemoryCategory::Fact,
        );
        bus.write_chunk(
            "raptor_pipe",
            "Chunk 3".into(),
            "Ctx 3".into(),
            v3.clone(),
            "app",
            false,
            MemoryCategory::Fact,
        );
        bus.write_chunk(
            "raptor_pipe",
            "Chunk 4".into(),
            "Ctx 4".into(),
            v4.clone(),
            "app",
            false,
            MemoryCategory::Fact,
        );

        // Run multi-level RAPTOR with cluster_size = 2 and max_levels = 2
        let (total_summaries, levels_built) = bus
            .build_raptor_tree("raptor_pipe", 2, 2, |prompt| async move {
                format!("Summary of {}", prompt.len())
            })
            .await
            .unwrap();

        // Level 1 builds 2 summaries, Level 2 builds 1 root summary -> 3 total summaries across 2 levels
        assert!(
            levels_built >= 2,
            "RAPTOR must build at least 2 levels recursively"
        );
        assert!(
            total_summaries >= 3,
            "RAPTOR must generate both intermediate and root summaries"
        );

        let contents = bus.get_pipe_contents("raptor_pipe").unwrap();
        let l1_summary = contents
            .iter()
            .find(|c| c.text.starts_with("[RAPTOR-L1]"))
            .unwrap();

        // Centroid vector verification: centroid must NOT simply equal child 0's vector!
        assert_ne!(
            l1_summary.vector.as_slice(),
            v1.as_slice(),
            "Summary vector must be a computed centroid, not child 0's vector"
        );

        // Re-running RAPTOR must filter out previous summaries and not treat them as leaves
        let (rerun_summaries, _) = bus
            .build_raptor_tree("raptor_pipe", 2, 1, |_| async { "Duplicate".into() })
            .await
            .unwrap();
        assert_eq!(
            rerun_summaries, 2,
            "Re-run must cluster only raw leaves, ignoring prior summaries"
        );
    }
}
