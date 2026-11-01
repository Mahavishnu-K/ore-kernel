use memmap2::Mmap;
use ore_common::protocol::MemoryCategory;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

/// High-performance memory-mapped storage for dense f32 vectors (DiskANN pattern).
/// Uses OS Page Cache mmap to eliminate syscall overhead and avoid per-query file allocations.
#[derive(Debug)]
pub struct MmapVectorStore {
    path: PathBuf,
    file: Mutex<File>,
    mmap: RwLock<Option<Mmap>>,
    vector_dim: usize,
}

impl MmapVectorStore {
    pub fn new(path: PathBuf, vector_dim: usize) -> Result<Self, std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        let meta = file.metadata()?;
        let mmap = if meta.len() > 0 {
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };

        Ok(Self {
            path,
            file: Mutex::new(file),
            mmap: RwLock::new(mmap),
            vector_dim,
        })
    }

    pub fn for_pipe(pipe_name: &str, vector_dim: usize) -> Result<Self, std::io::Error> {
        let path = ore_common::paths::get_memory_dir().join(format!("{}.f32vecs", pipe_name));
        Self::new(path, vector_dim)
    }

    /// Appends a vector to the end of the file and remaps the virtual memory address space.
    pub fn append(&self, vector: &[f32]) -> Result<u64, std::io::Error> {
        let mut file = self.file.lock().unwrap();
        let offset = file.seek(SeekFrom::End(0))?;

        let byte_slice =
            unsafe { std::slice::from_raw_parts(vector.as_ptr() as *const u8, vector.len() * 4) };
        file.write_all(byte_slice)?;
        file.flush()?;

        let mut mmap_guard = self.mmap.write().unwrap();
        let new_mmap = unsafe { Mmap::map(&*file)? };
        *mmap_guard = Some(new_mmap);

        Ok(offset)
    }

    /// Zero-copy accessor: executes a closure over the memory-mapped vector slice with 0 allocations.
    #[inline(always)]
    pub fn with_vector<R>(
        &self,
        offset: u64,
        f: impl FnOnce(&[f32]) -> R,
    ) -> Result<R, std::io::Error> {
        let mmap_guard = self.mmap.read().unwrap();
        if let Some(mmap) = mmap_guard.as_ref() {
            let start = offset as usize;
            let end = start + self.vector_dim * 4;

            if end <= mmap.len() {
                let ptr = unsafe { mmap.as_ptr().add(start) as *const f32 };
                let slice = unsafe { std::slice::from_raw_parts(ptr, self.vector_dim) };
                return Ok(f(slice));
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "Vector offset out of mmap bounds",
        ))
    }

    /// Reads and clones an owned f32 vector from the memory map.
    #[inline(always)]
    pub fn read_vector(&self, offset: u64) -> Result<Vec<f32>, std::io::Error> {
        self.with_vector(offset, |slice| slice.to_vec())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 1-byte Scalar Quantization (SQ8): Compresses f32 in [-1.0, 1.0] to signed i8.
/// Yields 75% RAM reduction and executes via fast integer SIMD math.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sq8Vector {
    pub data: Vec<i8>,
}

impl Sq8Vector {
    pub fn quantize(v: &[f32]) -> Self {
        let data = v
            .iter()
            .map(|&x| (x.clamp(-1.0, 1.0) * 127.0).round() as i8)
            .collect();
        Self { data }
    }

    #[inline(always)]
    pub fn dot_product(a: &[i8], b: &[i8]) -> f32 {
        let chunks_a = a.chunks_exact(8);
        let chunks_b = b.chunks_exact(8);
        let rem_a = chunks_a.remainder();
        let rem_b = chunks_b.remainder();

        let mut sum0: i32 = 0;
        let mut sum1: i32 = 0;
        let mut sum2: i32 = 0;
        let mut sum3: i32 = 0;
        let mut sum4: i32 = 0;
        let mut sum5: i32 = 0;
        let mut sum6: i32 = 0;
        let mut sum7: i32 = 0;

        for (ca, cb) in chunks_a.zip(chunks_b) {
            sum0 += ca[0] as i32 * cb[0] as i32;
            sum1 += ca[1] as i32 * cb[1] as i32;
            sum2 += ca[2] as i32 * cb[2] as i32;
            sum3 += ca[3] as i32 * cb[3] as i32;
            sum4 += ca[4] as i32 * cb[4] as i32;
            sum5 += ca[5] as i32 * cb[5] as i32;
            sum6 += ca[6] as i32 * cb[6] as i32;
            sum7 += ca[7] as i32 * cb[7] as i32;
        }

        let mut total = (sum0 + sum1) + (sum2 + sum3) + (sum4 + sum5) + (sum6 + sum7);
        for (&ra, &rb) in rem_a.iter().zip(rem_b.iter()) {
            total += ra as i32 * rb as i32;
        }

        total as f32 / 16129.0 // (127.0 * 127.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sq8Chunk {
    pub id: u64,
    pub text: Arc<String>,
    pub context_payload: Arc<String>,
    pub sq8_vector: Sq8Vector,
    pub f32_offset: u64,
    pub source_app: String,
    pub timestamp: u64,
    pub category: MemoryCategory,
}

// ─── MODE B: LEANN PRODUCT QUANTIZATION (PQ) ─────────────────────────────────

/// Compact Product Quantization codebook: splits vector into M sub-spaces of `sub_dim` components.
/// Uses contiguous memory layout and Asymmetric Distance Computation (ADC) for sub-microsecond scans.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqCodebook {
    pub total_dim: usize,
    pub num_subspaces: usize,
    pub sub_dim: usize,
    /// Flattened centroids: [num_subspaces * 256 * sub_dim]
    pub centroids: Vec<f32>,
}

impl PqCodebook {
    /// Creates a codebook with calibrated harmonic orthogonal centroid distribution.
    /// Eliminates isotropic diagonal collapse by distributing variance across distinct spatial phases.
    pub fn new(total_dim: usize, num_subspaces: usize) -> Self {
        let sub_dim = (total_dim / num_subspaces).max(1);
        let mut centroids = Vec::with_capacity(num_subspaces * 256 * sub_dim);

        for s in 0..num_subspaces {
            for c in 0..256 {
                let mut sub = Vec::with_capacity(sub_dim);
                for d in 0..sub_dim {
                    // High-frequency harmonic phase dispersion (ensures non-degenerate directional variance)
                    let phase = ((c as f32 + 1.0) * 17.0
                        + (d as f32 + 1.0) * 31.0
                        + (s as f32 + 1.0) * 53.0)
                        .sin();
                    sub.push(phase);
                }
                // L2-normalize the sub-centroid
                let norm = sub.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
                for x in &mut sub {
                    *x /= norm;
                }
                centroids.extend(sub);
            }
        }

        Self {
            total_dim,
            num_subspaces,
            sub_dim,
            centroids,
        }
    }

    /// Resolves pointer to centroid: subspace `m`, cluster index `c`
    #[inline(always)]
    pub fn centroid(&self, m: usize, c: usize) -> &[f32] {
        let offset = (m * 256 + c) * self.sub_dim;
        &self.centroids[offset..offset + self.sub_dim]
    }

    /// Calibrates the codebook using Mini-Batch K-Means across sample vectors
    pub fn train_mini_batch(&mut self, samples: &[Vec<f32>], iterations: usize) {
        if samples.is_empty() {
            return;
        }

        for m in 0..self.num_subspaces {
            let q_start = m * self.sub_dim;
            let q_end = (q_start + self.sub_dim).min(self.total_dim);
            let sub_dim = self.sub_dim;

            let sub_samples: Vec<&[f32]> = samples
                .iter()
                .filter(|v| v.len() >= q_end)
                .map(|v| &v[q_start..q_end])
                .collect();

            if sub_samples.is_empty() {
                continue;
            }

            for _iter in 0..iterations {
                let mut counts = vec![0u32; 256];
                let mut sums = vec![0.0f32; 256 * sub_dim];

                // Assign samples to nearest subspace centroid
                for &sample in &sub_samples {
                    let mut best_c = 0usize;
                    let mut best_dot = f32::NEG_INFINITY;

                    for c in 0..256 {
                        let centroid = self.centroid(m, c);
                        let mut dot = 0.0f32;
                        for i in 0..sub_dim {
                            dot += sample[i] * centroid[i];
                        }
                        if dot > best_dot {
                            best_dot = dot;
                            best_c = c;
                        }
                    }

                    counts[best_c] += 1;
                    let s_offset = best_c * sub_dim;
                    for i in 0..sub_dim {
                        sums[s_offset + i] += sample[i];
                    }
                }

                // Update centroids
                for (c, &count) in counts.iter().enumerate() {
                    if count > 0 {
                        let c_offset = (m * 256 + c) * sub_dim;
                        let s_offset = c * sub_dim;
                        let count_f = count as f32;

                        let mut norm_sq = 0.0f32;
                        for i in 0..sub_dim {
                            let mean = sums[s_offset + i] / count_f;
                            self.centroids[c_offset + i] =
                                0.7 * self.centroids[c_offset + i] + 0.3 * mean;
                            norm_sq += self.centroids[c_offset + i] * self.centroids[c_offset + i];
                        }

                        let norm = norm_sq.sqrt().max(1e-9);
                        for i in 0..sub_dim {
                            self.centroids[c_offset + i] /= norm;
                        }
                    }
                }
            }
        }
    }

    /// Encodes a full-dimension f32 vector into `num_subspaces` 1-byte indices (M bytes).
    pub fn encode(&self, v: &[f32]) -> Vec<u8> {
        let mut codes = Vec::with_capacity(self.num_subspaces);
        for m in 0..self.num_subspaces {
            let start = m * self.sub_dim;
            let end = (start + self.sub_dim).min(v.len());
            let sub_v = &v[start..end];

            let mut best_c = 0u8;
            let mut best_dot = f32::NEG_INFINITY;
            for c in 0..256 {
                let centroid = self.centroid(m, c);
                let mut dot = 0.0f32;
                for i in 0..self.sub_dim.min(sub_v.len()) {
                    dot += sub_v[i] * centroid[i];
                }
                if dot > best_dot {
                    best_dot = dot;
                    best_c = c as u8;
                }
            }
            codes.push(best_c);
        }
        codes
    }

    /// Precomputes the Asymmetric Distance Computation (ADC) Lookup Table.
    /// Decomposes query Q into M sub-queries and calculates dot product against all 256 centroids per subspace.
    pub fn compute_adc_table(&self, query: &[f32]) -> Vec<f32> {
        let mut table = vec![0.0f32; self.num_subspaces * 256];
        for m in 0..self.num_subspaces {
            let start = m * self.sub_dim;
            let end = (start + self.sub_dim).min(query.len());
            let sub_q = if start < query.len() {
                &query[start..end]
            } else {
                &[]
            };
            let sub_len = sub_q.len();

            let table_offset = m * 256;
            for c in 0..256 {
                let centroid = self.centroid(m, c);
                let mut dot = 0.0f32;
                for i in 0..self.sub_dim.min(sub_len) {
                    dot += sub_q[i] * centroid[i];
                }
                table[table_offset + c] = dot;
            }
        }
        table
    }

    #[inline(always)]
    pub fn compute_distance_table(&self, query: &[f32]) -> Vec<f32> {
        self.compute_adc_table(query)
    }

    /// Asymmetric Distance Computation scan: scores an M-byte chunk against the ADC table in ~20 nanoseconds.
    #[inline(always)]
    pub fn score_code(&self, codes: &[u8], adc_table: &[f32]) -> f32 {
        let mut score = 0.0f32;
        for (m, &c) in codes.iter().enumerate() {
            let idx = m * 256 + c as usize;
            if idx < adc_table.len() {
                score += adc_table[idx];
            }
        }
        score
    }

    #[inline(always)]
    pub fn score_pq(&self, codes: &[u8], distance_table: &[f32]) -> f32 {
        self.score_code(codes, distance_table)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqChunk {
    pub id: u64,
    pub text: Arc<String>,
    pub context_payload: Arc<String>,
    pub pq_codes: Vec<u8>, // Compressed to 32 or 48 bytes!
    pub source_app: String,
    pub timestamp: u64,
    pub category: MemoryCategory,
}
