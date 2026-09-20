use candle_core::Tensor;
use std::collections::HashMap;

pub struct KvManager;

impl KvManager {
    /// Extracts the live KV-Cache from the RAM and flattens it for the SSD
    pub fn flatten_cache(cache: &[Option<(Tensor, Tensor)>]) -> HashMap<String, Tensor> {
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
}
