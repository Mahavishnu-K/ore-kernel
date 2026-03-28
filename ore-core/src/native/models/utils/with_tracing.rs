//! -------------------------------------------------------------------------
//! Copyright (c) Hugging Face and the Candle Contributors.
//! Licensed under the Apache License, Version 2.0.
//!
//! This file contains code adapted from the huggingface/candle repository:
//! https://github.com/huggingface/candle
//! 
//! Modifications made for the ORE Kernel:
//! - Stripped unused submodules to eliminate internal crate dependencies.
//! - Added the `SystemEmbedder` OS wrapper for the Semantic Bus integration.
//! -------------------------------------------------------------------------

use candle_core::{Module, Result, Tensor};
use candle_nn::VarBuilder;

#[derive(Debug, Clone)]
pub struct Linear {
    inner: candle_nn::Linear,
    span: tracing::Span,
}

impl Linear {
    pub fn from_weights(weights: Tensor, bias: Option<Tensor>) -> Self {
        let inner = candle_nn::Linear::new(weights, bias);
        let span = tracing::span!(tracing::Level::TRACE, "linear");
        Self { inner, span }
    }
}

pub fn linear_b(d1: usize, d2: usize, b: bool, vb: VarBuilder) -> Result<Linear> {
    let inner = candle_nn::linear_b(d1, d2, b, vb)?;
    let span = tracing::span!(tracing::Level::TRACE, "linear");
    Ok(Linear { inner, span })
}

pub fn linear(d1: usize, d2: usize, vb: VarBuilder) -> Result<Linear> {
    let inner = candle_nn::linear(d1, d2, vb)?;
    let span = tracing::span!(tracing::Level::TRACE, "linear");
    Ok(Linear { inner, span })
}

pub fn linear_no_bias(d1: usize, d2: usize, vb: VarBuilder) -> Result<Linear> {
    let inner = candle_nn::linear_no_bias(d1, d2, vb)?;
    let span = tracing::span!(tracing::Level::TRACE, "linear");
    Ok(Linear { inner, span })
}

impl Module for Linear {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let _enter = self.span.enter();
        self.inner.forward(xs)
    }
}

#[derive(Clone, Debug)]
pub struct LayerNorm {
    inner: candle_nn::LayerNorm,
    span: tracing::Span,
}

impl LayerNorm {
    pub fn new(weight: Tensor, bias: Tensor, eps: f64) -> Self {
        let inner = candle_nn::LayerNorm::new(weight, bias, eps);
        let span = tracing::span!(tracing::Level::TRACE, "layer-norm");
        Self { inner, span }
    }
}

impl Module for LayerNorm {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let _enter = self.span.enter();
        self.inner.forward(xs)
    }
}

pub fn layer_norm<C: Into<candle_nn::LayerNormConfig>>(
    size: usize,
    c: C,
    vb: VarBuilder,
) -> Result<LayerNorm> {
    let inner = candle_nn::layer_norm(size, c, vb)?;
    let span = tracing::span!(tracing::Level::TRACE, "layer-norm");
    Ok(LayerNorm { inner, span })
}
