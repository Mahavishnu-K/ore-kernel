use ore_common::protocol::MemoryCategory;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

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

/// Append-only NVMe storage for exact f32 vectors (DiskANN pattern)
#[derive(Debug)]
pub struct DiskVectorStore {
    path: PathBuf,
}

impl DiskVectorStore {
    pub fn new(pipe_name: &str) -> Self {
        let path = ore_common::paths::get_memory_dir().join(format!("{}.f32vecs", pipe_name));
        Self { path }
    }

    pub fn append(&self, vector: &[f32]) -> Result<u64, std::io::Error> {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.path)?;

        let offset = file.seek(SeekFrom::End(0))?;
        let byte_slice = unsafe {
            std::slice::from_raw_parts(vector.as_ptr() as *const u8, std::mem::size_of_val(vector))
        };
        file.write_all(byte_slice)?;
        Ok(offset)
    }

    pub fn read_vector(&self, offset: u64, dim: usize) -> Result<Vec<f32>, std::io::Error> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&self.path)?;
        file.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0.0f32; dim];
        let byte_slice = unsafe {
            std::slice::from_raw_parts_mut(
                buffer.as_mut_ptr() as *mut u8,
                std::mem::size_of_val(&buffer[..]),
            )
        };
        file.read_exact(byte_slice)?;
        Ok(buffer)
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

/// Compact Product Quantization codebook: splits vector into M sub-spaces of 16 dims,
/// quantizing each into an 8-bit centroid index (97% storage savings).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqCodebook {
    pub num_subspaces: usize,
    pub sub_dim: usize,
    pub centroids: Vec<Vec<f32>>, // [num_subspaces * 256][sub_dim]
}

impl PqCodebook {
    pub fn new(total_dim: usize, num_subspaces: usize) -> Self {
        let sub_dim = (total_dim / num_subspaces).max(1);
        let mut centroids = Vec::with_capacity(num_subspaces * 256);
        for s in 0..num_subspaces {
            for c in 0..256 {
                let val = ((c as f32 - 128.0) / 128.0) * (1.0 / (s + 1) as f32);
                centroids.push(vec![val; sub_dim]);
            }
        }
        Self {
            num_subspaces,
            sub_dim,
            centroids,
        }
    }

    pub fn encode(&self, v: &[f32]) -> Vec<u8> {
        let mut codes = Vec::with_capacity(self.num_subspaces);
        for m in 0..self.num_subspaces {
            let start = m * self.sub_dim;
            let end = (start + self.sub_dim).min(v.len());
            let sub_v = &v[start..end];

            let mut best_c = 0u8;
            let mut best_dist = f32::MAX;
            for c in 0..256 {
                let centroid_idx = m * 256 + c;
                if let Some(centroid) = self.centroids.get(centroid_idx) {
                    let dist: f32 = sub_v
                        .iter()
                        .zip(centroid.iter())
                        .map(|(&x, &y)| (x - y) * (x - y))
                        .sum();
                    if dist < best_dist {
                        best_dist = dist;
                        best_c = c as u8;
                    }
                }
            }
            codes.push(best_c);
        }
        codes
    }

    pub fn compute_distance_table(&self, query: &[f32]) -> Vec<f32> {
        let mut table = vec![0.0f32; self.num_subspaces * 256];
        for m in 0..self.num_subspaces {
            let start = m * self.sub_dim;
            let end = (start + self.sub_dim).min(query.len());
            let sub_q = &query[start..end];

            for c in 0..256 {
                let centroid_idx = m * 256 + c;
                if let Some(centroid) = self.centroids.get(centroid_idx) {
                    let dot: f32 = sub_q
                        .iter()
                        .zip(centroid.iter())
                        .map(|(&x, &y)| x * y)
                        .sum();
                    table[m * 256 + c] = dot;
                }
            }
        }
        table
    }

    #[inline(always)]
    pub fn score_pq(&self, codes: &[u8], distance_table: &[f32]) -> f32 {
        let mut score = 0.0f32;
        for (m, &c) in codes.iter().enumerate() {
            score += distance_table[m * 256 + c as usize];
        }
        score
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
