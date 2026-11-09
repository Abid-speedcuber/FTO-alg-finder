//! Per-operation GPU policy. CPU-only callers and builds keep the original generators.
use std::{cell::RefCell, sync::Arc};
#[cfg(feature = "gpu")]
mod backend;
#[cfg(feature = "gpu")]
pub(crate) use backend::{BloomCompute, Gpu};

pub type GenerationLog = dyn Fn(&str) + Send + Sync;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Policy {
    Cpu,
    Auto,
    /// Explicit compute path for diagnostics; hardware/error fallback still applies.
    Gpu,
}
struct Session {
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
    struct Restore(Option<Arc<Session>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SESSION.with(|s| *s.borrow_mut() = self.0.take());
        }
    }
    let next = (policy != Policy::Cpu).then(|| {
        Arc::new(Session {
            #[cfg(feature = "gpu")]
            policy,
            log,
            #[cfg(feature = "gpu")]
            gpu: std::sync::OnceLock::new(),
        })
    });
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
    SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.policy == Policy::Gpu))
}
#[cfg(feature = "gpu")]
pub(crate) fn enabled() -> bool {
    SESSION.with(|s| s.borrow().is_some())
}
#[cfg(feature = "gpu")]
pub(crate) fn gpu() -> Option<Arc<Gpu>> {
    let session = SESSION.with(|s| s.borrow().clone())?;
    session
        .gpu
        .get_or_init(|| {
            let result = Gpu::new().map(Arc::new);
            match &result {
                Ok(gpu) => log(&format!(
                    "GPU generation: {} ({:?}); adaptive batches, GPU working budget {:.0} MiB",
                    gpu.name,
                    gpu.kind,
                    gpu.budget as f64 / 1048576.
                )),
                Err(error) => log(&format!("GPU unavailable; using CPU: {error}")),
            }
            result
        })
        .as_ref()
        .ok()
        .cloned()
}

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
            "not enough available RAM for {:.0} MiB; {:.0} MiB is currently available (including memory reserved for the app and system)",
            bytes as f64 / 1048576.,
            available as f64 / 1048576.
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
