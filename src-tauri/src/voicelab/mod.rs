//! The Voice Lab's platform half: the GPU check, the optional module, the service.
//!
//! Deliberately thin. Everything that can be decided without Windows —
//! whether a GPU qualifies, what a driver version means, which uv release to
//! fetch — lives in `tp_model::voicelab` where it is tested. What is left
//! here is the platform call and the process supervision.
//!
//! See `docs/design/0009-voice-lab.md`.

pub mod gpu;
pub mod module;
pub mod service;
