use candle_core::{Result, Tensor};

/// KV-Cache using concatenation for append operations
///
/// This implementation uses `Tensor::cat` instead of `slice_set` for updates,
/// providing significant GPU performance improvements for autoregressive generation.
///
/// # When to Use
///
/// **Recommended for:**
/// - GPU inference (CUDA, Metal)
/// - Autoregressive generation (token-by-token decoding)
///
/// **Use `KvCache` instead for:**
/// - CPU-only inference
/// - When you need fixed memory allocation upfront
///
/// # Example
///
/// ```ignore
/// use candle_nn::kv_cache::ConcatKvCache;
///
/// let mut cache = ConcatKvCache::new(2); // dim=2 for sequence dimension
///
/// // First token (prefill)
/// let k1 = Tensor::randn(0f32, 1., (1, 8, 10, 64), &device)?;
/// let v1 = Tensor::randn(0f32, 1., (1, 8, 10, 64), &device)?;
/// let (k, v) = cache.append(&k1, &v1)?;
///
/// // Subsequent tokens (decode)
/// let k_new = Tensor::randn(0f32, 1., (1, 8, 1, 64), &device)?;
/// let v_new = Tensor::randn(0f32, 1., (1, 8, 1, 64), &device)?;
/// let (k, v) = cache.append(&k_new, &v_new)?;
/// ```
#[derive(Debug, Clone)]
pub struct ConcatKvCache {
    k: Option<Tensor>,
    v: Option<Tensor>,
    dim: usize,
}

impl ConcatKvCache {
    /// Create a new empty concatenation-based KV-cache
    ///
    /// # Arguments
    /// * `dim` - The dimension along which to concatenate
    ///   - For attention with shape `[batch, heads, seq, head_dim]`, use `dim=2`
    ///   - For attention with shape `[batch, seq, heads, head_dim]`, use `dim=1`
    ///
    /// # Example
    /// ```ignore
    /// // For standard transformer attention: [B, H, S, D]
    /// let cache = ConcatKvCache::new(2);
    /// ```
    pub fn new(dim: usize) -> Self {
        Self {
            k: None,
            v: None,
            dim,
        }
    }

    /// Get current sequence length in the cache
    ///
    /// Returns 0 if the cache is empty.
    pub fn current_seq_len(&self) -> usize {
        self.k
            .as_ref()
            .and_then(|k| k.dims().get(self.dim).copied())
            .unwrap_or(0)
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.k.is_none()
    }

    /// Get the concatenation dimension
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Append key and value tensors to the cache
    ///
    /// This is the core operation that uses optimized concatenation kernels.
    ///
    /// # Arguments
    /// * `k` - Key tensor to append (shape: [..., seq_len, ...])
    /// * `v` - Value tensor to append (shape: [..., seq_len, ...])
    ///
    /// # Returns
    /// Tuple of `(full_k, full_v)` containing all cached keys and values,
    /// including the newly appended data.
    pub fn append(&mut self, k: &Tensor, v: &Tensor) -> Result<(Tensor, Tensor)> {
        // Detach inputs to break BackpropOp chain - KV caches are inference-only.
        let k = k.contiguous()?.detach();
        let v = v.contiguous()?.detach();

        self.k = Some(match &self.k {
            None => k,
            Some(k_cache) => Tensor::cat(&[k_cache, &k], self.dim)?.detach(),
        });

        self.v = Some(match &self.v {
            None => v,
            Some(v_cache) => Tensor::cat(&[v_cache, &v], self.dim)?.detach(),
        });

        Ok((
            self.k.as_ref().unwrap().clone(),
            self.v.as_ref().unwrap().clone(),
        ))
    }

    /// Reset the cache (clear all stored keys and values)
    ///
    /// After calling this, `is_empty()` will return `true` and
    /// `current_seq_len()` will return 0.
    pub fn reset(&mut self) {
        self.k = None;
        self.v = None;
    }

    /// Get reference to current K cache data
    ///
    /// Returns `None` if the cache is empty.
    pub fn k(&self) -> Option<&Tensor> {
        self.k.as_ref()
    }

    /// Get reference to current V cache data
    ///
    /// Returns `None` if the cache is empty.
    pub fn v(&self) -> Option<&Tensor> {
        self.v.as_ref()
    }

    /// Get mutable reference to K cache data
    ///
    /// Returns `None` if the cache is empty.
    pub fn k_mut(&mut self) -> Option<&mut Tensor> {
        self.k.as_mut()
    }

    /// Get mutable reference to V cache data
    ///
    /// Returns `None` if the cache is empty.
    pub fn v_mut(&mut self) -> Option<&mut Tensor> {
        self.v.as_mut()
    }

    /// Get owned K and V tensors, consuming the cache
    ///
    /// Returns `None` if the cache is empty.
    pub fn into_inner(self) -> Option<(Tensor, Tensor)> {
        match (self.k, self.v) {
            (Some(k), Some(v)) => Some((k, v)),
            _ => None,
        }
    }

    /// Extracts the raw Key/Value tensors for the OS Pager
    pub fn get_tensors(&self) -> Option<(Tensor, Tensor)> {
        match (&self.k, &self.v) {
            (Some(k), Some(v)) => Some((k.clone(), v.clone())),
            _ => None,
        }
    }

    /// Injects frozen Key/Value tensors from the OS Pager
    pub fn set_tensors(&mut self, k: Tensor, v: Tensor) {
        self.k = Some(k);
        self.v = Some(v);
    }
}
