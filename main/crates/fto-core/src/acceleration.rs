//! Per-operation GPU policy. CPU-only callers and builds keep the original generators.
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[cfg(feature = "gpu")]
mod backend;
#[cfg(feature = "gpu")]
pub(crate) use backend::{BloomCompute, Gpu, PruningCompute};

/// Small, bounded calibration using the production pruning compute kernel.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuBenchmark {
    pub eligible: bool,
    pub adapter: String,
    pub cpu_nodes_per_second: f64,
    pub gpu_nodes_per_second: f64,
    pub cpu_workers: usize,
    pub message: String,
}
impl GpuBenchmark {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            eligible: false,
            adapter: String::new(),
            cpu_nodes_per_second: 0.,
            gpu_nodes_per_second: 0.,
            cpu_workers: 0,
            message: message.into(),
        }
    }
}
/// Fingerprint changes when the processor, compute adapters, or drivers change.
#[cfg(not(target_os = "emscripten"))]
pub fn gpu_hardware_fingerprint() -> String {
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    let cpu = system.cpus().first().map_or("unknown", |cpu| cpu.brand());
    let mut identity = format!(
        "{}:{}:{cpu}:{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        system.cpus().len()
    );
    #[cfg(feature = "gpu")]
    {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let mut adapters: Vec<_> = instance
            .enumerate_adapters(wgpu::Backends::all())
            .into_iter()
            .map(|adapter| format!("{:?}", adapter.get_info()))
            .collect();
        adapters.sort();
        identity.push_str(&adapters.join("|"));
    }
    identity
}
pub fn benchmark_gpu() -> GpuBenchmark {
    #[cfg(feature = "gpu")]
    {
        with_policy(Policy::Gpu, None, backend::benchmark_pruning)
    }
    #[cfg(not(feature = "gpu"))]
    {
        GpuBenchmark::unavailable("This build does not include GPU acceleration.")
    }
}

pub type GenerationLog = dyn Fn(&str) + Send + Sync;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Policy {
    Cpu,
    Auto,
    /// Explicit compute path for diagnostics; hardware/error fallback still applies.
    Gpu,
}
pub struct GenerationControl {
    enabled: AtomicBool,
}
impl GenerationControl {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
        }
    }
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}
struct Session {
    control: Arc<GenerationControl>,
    live: bool,
    #[cfg(feature = "gpu")]
    policy: Policy,
    log: Option<Arc<GenerationLog>>,
    #[cfg(feature = "gpu")]
    gpu: std::sync::OnceLock<Result<Arc<Gpu>, String>>,
}
thread_local! { static SESSION: RefCell<Option<Arc<Session>>> = const { RefCell::new(None) }; }

/// Applies only to this synchronous operation; nested scopes and panics restore
/// the previous policy. Generator workers do not change process-global settings.
pub fn with_policy<T>(
    policy: Policy,
    log: Option<Arc<GenerationLog>>,
    action: impl FnOnce() -> T,
) -> T {
    scoped_policy(
        policy,
        Arc::new(GenerationControl::new(policy != Policy::Cpu)),
        false,
        log,
        action,
    )
}

pub fn with_control<T>(
    policy: Policy,
    control: Arc<GenerationControl>,
    log: Option<Arc<GenerationLog>>,
    action: impl FnOnce() -> T,
) -> T {
    scoped_policy(policy, control, true, log, action)
}
fn scoped_policy<T>(
    policy: Policy,
    control: Arc<GenerationControl>,
    live: bool,
    log: Option<Arc<GenerationLog>>,
    action: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Arc<Session>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SESSION.with(|s| *s.borrow_mut() = self.0.take());
        }
    }
    let next = Some(Arc::new(Session {
        control,
        live,
        #[cfg(feature = "gpu")]
        policy,
        log,
        #[cfg(feature = "gpu")]
        gpu: std::sync::OnceLock::new(),
    }));
    let _restore = Restore(SESSION.with(|s| s.replace(next)));
    #[cfg(not(feature = "gpu"))]
    if policy != Policy::Cpu {
        self::log("GPU acceleration is not compiled in; using CPU");
    }
    action()
}
pub(crate) fn log(message: &str) {
    SESSION.with(|s| {
        if let Some(session) = s.borrow().as_ref() {
            if let Some(log) = &session.log {
                log(message);
            }
        }
    });
}
#[cfg(feature = "gpu")]
pub(crate) fn forced() -> bool {
    SESSION.with(|s| {
        s.borrow()
            .as_ref()
            .is_some_and(|s| s.policy == Policy::Gpu && s.control.enabled())
    })
}
#[cfg(feature = "gpu")]
pub(crate) fn enabled() -> bool {
    SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.control.enabled()))
}
#[cfg(feature = "gpu")]
pub(crate) fn gpu() -> Option<Arc<Gpu>> {
    let session = SESSION.with(|s| s.borrow().clone())?;
    if !session.control.enabled() {
        return None;
    }
    session
        .gpu
        .get_or_init(|| {
            let result = Gpu::new().map(Arc::new);
            match &result {
                Ok(gpu) => log(&format!(
                    "GPU generation: {} ({:?}); adaptive batches, GPU working budget {}",
                    gpu.name,
                    gpu.kind,
                    crate::format_bytes(gpu.budget)
                )),
                Err(error) => log(&format!("GPU unavailable; using CPU: {error}")),
            }
            result
        })
        .as_ref()
        .ok()
        .cloned()
}

pub(crate) fn live() -> bool {
    SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.live))
}
pub(crate) fn fallback(reason: &str) {
    SESSION.with(|s| {
        if let Some(session) = s.borrow().as_ref() {
            session.control.set_enabled(false);
        }
    });
    log(&format!(
        "generation backend: CPU; {reason}; continuing without losing progress"
    ));
}
pub(crate) struct GenerationStage;
impl Drop for GenerationStage {
    fn drop(&mut self) {
        log("generation stage: finished");
    }
}
pub(crate) fn stage(name: &str) -> GenerationStage {
    log(&format!("generation stage: CPU; {name}"));
    #[cfg(not(feature = "gpu"))]
    if SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.control.enabled())) {
        fallback("GPU acceleration is not compiled in");
    }
    GenerationStage
}

#[cfg(not(target_os = "emscripten"))]
pub fn available_memory_bytes() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.available_memory()
}
/// Checked against current available memory, not an arbitrary table-size ceiling.
pub(crate) fn check_host_allocation(bytes: u64) -> Result<(), String> {
    if bytes > isize::MAX as u64 {
        return Err("table size exceeds this platform's address space".into());
    }
    let available = available_memory_bytes();
    let reserve = (available / 5).max(512 * 1024 * 1024);
    if available > 0 && bytes > available.saturating_sub(reserve) {
        return Err(format!(
            "not enough available RAM for {}; {} is currently available (including memory reserved for the app and system)",
            crate::format_bytes(bytes),
            crate::format_bytes(available)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "runs the first-launch hardware probe on this machine"]
    fn startup_benchmark_reports_valid_results_or_disables_gpu() {
        let result = benchmark_gpu();
        println!("startup GPU comparison: {result:?}");
        assert!(result.cpu_nodes_per_second.is_finite());
        assert!(result.gpu_nodes_per_second.is_finite());
        if result.eligible {
            assert!(result.gpu_nodes_per_second > result.cpu_nodes_per_second);
        }
    }
    #[test]
    #[cfg(feature = "gpu")]
    fn live_cpu_policy_can_be_changed_from_another_thread_and_reject_gpu() {
        let control = Arc::new(GenerationControl::new(false));
        let messages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let saved = messages.clone();
        let log: Arc<GenerationLog> =
            Arc::new(move |message| saved.lock().unwrap().push(message.to_owned()));
        with_control(Policy::Auto, control.clone(), Some(log), || {
            assert!(!enabled());
            let other = control.clone();
            std::thread::spawn(move || other.set_enabled(true))
                .join()
                .unwrap();
            assert!(enabled());
            assert!(live());
            fallback("GPU is slower than CPU in the timing comparison");
            assert!(!enabled());
        });
        assert!(!enabled());
        assert!(
            messages
                .lock()
                .unwrap()
                .iter()
                .any(|m| m.contains("slower") && m.contains("without losing progress"))
        );
    }
    #[cfg(feature = "gpu")]
    #[test]
    fn policy_scope_restores_after_nested_cpu_scope_and_panic() {
        assert!(!enabled());
        with_policy(Policy::Gpu, None, || {
            assert!(enabled());
            assert!(forced());
            with_policy(Policy::Cpu, None, || assert!(!enabled()));
            assert!(forced());
        });
        assert!(!enabled());
        let result =
            std::panic::catch_unwind(|| with_policy(Policy::Auto, None, || panic!("test unwind")));
        assert!(result.is_err());
        assert!(!enabled());
    }
}

#[cfg(target_os = "emscripten")]
pub fn gpu_hardware_fingerprint() -> String { "browser-cpu".into() }

// Browsers do not expose available host RAM. Allocation checks still enforce
// the address-space bound; fallible allocations report exhaustion.
#[cfg(target_os = "emscripten")]
pub fn available_memory_bytes() -> u64 { 0 }
