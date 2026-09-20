//! Backend-independent Rendering Hardware Interface and renderer policy.
//!
//! Concrete graphics APIs live in leaf adapter crates. This crate exposes only Rustic-owned
//! descriptors, typed generational handles, commands, lifecycle state, and diagnostics.

mod rhi;

pub use rhi::*;

use serde::{Deserialize, Serialize};

/// Graphics APIs supported by the engine-owned RHI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererBackend {
    /// Khronos Vulkan backend.
    Vulkan,
    /// Microsoft Direct3D 12 backend.
    Direct3D12,
    /// Apple Metal backend.
    Metal,
    /// Compatibility OpenGL backend.
    OpenGl,
    /// Software rasterizer such as WARP or llvmpipe.
    Software,
}

/// Broad operating-system family used for API preference ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingSystem {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    MacOs,
    /// Linux desktop.
    Linux,
    /// Any future/unknown platform.
    Other,
}

/// Physical GPU classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    /// Dedicated discrete GPU.
    Discrete,
    /// Integrated GPU sharing system memory.
    Integrated,
    /// CPU/software rasterizer.
    Software,
    /// Driver did not report a useful classification.
    Unknown,
}

/// Measured capabilities for a successfully created adapter/device candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterCandidate {
    /// Backend used to expose the adapter.
    pub backend: RendererBackend,
    /// Reported physical device class.
    pub device_class: DeviceClass,
    /// Dedicated memory, or the conservative budget for an integrated adapter.
    pub memory_mib: u32,
    /// Whether a presentation surface can be created for the primary display.
    pub can_present: bool,
    /// Required baseline engine feature set is supported.
    pub supports_baseline: bool,
    /// Optional high-end feature score in the inclusive range 0..=100.
    pub feature_score: u8,
    /// Driver stability score in the inclusive range 0..=100.
    pub stability_score: u8,
    /// Lightweight startup benchmark score, when one has been run.
    pub benchmark_score: Option<u16>,
}

/// Power policy affecting adapter and quality selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPolicy {
    /// Prefer maximum graphics capability.
    Performance,
    /// Balance quality and energy use.
    Balanced,
    /// Prefer lower-power adapters and settings.
    Efficiency,
}

/// Automatically selected quality configuration family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityProfile {
    /// High-end feature set, including optional ray tracing where available.
    Ultra,
    /// High fidelity without the most expensive defaults.
    High,
    /// Stable quality/performance compromise.
    Balanced,
    /// Reduced GPU and memory pressure.
    Low,
    /// Minimum feature set and maximum compatibility.
    Compatibility,
}

/// Result of backend selection, including a score useful in diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererSelection {
    /// Chosen graphics backend.
    pub backend: RendererBackend,
    /// Chosen device class.
    pub device_class: DeviceClass,
    /// Chosen initial quality profile.
    pub quality: QualityProfile,
    /// Reproducible policy score for diagnostics.
    pub score: i32,
}

/// Selects the safest high-scoring adapter. Invalid candidates are never considered.
pub fn select_renderer(
    os: OperatingSystem,
    policy: PowerPolicy,
    system_memory_mib: u32,
    candidates: &[AdapterCandidate],
) -> Option<RendererSelection> {
    candidates
        .iter()
        .filter(|candidate| candidate.can_present && candidate.supports_baseline)
        .map(|candidate| {
            let score = score_candidate(os, policy, candidate);
            (candidate, score)
        })
        .max_by(
            |(left_candidate, left_score), (right_candidate, right_score)| {
                let preferred = |candidate: &AdapterCandidate| {
                    os == OperatingSystem::Windows
                        && candidate.device_class == DeviceClass::Discrete
                        && candidate.backend == RendererBackend::Vulkan
                };
                preferred(left_candidate)
                    .cmp(&preferred(right_candidate))
                    .then_with(|| left_score.cmp(right_score))
                    .then_with(|| {
                        // The explicit backend ordinal makes ties stable across probe order.
                        backend_tie_break(os, left_candidate.backend)
                            .cmp(&backend_tie_break(os, right_candidate.backend))
                    })
            },
        )
        .map(|(candidate, score)| RendererSelection {
            backend: candidate.backend,
            device_class: candidate.device_class,
            quality: classify_quality(score, candidate.memory_mib, system_memory_mib),
            score,
        })
}

fn score_candidate(os: OperatingSystem, policy: PowerPolicy, candidate: &AdapterCandidate) -> i32 {
    let memory_score = i32::try_from((candidate.memory_mib / 256).min(48)).unwrap_or(48);
    let feature_score = i32::from(candidate.feature_score.min(100));
    let stability_score = i32::from(candidate.stability_score.min(100)) * 2;
    let benchmark_score = i32::from(candidate.benchmark_score.unwrap_or(0).min(300));
    let api_score = backend_tie_break(os, candidate.backend) * 12;
    let class_score = match (policy, candidate.device_class) {
        (PowerPolicy::Performance, DeviceClass::Discrete) => 100,
        (PowerPolicy::Efficiency, DeviceClass::Integrated) => 80,
        (PowerPolicy::Efficiency, DeviceClass::Discrete) => -30,
        (_, DeviceClass::Discrete) => 65,
        (_, DeviceClass::Integrated) => 45,
        (_, DeviceClass::Unknown) => 10,
        (_, DeviceClass::Software) => -200,
    };

    stability_score + feature_score + memory_score + benchmark_score + api_score + class_score
}

const fn backend_tie_break(os: OperatingSystem, backend: RendererBackend) -> i32 {
    match (os, backend) {
        (OperatingSystem::Windows, RendererBackend::Direct3D12)
        | (OperatingSystem::MacOs, RendererBackend::Metal)
        | (OperatingSystem::Linux, RendererBackend::Vulkan) => 5,
        (OperatingSystem::Windows, RendererBackend::Vulkan) => 4,
        (_, RendererBackend::OpenGl) => 2,
        (_, RendererBackend::Software) => 0,
        _ => 1,
    }
}

const fn classify_quality(
    score: i32,
    adapter_memory_mib: u32,
    system_memory_mib: u32,
) -> QualityProfile {
    if score >= 650 && adapter_memory_mib >= 10_240 && system_memory_mib >= 16_384 {
        QualityProfile::Ultra
    } else if score >= 500 && adapter_memory_mib >= 6_144 && system_memory_mib >= 12_288 {
        QualityProfile::High
    } else if score >= 350 && adapter_memory_mib >= 2_048 && system_memory_mib >= 8_192 {
        QualityProfile::Balanced
    } else if score >= 180 && system_memory_mib >= 4_096 {
        QualityProfile::Low
    } else {
        QualityProfile::Compatibility
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(backend: RendererBackend, device_class: DeviceClass) -> AdapterCandidate {
        AdapterCandidate {
            backend,
            device_class,
            memory_mib: 8_192,
            can_present: true,
            supports_baseline: true,
            feature_score: 80,
            stability_score: 90,
            benchmark_score: Some(180),
        }
    }

    #[test]
    fn windows_prefers_vulkan_on_discrete_gpus() {
        let selected = select_renderer(
            OperatingSystem::Windows,
            PowerPolicy::Performance,
            16_384,
            &[
                candidate(RendererBackend::Vulkan, DeviceClass::Discrete),
                candidate(RendererBackend::Direct3D12, DeviceClass::Discrete),
            ],
        )
        .expect("a valid adapter");

        assert_eq!(selected.backend, RendererBackend::Vulkan);
        assert_eq!(selected.quality, QualityProfile::High);
    }

    #[test]
    fn windows_integrated_graphics_keep_d3d12_preference() {
        let selected = select_renderer(
            OperatingSystem::Windows,
            PowerPolicy::Performance,
            16_384,
            &[
                candidate(RendererBackend::Vulkan, DeviceClass::Integrated),
                candidate(RendererBackend::Direct3D12, DeviceClass::Integrated),
            ],
        )
        .expect("integrated adapter");
        assert_eq!(selected.backend, RendererBackend::Direct3D12);
    }

    #[test]
    fn windows_hybrid_gpu_uses_discrete_vulkan_and_falls_back_if_invalid() {
        let mut discrete = candidate(RendererBackend::Vulkan, DeviceClass::Discrete);
        let integrated = candidate(RendererBackend::Direct3D12, DeviceClass::Integrated);
        for supported in [true, false] {
            discrete.supports_baseline = supported;
            let selected = select_renderer(
                OperatingSystem::Windows,
                PowerPolicy::Efficiency,
                16_384,
                &[integrated.clone(), discrete.clone()],
            )
            .expect("working adapter");
            assert_eq!(
                selected.backend,
                if supported {
                    RendererBackend::Vulkan
                } else {
                    RendererBackend::Direct3D12
                }
            );
        }
    }

    #[test]
    fn efficiency_policy_can_prefer_integrated_adapter() {
        let mut integrated = candidate(RendererBackend::Direct3D12, DeviceClass::Integrated);
        integrated.memory_mib = 4_096;
        let selected = select_renderer(
            OperatingSystem::Windows,
            PowerPolicy::Efficiency,
            16_384,
            &[
                candidate(RendererBackend::Direct3D12, DeviceClass::Discrete),
                integrated,
            ],
        )
        .expect("a valid adapter");

        assert_eq!(selected.device_class, DeviceClass::Integrated);
    }

    #[test]
    fn invalid_adapters_are_rejected() {
        let mut invalid = candidate(RendererBackend::Vulkan, DeviceClass::Discrete);
        invalid.can_present = false;
        assert_eq!(
            select_renderer(
                OperatingSystem::Linux,
                PowerPolicy::Balanced,
                8_192,
                &[invalid]
            ),
            None
        );
    }

    #[test]
    fn software_path_remains_available_as_last_resort() {
        let mut software = candidate(RendererBackend::Software, DeviceClass::Software);
        software.memory_mib = 512;
        software.feature_score = 0;
        software.benchmark_score = None;
        let selected = select_renderer(
            OperatingSystem::Other,
            PowerPolicy::Balanced,
            4_096,
            &[software],
        )
        .expect("software adapter is valid");

        assert_eq!(selected.quality, QualityProfile::Compatibility);
    }
}
