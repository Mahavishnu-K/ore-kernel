use super::quantized_var_builder::VarBuilder;
use candle_core::quantized::QTensor;
use candle_core::{Module, Result, Tensor};

#[derive(Debug, Clone)]
pub struct RmsNorm {
    weight: Tensor,
    eps: f64,
    span: tracing::Span,
}

impl RmsNorm {
    pub fn new(size: usize, eps: f64, vb: VarBuilder) -> Result<Self> {
        let span = tracing::span!(tracing::Level::TRACE, "rms-norm");
        let weight = vb.get(size, "weight")?.dequantize(vb.device())?;
        Ok(Self { weight, eps, span })
    }

    pub fn from_qtensor(weight: QTensor, eps: f64) -> Result<Self> {
        let span = tracing::span!(tracing::Level::TRACE, "rms-norm");
        let weight = weight.dequantize(&weight.device())?;
        Ok(Self { weight, eps, span })
    }
}

impl Module for RmsNorm {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let _enter = self.span.enter();
        candle_nn::ops::rms_norm(x, &self.weight, self.eps as f32)
    }
}
