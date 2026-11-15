use super::*;
use fto_core::acceleration::{self, GenerationControl, GenerationLog, Policy};

pub(super) type GenerationSlot = Arc<Mutex<Option<Arc<ActiveGeneration>>>>;
pub(super) struct ActiveGeneration {
    control: Arc<GenerationControl>,
    status: Mutex<GenerationStatus>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerationStatus {
    active: bool,
    gpu_enabled: bool,
    backend: &'static str,
    message: String,
}
fn publish(app: &AppHandle, session: &ActiveGeneration, message: &str) {
    let Ok(mut state) = session.status.lock() else {
        return;
    };
    state.gpu_enabled = session.control.enabled();
    if message == "generation stage: finished" {
        state.active = false;
        state.message.clear();
    } else if message.starts_with("generation stage:") {
        state.active = true;
        state.backend = "cpu";
        state.message.clear();
    } else if message.starts_with("generation backend: GPU;") {
        state.backend = "gpu";
        state.message.clear();
    } else if let Some(reason) = message.strip_prefix("generation backend: CPU;") {
        state.backend = "cpu";
        state.message = reason.trim().to_owned();
    }
    let _ = app.emit("generation-status", state.clone());
}
pub(super) fn with_gpu_generation<T>(
    enabled: bool,
    app: &AppHandle,
    slot: &GenerationSlot,
    action: impl FnOnce() -> T,
) -> T {
    let control = Arc::new(GenerationControl::new(enabled));
    let session = Arc::new(ActiveGeneration {
        control: control.clone(),
        status: Mutex::new(GenerationStatus {
            active: false,
            gpu_enabled: enabled,
            backend: "cpu",
            message: String::new(),
        }),
    });
    if let Ok(mut active) = slot.lock() {
        *active = Some(session.clone());
    }
    struct Cleanup<'a> {
        slot: &'a GenerationSlot,
        app: &'a AppHandle,
        session: Arc<ActiveGeneration>,
        cancel: Option<Arc<AtomicBool>>,
    }
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            if let Ok(mut active) = self.slot.lock() {
                if active
                    .as_ref()
                    .is_some_and(|s| Arc::ptr_eq(s, &self.session))
                {
                    publish(self.app, &self.session, "generation stage: finished");
                    *active = None;
                }
            }
            let state = self.app.state::<SolverState>();
            if let Ok(mut current) = state.current_cancel.lock() {
                if current
                    .as_ref()
                    .zip(self.cancel.as_ref())
                    .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
                {
                    *current = None;
                }
            }
            if std::thread::panicking() {
                let _ = self.app.emit(
                    "solve-error",
                    "The worker stopped unexpectedly. You can start another solve.",
                );
            }
        }
    }
    let _cleanup = Cleanup {
        slot,
        app,
        session: session.clone(),
        cancel: app
            .state::<SolverState>()
            .current_cancel
            .lock()
            .ok()
            .and_then(|s| s.clone()),
    };
    let progress_app = app.clone();
    let log: Arc<GenerationLog> = Arc::new(move |message| {
        if message.starts_with("generation ") {
            publish(&progress_app, &session, message);
        }
        if !message.starts_with("generation stage:") {
            emit_line(&progress_app, "acceleration", message);
        }
    });
    acceleration::with_control(
        if enabled { Policy::Auto } else { Policy::Cpu },
        control,
        Some(log),
        action,
    )
}

#[tauri::command]
pub(super) fn set_generation_gpu(
    enabled: bool,
    state: tauri::State<'_, SolverState>,
    app: AppHandle,
) -> Result<bool, String> {
    let session = state
        .generation
        .lock()
        .map_err(|_| "generation state lock poisoned")?
        .clone();
    let Some(session) = session else {
        return Ok(false);
    };
    session.control.set_enabled(enabled);
    let mut status = session
        .status
        .lock()
        .map_err(|_| "generation status lock poisoned")?;
    status.gpu_enabled = enabled;
    status.message = if enabled {
        "GPU requested. Running a short CPU/GPU speed comparison; completed work is retained."
    } else {
        "CPU requested. Finishing the current bounded batch; completed work is retained."
    }
    .into();
    app.emit("generation-status", status.clone())
        .map_err(|e| e.to_string())?;
    Ok(true)
}

pub(super) fn snapshot(slot: &GenerationSlot) -> Option<GenerationStatus> {
    let session = slot.lock().ok()?.clone()?;
    let status = session.status.lock().ok()?.clone();
    Some(status)
}
