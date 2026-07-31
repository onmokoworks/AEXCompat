use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum GpuRuntimeBackendKind {
    #[serde(rename = "apple-opencl")]
    AppleOpenCl,
    #[serde(rename = "wgpu-metal")]
    WgpuMetal,
}

impl GpuRuntimeBackendKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::AppleOpenCl => "apple-opencl",
            Self::WgpuMetal => "wgpu-metal",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GpuSuiteEvidence {
    pub allocations_created: u64,
    pub allocations_freed: u64,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub worlds_created: u64,
    pub worlds_disposed: u64,
    pub invalid_operations: u64,
    pub exclusive_access_depth: u32,
    pub live_host_allocations: usize,
    pub live_gpu_worlds: usize,
    pub live_borrowed_gpu_worlds: usize,
    pub live_device_allocations: usize,
    pub live_bytes: usize,
    pub transport_active: bool,
    pub cleanup_balanced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OpenClErrorEvidence {
    pub operation: String,
    pub status: i32,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OpenClBridgeEvidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_units: Option<u32>,
    pub api_calls: BTreeMap<String, u64>,
    pub source_strings: u64,
    pub source_bytes: u64,
    pub programs_built: u64,
    pub kernels_created: u64,
    pub kernels_released: u64,
    pub scalar_arguments: u64,
    pub scalar_argument_bytes: u64,
    pub buffer_arguments: u64,
    pub kernel_dispatches: u64,
    pub dispatched_work_items: u64,
    pub errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<OpenClErrorEvidence>,
    pub live_buffers: usize,
    pub live_programs: usize,
    pub live_kernels: usize,
    pub native_contexts: usize,
    pub native_command_queues: usize,
    pub native_buffers: usize,
    pub native_programs: usize,
    pub native_kernels: usize,
    pub native_release_errors: usize,
    pub cleanup_balanced: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WgpuResourceCounts {
    pub buffers: usize,
    pub staging_buffers: usize,
    pub shader_modules: usize,
    pub bind_group_layouts: usize,
    pub pipeline_layouts: usize,
    pub pipelines: usize,
    pub bind_groups: usize,
    pub command_buffers: usize,
}

impl WgpuResourceCounts {
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WgpuRuntimeEvidence {
    pub executor_available: bool,
    pub backend_operations_attempted: u64,
    pub created_resources: WgpuResourceCounts,
    pub live_resources: WgpuResourceCounts,
    pub cleanup_balanced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RenderBackendRequest {
    #[default]
    Cpu,
    OpenCl {
        device_index: u32,
    },
    WgpuMetal {
        device_index: u32,
    },
}

impl RenderBackendRequest {
    pub fn backend_name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::OpenCl { .. } => "opencl",
            Self::WgpuMetal { .. } => "wgpu-metal",
        }
    }

    pub fn is_gpu(self) -> bool {
        matches!(self, Self::OpenCl { .. } | Self::WgpuMetal { .. })
    }

    pub fn plugin_framework(self) -> Option<&'static str> {
        match self {
            Self::Cpu => None,
            Self::OpenCl { .. } | Self::WgpuMetal { .. } => Some("opencl"),
        }
    }

    pub fn runtime_backend(self) -> Option<GpuRuntimeBackendKind> {
        match self {
            Self::Cpu => None,
            Self::OpenCl { .. } => Some(GpuRuntimeBackendKind::AppleOpenCl),
            Self::WgpuMetal { .. } => Some(GpuRuntimeBackendKind::WgpuMetal),
        }
    }

    pub fn device_index(self) -> Option<u32> {
        match self {
            Self::Cpu => None,
            Self::OpenCl { device_index } | Self::WgpuMetal { device_index } => Some(device_index),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GpuSelectorDiagnostic {
    pub selector: &'static str,
    pub attempted: bool,
    pub completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<i32>,
}

impl GpuSelectorDiagnostic {
    fn pending(selector: &'static str) -> Self {
        Self {
            selector,
            attempted: false,
            completed: false,
            error: None,
        }
    }

    fn begin(&mut self) {
        self.attempted = true;
    }

    fn finish(&mut self, error: i32) {
        self.completed = true;
        self.error = Some(error);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GpuRenderDiagnostic {
    pub requested_backend: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_backend: Option<GpuRuntimeBackendKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_framework: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_index: Option<u32>,
    pub setup: GpuSelectorDiagnostic,
    pub pre_render: GpuSelectorDiagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_render_output_flags: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_render_possible: Option<bool>,
    pub render: GpuSelectorDiagnostic,
    pub setdown: GpuSelectorDiagnostic,
    pub runtime_started: bool,
    pub transport_prepared: bool,
    pub transport_finished: bool,
    pub runtime_ended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_begin_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_prepare_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_cleanup_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_end_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_suite: Option<GpuSuiteEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opencl: Option<OpenClBridgeEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wgpu: Option<WgpuRuntimeEvidence>,
    pub cleanup_complete: bool,
}

impl GpuRenderDiagnostic {
    pub fn pending(request: RenderBackendRequest) -> Self {
        let device_index = request.device_index();
        let plugin_framework = request.plugin_framework();
        Self {
            requested_backend: request.backend_name(),
            runtime_backend: None,
            framework: plugin_framework,
            plugin_framework,
            device_index,
            setup: GpuSelectorDiagnostic::pending("GPU_DEVICE_SETUP"),
            pre_render: GpuSelectorDiagnostic::pending("SMART_PRE_RENDER"),
            pre_render_output_flags: None,
            gpu_render_possible: None,
            render: GpuSelectorDiagnostic::pending(if request.is_gpu() {
                "SMART_RENDER_GPU"
            } else {
                "SMART_RENDER"
            }),
            setdown: GpuSelectorDiagnostic::pending("GPU_DEVICE_SETDOWN"),
            runtime_started: false,
            transport_prepared: false,
            transport_finished: false,
            runtime_ended: false,
            runtime_begin_error: None,
            transport_prepare_error: None,
            transport_cleanup_error: None,
            runtime_end_error: None,
            device_suite: None,
            opencl: None,
            wgpu: None,
            cleanup_complete: !request.is_gpu(),
        }
    }

    pub fn classic_cpu(render_error: i32) -> Self {
        let mut diagnostic = Self::pending(RenderBackendRequest::Cpu);
        diagnostic.pre_render = GpuSelectorDiagnostic::pending("SMART_PRE_RENDER");
        diagnostic.render = GpuSelectorDiagnostic::pending("RENDER");
        diagnostic.render.begin();
        diagnostic.render.finish(render_error);
        diagnostic
    }

    pub(crate) fn finish_runtime_evidence(
        &mut self,
        device_suite: GpuSuiteEvidence,
        opencl: OpenClBridgeEvidence,
        wgpu: Option<WgpuRuntimeEvidence>,
    ) {
        let plugin_cleanup_complete = match self.setup.error {
            Some(0) => self.setdown.completed && self.setdown.error == Some(0),
            Some(_) => true,
            None => false,
        };
        let wgpu_cleanup_complete = wgpu
            .as_ref()
            .is_none_or(|evidence| evidence.cleanup_balanced && evidence.live_resources.is_zero());
        self.cleanup_complete = plugin_cleanup_complete
            && self.runtime_ended
            && (!self.transport_prepared || self.transport_finished)
            && self.transport_prepare_error.is_none()
            && self.transport_cleanup_error.is_none()
            && self.runtime_end_error.is_none()
            && !device_suite.transport_active
            && device_suite.live_host_allocations == 0
            && device_suite.live_gpu_worlds == 0
            && device_suite.live_borrowed_gpu_worlds == 0
            && device_suite.live_device_allocations == 0
            && device_suite.live_bytes == 0
            && device_suite.cleanup_balanced
            && opencl.live_buffers == 0
            && opencl.live_programs == 0
            && opencl.live_kernels == 0
            && opencl.cleanup_balanced
            && wgpu_cleanup_complete;
        self.device_suite = Some(device_suite);
        self.opencl = Some(opencl);
        self.wgpu = wgpu;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GpuContext {
    pub framework: i32,
    pub device_index: u32,
    pub gpu_data: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleCall {
    Setup { framework: i32, device_index: u32 },
    PreRender { context: Option<GpuContext> },
    RenderCpu,
    RenderGpu { context: GpuContext },
    Setdown { context: GpuContext },
}

pub(crate) enum LifecycleReply<T, E> {
    Setup {
        error: i32,
        gpu_data: Result<u64, E>,
    },
    PreRender {
        error: i32,
        output_flags: u16,
    },
    Render {
        error: i32,
        output: T,
    },
    Setdown {
        error: i32,
    },
}

#[derive(Debug)]
pub(crate) enum LifecycleFailure<E> {
    Dispatch { selector: &'static str, source: E },
    Selector { selector: &'static str, error: i32 },
    GpuRenderNotPossible,
    InternalContract(&'static str),
}

pub(crate) struct LifecycleExecution<T, E> {
    pub result: Result<T, LifecycleFailure<E>>,
    pub diagnostic: GpuRenderDiagnostic,
}

pub(crate) fn run_smart_lifecycle<T, E>(
    request: RenderBackendRequest,
    opencl_framework: i32,
    gpu_render_possible_flag: u16,
    mut dispatch: impl FnMut(LifecycleCall) -> Result<LifecycleReply<T, E>, E>,
) -> LifecycleExecution<T, E> {
    let mut diagnostic = GpuRenderDiagnostic::pending(request);
    let context = match request {
        RenderBackendRequest::Cpu => None,
        RenderBackendRequest::OpenCl { device_index }
        | RenderBackendRequest::WgpuMetal { device_index } => {
            diagnostic.setup.begin();
            let setup = match dispatch(LifecycleCall::Setup {
                framework: opencl_framework,
                device_index,
            }) {
                Ok(LifecycleReply::Setup { error, gpu_data }) => {
                    diagnostic.setup.finish(error);
                    if error != 0 {
                        return LifecycleExecution {
                            result: Err(LifecycleFailure::Selector {
                                selector: "GPU_DEVICE_SETUP",
                                error,
                            }),
                            diagnostic,
                        };
                    }
                    match gpu_data {
                        Ok(gpu_data) => Ok(GpuContext {
                            framework: opencl_framework,
                            device_index,
                            gpu_data,
                        }),
                        Err(source) => Err(source),
                    }
                }
                Ok(_) => {
                    return LifecycleExecution {
                        result: Err(LifecycleFailure::InternalContract(
                            "GPU_DEVICE_SETUP returned the wrong reply",
                        )),
                        diagnostic,
                    };
                }
                Err(source) => {
                    return LifecycleExecution {
                        result: Err(LifecycleFailure::Dispatch {
                            selector: "GPU_DEVICE_SETUP",
                            source,
                        }),
                        diagnostic,
                    };
                }
            };
            match setup {
                Ok(context) => Some(context),
                Err(source) => {
                    // The selector itself succeeded, so cleanup is still
                    // mandatory even when reading its output fails. A zero
                    // gpu_data value is the only bounded value available.
                    let context = GpuContext {
                        framework: opencl_framework,
                        device_index,
                        gpu_data: 0,
                    };
                    let primary = LifecycleFailure::Dispatch {
                        selector: "GPU_DEVICE_SETUP_OUTPUT",
                        source,
                    };
                    return finish_setdown(&mut dispatch, context, primary, diagnostic);
                }
            }
        }
    };

    let body = run_pre_render_and_render(
        &mut dispatch,
        context,
        gpu_render_possible_flag,
        &mut diagnostic,
    );
    if let Some(context) = context {
        finish_setdown_result(&mut dispatch, context, body, diagnostic)
    } else {
        LifecycleExecution {
            result: body,
            diagnostic,
        }
    }
}

fn run_pre_render_and_render<T, E>(
    dispatch: &mut impl FnMut(LifecycleCall) -> Result<LifecycleReply<T, E>, E>,
    context: Option<GpuContext>,
    gpu_render_possible_flag: u16,
    diagnostic: &mut GpuRenderDiagnostic,
) -> Result<T, LifecycleFailure<E>> {
    diagnostic.pre_render.begin();
    let output_flags = match dispatch(LifecycleCall::PreRender { context }) {
        Ok(LifecycleReply::PreRender {
            error,
            output_flags,
        }) => {
            diagnostic.pre_render.finish(error);
            diagnostic.pre_render_output_flags = Some(output_flags);
            if error != 0 {
                return Err(LifecycleFailure::Selector {
                    selector: "SMART_PRE_RENDER",
                    error,
                });
            }
            output_flags
        }
        Ok(_) => {
            return Err(LifecycleFailure::InternalContract(
                "SMART_PRE_RENDER returned the wrong reply",
            ));
        }
        Err(source) => {
            return Err(LifecycleFailure::Dispatch {
                selector: "SMART_PRE_RENDER",
                source,
            });
        }
    };

    let gpu_render_possible = output_flags & gpu_render_possible_flag != 0;
    diagnostic.gpu_render_possible = Some(gpu_render_possible);
    let call = match context {
        Some(context) if gpu_render_possible => LifecycleCall::RenderGpu { context },
        Some(_) => return Err(LifecycleFailure::GpuRenderNotPossible),
        None => LifecycleCall::RenderCpu,
    };
    diagnostic.render.begin();
    match dispatch(call) {
        Ok(LifecycleReply::Render { error, output }) => {
            diagnostic.render.finish(error);
            if error == 0 {
                Ok(output)
            } else {
                Err(LifecycleFailure::Selector {
                    selector: diagnostic.render.selector,
                    error,
                })
            }
        }
        Ok(_) => Err(LifecycleFailure::InternalContract(
            "SMART_RENDER returned the wrong reply",
        )),
        Err(source) => Err(LifecycleFailure::Dispatch {
            selector: diagnostic.render.selector,
            source,
        }),
    }
}

fn finish_setdown<T, E>(
    dispatch: &mut impl FnMut(LifecycleCall) -> Result<LifecycleReply<T, E>, E>,
    context: GpuContext,
    primary: LifecycleFailure<E>,
    diagnostic: GpuRenderDiagnostic,
) -> LifecycleExecution<T, E> {
    finish_setdown_result(dispatch, context, Err(primary), diagnostic)
}

fn finish_setdown_result<T, E>(
    dispatch: &mut impl FnMut(LifecycleCall) -> Result<LifecycleReply<T, E>, E>,
    context: GpuContext,
    primary: Result<T, LifecycleFailure<E>>,
    mut diagnostic: GpuRenderDiagnostic,
) -> LifecycleExecution<T, E> {
    diagnostic.setdown.begin();
    let cleanup = match dispatch(LifecycleCall::Setdown { context }) {
        Ok(LifecycleReply::Setdown { error }) => {
            diagnostic.setdown.finish(error);
            diagnostic.cleanup_complete = error == 0;
            (error != 0).then_some(LifecycleFailure::Selector {
                selector: "GPU_DEVICE_SETDOWN",
                error,
            })
        }
        Ok(_) => Some(LifecycleFailure::InternalContract(
            "GPU_DEVICE_SETDOWN returned the wrong reply",
        )),
        Err(source) => Some(LifecycleFailure::Dispatch {
            selector: "GPU_DEVICE_SETDOWN",
            source,
        }),
    };
    LifecycleExecution {
        result: match primary {
            Err(primary) => Err(primary),
            Ok(output) => cleanup.map_or(Ok(output), Err),
        },
        diagnostic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FixtureError {
        PreRenderDispatch,
    }

    #[test]
    fn cpu_default_never_touches_gpu_selectors() {
        let mut calls = Vec::new();
        let execution = run_smart_lifecycle(
            RenderBackendRequest::default(),
            1,
            2,
            |call| -> Result<LifecycleReply<&'static str, FixtureError>, FixtureError> {
                calls.push(call);
                Ok(match call {
                    LifecycleCall::PreRender { context: None } => LifecycleReply::PreRender {
                        error: 0,
                        output_flags: 2,
                    },
                    LifecycleCall::RenderCpu => LifecycleReply::Render {
                        error: 0,
                        output: "cpu",
                    },
                    _ => panic!("CPU lifecycle dispatched {call:?}"),
                })
            },
        );
        assert_eq!(execution.result.unwrap(), "cpu");
        assert_eq!(
            calls,
            [
                LifecycleCall::PreRender { context: None },
                LifecycleCall::RenderCpu
            ]
        );
        assert!(!execution.diagnostic.setup.attempted);
        assert!(!execution.diagnostic.setdown.attempted);
        assert_eq!(execution.diagnostic.requested_backend, "cpu");
        assert_eq!(execution.diagnostic.plugin_framework, None);
    }

    #[test]
    fn wgpu_metal_reports_the_opencl_plugin_framework() {
        let request = RenderBackendRequest::WgpuMetal { device_index: 5 };
        let mut diagnostic = GpuRenderDiagnostic::pending(request);

        assert_eq!(request.backend_name(), "wgpu-metal");
        assert!(request.is_gpu());
        assert_eq!(diagnostic.requested_backend, "wgpu-metal");
        assert_eq!(diagnostic.runtime_backend, None);
        assert_eq!(diagnostic.framework, Some("opencl"));
        assert_eq!(diagnostic.plugin_framework, Some("opencl"));
        assert_eq!(diagnostic.device_index, Some(5));
        assert!(!diagnostic.cleanup_complete);

        diagnostic.runtime_backend = request.runtime_backend();
        diagnostic.wgpu = Some(WgpuRuntimeEvidence::default());
        let json = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(json["requested_backend"], "wgpu-metal");
        assert_eq!(json["runtime_backend"], "wgpu-metal");
        assert_eq!(json["framework"], "opencl");
        assert_eq!(json["plugin_framework"], "opencl");
        assert!(json.get("wgpu").is_some());
    }

    #[test]
    fn wgpu_metal_preserves_the_opencl_selector_contract() {
        let context = GpuContext {
            framework: 1,
            device_index: 5,
            gpu_data: 0x5678,
        };
        let mut calls = Vec::new();
        let execution = run_smart_lifecycle(
            RenderBackendRequest::WgpuMetal { device_index: 5 },
            1,
            2,
            |call| -> Result<LifecycleReply<&'static str, FixtureError>, FixtureError> {
                calls.push(call);
                Ok(match call {
                    LifecycleCall::Setup {
                        framework: 1,
                        device_index: 5,
                    } => LifecycleReply::Setup {
                        error: 0,
                        gpu_data: Ok(0x5678),
                    },
                    LifecycleCall::PreRender {
                        context: Some(actual),
                    } if actual == context => LifecycleReply::PreRender {
                        error: 0,
                        output_flags: 2,
                    },
                    LifecycleCall::RenderGpu { context: actual } if actual == context => {
                        LifecycleReply::Render {
                            error: 0,
                            output: "wgpu",
                        }
                    }
                    LifecycleCall::Setdown { context: actual } if actual == context => {
                        LifecycleReply::Setdown { error: 0 }
                    }
                    _ => panic!("unexpected wgpu lifecycle call {call:?}"),
                })
            },
        );

        assert_eq!(execution.result.unwrap(), "wgpu");
        assert_eq!(
            calls,
            [
                LifecycleCall::Setup {
                    framework: 1,
                    device_index: 5,
                },
                LifecycleCall::PreRender {
                    context: Some(context),
                },
                LifecycleCall::RenderGpu { context },
                LifecycleCall::Setdown { context },
            ]
        );
        assert_eq!(execution.diagnostic.plugin_framework, Some("opencl"));
        assert!(execution.diagnostic.cleanup_complete);
    }

    #[test]
    fn opencl_success_records_setup_pre_render_gpu_render_setdown_order() {
        let context = GpuContext {
            framework: 1,
            device_index: 3,
            gpu_data: 0x1234,
        };
        let mut calls = Vec::new();
        let execution = run_smart_lifecycle(
            RenderBackendRequest::OpenCl { device_index: 3 },
            1,
            2,
            |call| -> Result<LifecycleReply<&'static str, FixtureError>, FixtureError> {
                calls.push(call);
                Ok(match call {
                    LifecycleCall::Setup {
                        framework: 1,
                        device_index: 3,
                    } => LifecycleReply::Setup {
                        error: 0,
                        gpu_data: Ok(0x1234),
                    },
                    LifecycleCall::PreRender {
                        context: Some(actual),
                    } if actual == context => LifecycleReply::PreRender {
                        error: 0,
                        output_flags: 2,
                    },
                    LifecycleCall::RenderGpu { context: actual } if actual == context => {
                        LifecycleReply::Render {
                            error: 0,
                            output: "gpu",
                        }
                    }
                    LifecycleCall::Setdown { context: actual } if actual == context => {
                        LifecycleReply::Setdown { error: 0 }
                    }
                    _ => panic!("unexpected lifecycle call {call:?}"),
                })
            },
        );
        assert_eq!(execution.result.unwrap(), "gpu");
        assert_eq!(
            calls,
            [
                LifecycleCall::Setup {
                    framework: 1,
                    device_index: 3
                },
                LifecycleCall::PreRender {
                    context: Some(context)
                },
                LifecycleCall::RenderGpu { context },
                LifecycleCall::Setdown { context }
            ]
        );
        assert_eq!(execution.diagnostic.gpu_render_possible, Some(true));
        assert!(execution.diagnostic.cleanup_complete);
    }

    #[test]
    fn successful_setup_is_set_down_after_pre_render_dispatch_failure() {
        let mut calls = Vec::new();
        let execution = run_smart_lifecycle(
            RenderBackendRequest::OpenCl { device_index: 0 },
            1,
            2,
            |call| -> Result<LifecycleReply<(), FixtureError>, FixtureError> {
                calls.push(call);
                match call {
                    LifecycleCall::Setup { .. } => Ok(LifecycleReply::Setup {
                        error: 0,
                        gpu_data: Ok(7),
                    }),
                    LifecycleCall::PreRender { .. } => Err(FixtureError::PreRenderDispatch),
                    LifecycleCall::Setdown { .. } => Ok(LifecycleReply::Setdown { error: 0 }),
                    _ => panic!("render must not run after pre-render failure"),
                }
            },
        );
        assert!(matches!(
            execution.result,
            Err(LifecycleFailure::Dispatch {
                selector: "SMART_PRE_RENDER",
                source: FixtureError::PreRenderDispatch
            })
        ));
        assert!(matches!(
            calls.as_slice(),
            [
                LifecycleCall::Setup { .. },
                LifecycleCall::PreRender { .. },
                LifecycleCall::Setdown { .. }
            ]
        ));
        assert!(execution.diagnostic.cleanup_complete);
    }

    #[test]
    fn missing_gpu_render_flag_fails_closed_and_still_sets_down() {
        let mut calls = Vec::new();
        let execution = run_smart_lifecycle(
            RenderBackendRequest::OpenCl { device_index: 0 },
            1,
            2,
            |call| -> Result<LifecycleReply<(), FixtureError>, FixtureError> {
                calls.push(call);
                Ok(match call {
                    LifecycleCall::Setup { .. } => LifecycleReply::Setup {
                        error: 0,
                        gpu_data: Ok(7),
                    },
                    LifecycleCall::PreRender { .. } => LifecycleReply::PreRender {
                        error: 0,
                        output_flags: 0,
                    },
                    LifecycleCall::Setdown { .. } => LifecycleReply::Setdown { error: 0 },
                    _ => panic!("GPU render must not run without negotiation"),
                })
            },
        );
        assert!(matches!(
            execution.result,
            Err(LifecycleFailure::GpuRenderNotPossible)
        ));
        assert_eq!(execution.diagnostic.gpu_render_possible, Some(false));
        assert!(matches!(
            calls.as_slice(),
            [
                LifecycleCall::Setup { .. },
                LifecycleCall::PreRender { .. },
                LifecycleCall::Setdown { .. }
            ]
        ));
    }

    #[test]
    fn failed_setup_does_not_claim_or_run_setdown() {
        let mut calls = Vec::new();
        let execution = run_smart_lifecycle(
            RenderBackendRequest::OpenCl { device_index: 0 },
            1,
            2,
            |call| -> Result<LifecycleReply<(), FixtureError>, FixtureError> {
                calls.push(call);
                Ok(LifecycleReply::Setup {
                    error: 512,
                    gpu_data: Ok(0),
                })
            },
        );
        assert!(matches!(
            execution.result,
            Err(LifecycleFailure::Selector {
                selector: "GPU_DEVICE_SETUP",
                error: 512
            })
        ));
        assert_eq!(calls.len(), 1);
        assert!(!execution.diagnostic.setdown.attempted);
        assert!(!execution.diagnostic.cleanup_complete);
    }

    #[test]
    fn cleanup_error_is_reported_after_successful_gpu_render() {
        let execution = run_smart_lifecycle(
            RenderBackendRequest::OpenCl { device_index: 0 },
            1,
            2,
            |call| -> Result<LifecycleReply<(), FixtureError>, FixtureError> {
                Ok(match call {
                    LifecycleCall::Setup { .. } => LifecycleReply::Setup {
                        error: 0,
                        gpu_data: Ok(7),
                    },
                    LifecycleCall::PreRender { .. } => LifecycleReply::PreRender {
                        error: 0,
                        output_flags: 2,
                    },
                    LifecycleCall::RenderGpu { .. } => LifecycleReply::Render {
                        error: 0,
                        output: (),
                    },
                    LifecycleCall::Setdown { .. } => LifecycleReply::Setdown { error: 13 },
                    _ => panic!("unexpected CPU render"),
                })
            },
        );
        assert!(matches!(
            execution.result,
            Err(LifecycleFailure::Selector {
                selector: "GPU_DEVICE_SETDOWN",
                error: 13
            })
        ));
        assert_eq!(execution.diagnostic.setdown.error, Some(13));
        assert!(!execution.diagnostic.cleanup_complete);
    }
}
