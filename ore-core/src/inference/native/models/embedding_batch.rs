//! Batched forward passes for sentence embedders (BERT, NomicBert).
//!
//! Embedding one text per forward pass leaves the CPU's matrix units idle on tiny
//! matrices. Here every input is tokenized once, inputs are sorted by token count,
//! and texts of similar length run together as one right-padded batch with an
//! attention mask. Padding is masked out of attention and out of mean pooling, so
//! each vector matches what that text would get on its own (up to float rounding).
//!
//! Batches are bounded three ways, so one long input can't blow up memory:
//!   - at most `MAX_BATCH_TEXTS` texts,
//!   - at most `MAX_BATCH_TOKENS` padded tokens (activations grow with batch × length),
//!   - at most `MAX_BATCH_ATTENTION` padded attention cells per head
//!     (scores grow with batch × length²).
//!
//! and a text only joins a batch whose padded length is at most about twice its
//! own (`PAD_SLACK` tokens of grace), so short texts are never padded to a long one.
//! A text that exceeds a budget on its own still runs, alone in its batch.

use candle_core::{D, Device, Result, Tensor};
use tokenizers::Tokenizer;

pub const MAX_BATCH_TEXTS: usize = 32;
pub const MAX_BATCH_TOKENS: usize = 8 * 1024;
pub const MAX_BATCH_ATTENTION: usize = 4 * 1024 * 1024;
/// Padding a very short text costs little, so texts within this many tokens of
/// half the batch length may still join it.
const PAD_SLACK: usize = 16;

/// Groups text indices into batches. Longest texts come first, so each batch's
/// padded length is the length of its first member.
pub fn plan_batches(lengths: &[usize]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..lengths.len()).collect();
    // Stable sort: equal lengths keep input order, so batching is deterministic.
    order.sort_by(|&a, &b| lengths[b].cmp(&lengths[a]));

    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut padded_len = 0;

    for idx in order {
        if current.is_empty() {
            padded_len = lengths[idx].max(1);
            current.push(idx);
            continue;
        }
        let n = current.len() + 1;
        let fits = n <= MAX_BATCH_TEXTS
            && n * padded_len <= MAX_BATCH_TOKENS
            && n * padded_len * padded_len <= MAX_BATCH_ATTENTION
            && padded_len <= 2 * lengths[idx] + PAD_SLACK;
        if fits {
            current.push(idx);
        } else {
            batches.push(std::mem::take(&mut current));
            padded_len = lengths[idx].max(1);
            current.push(idx);
        }
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// Tokenizes `texts`, runs them through `forward` in padded batches, and returns
/// one vector per input, in input order.
///
/// The tokenizer must have padding disabled (each batch is padded here) and
/// truncation set to the model's position limit. `forward` receives `input_ids`
/// and `attention_mask`, both `(batch, seq_len)` u32 with 1 = token and 0 = padding,
/// and returns pooled, normalized embeddings of shape `(batch, dim)`.
pub fn embed_in_batches<F>(
    tokenizer: &Tokenizer,
    device: &Device,
    texts: Vec<String>,
    mut forward: F,
) -> anyhow::Result<Vec<Vec<f32>>>
where
    F: FnMut(&Tensor, &Tensor) -> Result<Tensor>,
{
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let encodings = tokenizer
        .encode_batch(texts, true)
        .map_err(anyhow::Error::msg)?;
    let lengths: Vec<usize> = encodings.iter().map(|e| e.get_ids().len()).collect();
    let pad_id = tokenizer.token_to_id("[PAD]").unwrap_or(0);

    let mut vectors: Vec<Option<Vec<f32>>> = vec![None; encodings.len()];

    for batch in plan_batches(&lengths) {
        let padded_len = batch.iter().map(|&i| lengths[i]).max().unwrap_or(0).max(1);
        let mut ids = Vec::with_capacity(batch.len() * padded_len);
        let mut mask = Vec::with_capacity(batch.len() * padded_len);
        for &i in &batch {
            let real = encodings[i].get_ids();
            ids.extend_from_slice(real);
            ids.extend(std::iter::repeat_n(pad_id, padded_len - real.len()));
            mask.extend(std::iter::repeat_n(1u32, real.len()));
            mask.extend(std::iter::repeat_n(0u32, padded_len - real.len()));
        }

        let ids = Tensor::from_vec(ids, (batch.len(), padded_len), device)?;
        let mask = Tensor::from_vec(mask, (batch.len(), padded_len), device)?;
        let pooled = forward(&ids, &mask)?.to_vec2::<f32>()?;
        anyhow::ensure!(
            pooled.len() == batch.len(),
            "embedder returned {} vectors for a batch of {}",
            pooled.len(),
            batch.len()
        );

        for (&slot, vector) in batch.iter().zip(pooled) {
            vectors[slot] = Some(vector);
        }
    }

    vectors
        .into_iter()
        .enumerate()
        .map(|(i, v)| v.ok_or_else(|| anyhow::anyhow!("text {i} was never embedded")))
        .collect()
}

/// Mean-pools token embeddings over real tokens only.
///
/// `hidden_states` is `(batch, seq_len, dim)`; `attention_mask` is `(batch, seq_len)`
/// with 1 for real tokens and 0 for padding. Returns `(batch, dim)`.
pub fn mean_pooling(hidden_states: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
    let (batch, seq_len, hidden_dim) = hidden_states.dims3()?;
    let mask = attention_mask.to_dtype(hidden_states.dtype())?;
    let mask_expanded = mask
        .unsqueeze(2)?
        .broadcast_as((batch, seq_len, hidden_dim))?;
    let sum_hidden = (hidden_states * &mask_expanded)?.sum(1)?;
    let sum_mask = mask
        .sum(1)?
        .unsqueeze(1)?
        .broadcast_as((batch, hidden_dim))?
        .clamp(1e-9, f64::MAX)?;
    sum_hidden / sum_mask
}

/// L2-normalizes each row. A zero row stays zero instead of becoming NaN.
pub fn l2_normalize(x: &Tensor) -> Result<Tensor> {
    let norm = x
        .sqr()?
        .sum_keepdim(D::Minus1)?
        .sqrt()?
        .clamp(1e-12, f64::MAX)?;
    x.broadcast_div(&norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flatten(batches: &[Vec<usize>]) -> Vec<usize> {
        let mut all: Vec<usize> = batches.iter().flatten().copied().collect();
        all.sort();
        all
    }

    #[test]
    fn every_text_lands_in_exactly_one_batch() {
        let lengths: Vec<usize> = (0..100).map(|i| (i * 37) % 300 + 1).collect();
        let batches = plan_batches(&lengths);
        assert_eq!(flatten(&batches), (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn batches_respect_every_budget() {
        let lengths: Vec<usize> = (0..500).map(|i| (i * 7919) % 2000 + 1).collect();
        for batch in plan_batches(&lengths) {
            let padded = batch.iter().map(|&i| lengths[i]).max().unwrap();
            if batch.len() > 1 {
                assert!(batch.len() <= MAX_BATCH_TEXTS);
                assert!(batch.len() * padded <= MAX_BATCH_TOKENS);
                assert!(batch.len() * padded * padded <= MAX_BATCH_ATTENTION);
            }
        }
    }

    #[test]
    fn short_texts_fill_whole_batches() {
        let batches = plan_batches(&[20; 100]);
        assert_eq!(
            batches.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![32, 32, 32, 4]
        );
    }

    #[test]
    fn an_oversized_text_runs_alone() {
        let batches = plan_batches(&[8000, 10, 10]);
        assert_eq!(batches[0], vec![0]);
        assert_eq!(flatten(&batches), vec![0, 1, 2]);
    }

    #[test]
    fn padding_waste_is_bounded() {
        let lengths: Vec<usize> = (0..400).map(|i| (i * 7919) % 1500 + 1).collect();
        for batch in plan_batches(&lengths) {
            let padded = batch.iter().map(|&i| lengths[i]).max().unwrap();
            for &i in &batch {
                assert!(
                    padded <= 2 * lengths[i] + PAD_SLACK,
                    "text of {} padded to {}",
                    lengths[i],
                    padded
                );
            }
        }
    }

    #[test]
    fn similar_lengths_are_batched_together() {
        // Longest first, so a long text never pads a batch of short ones.
        let batches = plan_batches(&[5, 900, 6, 950, 7]);
        assert_eq!(batches[0], vec![3, 1]);
        assert_eq!(batches[1], vec![4, 2, 0]);
    }

    #[test]
    fn empty_input_plans_nothing() {
        assert!(plan_batches(&[]).is_empty());
    }
}
