use anyhow::{Context, Result};
use std::path::PathBuf;
use tract_onnx::prelude::*;
use tokenizers::Tokenizer;

// Fixed sequence length: covers all practical todo text, lets the optimizer
// specialize the graph (no symbolic dimensions).
const SEQ_LEN: usize = 128;

// Name of the pre-compiled NNEF model cache stored alongside the ONNX file.
// Loading from NNEF skips the heavy graph-optimization pass, which can OOM
// on large models in release builds.
const NNEF_CACHE: &str = "model.nnef";

pub struct Embedder {
    model: SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>,
    tokenizer: Tokenizer,
}

impl Embedder {
    pub fn load(model_dir: &PathBuf) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        let nnef_path = model_dir.join(NNEF_CACHE);
        let model = if nnef_path.exists() {
            // Fast path: load pre-compiled graph — no optimization pass needed.
            tract_nnef::nnef()
                .model_for_path(&nnef_path)
                .context("Failed to load compiled model cache")?
                .into_runnable()
                .context("Failed to make model runnable")?
        } else {
            // Slow path: compile from ONNX and cache for future runs.
            // This can use a lot of memory on large models; run via debug binary
            // (`make compile-model`) if the release binary gets OOM-killed here.
            let typed = Self::compile_onnx(model_dir)?;
            if let Err(e) = tract_nnef::nnef().write_to_dir(&typed, &nnef_path) {
                eprintln!("Warning: could not cache compiled model: {e}");
            }
            typed.into_runnable().context("Failed to make model runnable")?
        };

        Ok(Self { model, tokenizer })
    }

    /// Compile the ONNX model to an optimized TypedModel.
    /// Call this once; subsequent loads use the NNEF cache.
    pub fn compile_onnx(
        model_dir: &PathBuf,
    ) -> Result<TypedModel> {
        let input_fact = InferenceFact::dt_shape(i64::datum_type(), &[1usize, SEQ_LEN]);
        tract_onnx::onnx()
            .model_for_path(model_dir.join("model.onnx"))
            .context("Failed to load ONNX model")?
            .with_input_fact(0, input_fact.clone())?
            .with_input_fact(1, input_fact.clone())?
            .with_input_fact(2, input_fact)?
            .into_optimized()
            .context("Failed to optimize model")
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self.tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        // Truncate to SEQ_LEN and pad with zeros (PAD=0, mask=0, type_id=0).
        let mut ids      = vec![0i64; SEQ_LEN];
        let mut mask     = vec![0i64; SEQ_LEN];
        let mut type_ids = vec![0i64; SEQ_LEN];
        let actual_len = encoding.get_ids().len().min(SEQ_LEN);
        for i in 0..actual_len {
            ids[i]      = encoding.get_ids()[i] as i64;
            mask[i]     = encoding.get_attention_mask()[i] as i64;
            type_ids[i] = encoding.get_type_ids()[i] as i64;
        }

        let inputs = tvec![
            tract_ndarray::Array2::from_shape_vec((1, SEQ_LEN), ids)?.into_tvalue(),
            tract_ndarray::Array2::from_shape_vec((1, SEQ_LEN), mask)?.into_tvalue(),
            tract_ndarray::Array2::from_shape_vec((1, SEQ_LEN), type_ids)?.into_tvalue(),
        ];

        let outputs = self.model.run(inputs).context("Model inference failed")?;

        // Use sentence_embedding output if present, otherwise mean-pool last_hidden_state.
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
            let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 { for v in &mut pooled { *v /= norm; } }
            pooled
        };

        Ok(embedding)
    }
}
