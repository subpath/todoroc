use anyhow::{Context, Result};
use std::path::PathBuf;
use tract_onnx::prelude::*;
use tokenizers::Tokenizer;

pub struct Embedder {
    model: SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>,
    tokenizer: Tokenizer,
}

impl Embedder {
    pub fn load(model_dir: &PathBuf) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        let model = tract_onnx::onnx()
            .model_for_path(model_dir.join("model.onnx"))
            .context("Failed to load ONNX model")?
            .into_optimized()
            .context("Failed to optimize model")?
            .into_runnable()
            .context("Failed to make model runnable")?;

        Ok(Self { model, tokenizer })
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self.tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&x| x as i64).collect();
        let type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&x| x as i64).collect();
        let len = ids.len();

        let inputs = tvec![
            tract_ndarray::Array2::from_shape_vec((1, len), ids)?.into_tvalue(),
            tract_ndarray::Array2::from_shape_vec((1, len), mask)?.into_tvalue(),
            tract_ndarray::Array2::from_shape_vec((1, len), type_ids)?.into_tvalue(),
        ];

        let outputs = self.model.run(inputs).context("Model inference failed")?;

        // all-MiniLM-L6-v2 ONNX has two outputs:
        //   0: last_hidden_state  [1, seq_len, 384]
        //   1: sentence_embedding [1, 384]  (already mean-pooled + normalized)
        // Use sentence_embedding if present, otherwise fall back to mean pooling.
        let embedding = if outputs.len() > 1 {
            let out = outputs[1].to_array_view::<f32>()
                .context("Failed to read sentence_embedding output")?;
            out.iter().copied().collect()
        } else {
            let out = outputs[0].to_array_view::<f32>()
                .context("Failed to read last_hidden_state output")?;
            let shape = out.shape();
            let seq_len = shape[1];
            let hidden = shape[2];
            let mut pooled = vec![0f32; hidden];
            for i in 0..seq_len {
                for j in 0..hidden {
                    pooled[j] += out[[0, i, j]];
                }
            }
            for v in &mut pooled { *v /= seq_len as f32; }
            // L2 normalize
            let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 { for v in &mut pooled { *v /= norm; } }
            pooled
        };

        Ok(embedding)
    }
}
