use candle_core::{Device, Tensor};
use std::collections::HashMap;

/// Key of the token-id tensor stored next to the layer tensors in a paged-out KV cache.
/// It records which tokens the cache was computed from, so a restored cache is only
/// reused as far as it matches the next prompt.
pub const TOKEN_IDS_KEY: &str = "ore_token_ids";

pub struct KvManager;

impl KvManager {
    /// Extracts the live KV-Cache from the RAM and flattens it for the SSD, together
    /// with the token ids it holds (one per cached position).
    pub fn flatten_cache(
        cache: &[Option<(Tensor, Tensor)>],
        token_ids: &[u32],
    ) -> HashMap<String, Tensor> {
        let mut map = HashMap::new();

        for (layer_idx, layer_cache) in cache.iter().enumerate() {
            if let Some((k, v)) = layer_cache {
                // CRITICAL FIX: Force the tensors to be contiguous in memory!
                // Safetensors will corrupt the SSD file if it receives a sliced view.
                // Name them exactly so we know where they go when we wake up
                map.insert(format!("layer_{}_k", layer_idx), k.contiguous().unwrap());
                map.insert(format!("layer_{}_v", layer_idx), v.contiguous().unwrap());
            }
        }
        if !token_ids.is_empty()
            && let Ok(ids) = Tensor::new(token_ids, &Device::Cpu)
        {
            map.insert(TOKEN_IDS_KEY.to_string(), ids);
        }
        map
    }

    /// Reads the SSD HashMap and rebuilds the exact 3D structure the LLM needs
    pub fn unflatten_cache(
        map: &HashMap<String, Tensor>,
        num_layers: usize,
    ) -> Vec<Option<(Tensor, Tensor)>> {
        let mut cache = vec![None; num_layers];

        for (layer_idx, layer_cache) in cache.iter_mut().enumerate().take(num_layers) {
            let k_key = format!("layer_{}_k", layer_idx);
            let v_key = format!("layer_{}_v", layer_idx);

            // If both Key and Value exist for this layer, reconstruct it
            if let (Some(k), Some(v)) = (map.get(&k_key), map.get(&v_key)) {
                *layer_cache = Some((k.clone(), v.clone()));
            }
        }
        cache
    }

    /// The token ids a paged-out cache was computed from. `None` for pages written
    /// before ids were recorded: those can't be checked against the next prompt.
    pub fn token_ids(map: &HashMap<String, Tensor>) -> Option<Vec<u32>> {
        map.get(TOKEN_IDS_KEY)?.to_vec1::<u32>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::DType;

    #[test]
    fn token_ids_survive_a_round_trip_through_safetensors() {
        let k = Tensor::zeros((1, 2, 3, 4), DType::F32, &Device::Cpu).unwrap();
        let cache = vec![Some((k.clone(), k)), None];
        let ids = [151644u32, 872, 198, 9707, 0];

        let path =
            std::env::temp_dir().join(format!("ore_kv_ids_{}.safetensors", std::process::id()));
        candle_core::safetensors::save(&KvManager::flatten_cache(&cache, &ids), &path).unwrap();
        let loaded = candle_core::safetensors::load(&path, &Device::Cpu).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(KvManager::token_ids(&loaded).unwrap(), ids);
        let restored = KvManager::unflatten_cache(&loaded, 2);
        assert!(restored[0].is_some() && restored[1].is_none());
    }

    #[test]
    fn a_page_without_token_ids_is_reported_as_unverifiable() {
        let k = Tensor::zeros((1, 2, 3, 4), DType::F32, &Device::Cpu).unwrap();
        let map = KvManager::flatten_cache(&[Some((k.clone(), k))], &[]);
        assert!(KvManager::token_ids(&map).is_none());
    }
}
