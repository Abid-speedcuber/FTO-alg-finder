//! Startup calibration is shared by startup and UI requests and cached per hardware/driver.
use super::*;
use fto_core::acceleration::{self, GpuBenchmark};
const VERSION: u32 = 4;
#[derive(Clone, Serialize, Deserialize)]
struct StoredProfile {
    version: u32,
    fingerprint: String,
    benchmark: GpuBenchmark,
}
#[derive(Default)]
pub(super) struct GpuProfileState {
    gate: Mutex<()>,
    record: Mutex<Option<StoredProfile>>,
}
fn valid(record: &StoredProfile, fingerprint: &str) -> bool {
    record.version == VERSION
        && record.fingerprint == fingerprint
        && record.benchmark.cpu_nodes_per_second.is_finite()
        && record.benchmark.gpu_nodes_per_second.is_finite()
        && (!record.benchmark.eligible
            || (record.benchmark.cpu_nodes_per_second > 0.
                && record.benchmark.gpu_nodes_per_second
                    > record.benchmark.cpu_nodes_per_second))
}
fn profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("gpu-benchmark-v1.json"))
}
fn load_or_benchmark(
    path: &Path,
    fingerprint: &str,
    benchmark: impl FnOnce() -> GpuBenchmark,
) -> StoredProfile {
    let record = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoredProfile>(&bytes).ok())
        .filter(|record| valid(record, fingerprint))
        .unwrap_or_else(|| StoredProfile {
            version: VERSION,
            fingerprint: fingerprint.to_owned(),
            benchmark: benchmark(),
        });
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // A failed preference write does not stop the app; this process retains the result.
    if let Ok(bytes) = serde_json::to_vec(&record) {
        let _ = fs::write(&path, bytes);
    }
    record
}
fn calibrate(app: &AppHandle) -> Result<GpuBenchmark, String> {
    let state = app.state::<GpuProfileState>();
    let _gate = state.gate.lock().map_err(|_| "GPU profile lock poisoned")?;
    if let Some(record) = state
        .record
        .lock()
        .map_err(|_| "GPU profile lock poisoned")?
        .as_ref()
    {
        return Ok(record.benchmark.clone());
    }
    let fingerprint = std::panic::catch_unwind(acceleration::gpu_hardware_fingerprint)
        .unwrap_or_else(|_| {
            format!(
                "unavailable:{}:{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        });
    let path = profile_path(app)?;
    let record = load_or_benchmark(&path, &fingerprint, || {
        std::panic::catch_unwind(acceleration::benchmark_gpu)
            .unwrap_or_else(|_| GpuBenchmark::unavailable("GPU calibration failed; using CPU."))
    });
    let result = record.benchmark.clone();
    *state
        .record
        .lock()
        .map_err(|_| "GPU profile lock poisoned")? = Some(record);
    Ok(result)
}
#[cfg_attr(feature = "desktop", tauri::command)]
pub(super) async fn get_gpu_profile(app: AppHandle) -> Result<GpuBenchmark, String> {
    tauri::async_runtime::spawn_blocking(move || calibrate(&app))
        .await
        .map_err(|e| e.to_string())?
}
pub(super) fn allowed(app: &AppHandle) -> bool {
    app.state::<GpuProfileState>()
        .record
        .lock()
        .ok()
        .and_then(|record| record.as_ref().map(|record| record.benchmark.eligible))
        .unwrap_or(false)
}
pub(super) fn reset(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<GpuProfileState>();
    let _gate = state.gate.lock().map_err(|_| "GPU profile lock poisoned")?;
    let path = profile_path(app)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    *state
        .record
        .lock()
        .map_err(|_| "GPU profile lock poisoned")? = None;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_hardware_does_not_repeat_the_benchmark_and_changes_do() {
        let directory = std::env::temp_dir().join(format!(
            "fto-gpu-profile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("profile.json");
        let calls = std::cell::Cell::new(0);
        let generate = || {
            calls.set(calls.get() + 1);
            GpuBenchmark::unavailable("No compatible GPU")
        };
        load_or_benchmark(&path, "original", generate);
        load_or_benchmark(&path, "original", generate);
        assert_eq!(calls.get(), 1);
        load_or_benchmark(&path, "new-driver", generate);
        assert_eq!(calls.get(), 2);
        fs::write(&path, b"partial-file").unwrap();
        load_or_benchmark(&path, "new-driver", generate);
        assert_eq!(calls.get(), 3);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn only_matching_hardware_and_successful_benchmarks_enable_gpu() {
        let mut record = StoredProfile {
            version: VERSION,
            fingerprint: "cpu/gpu/driver".into(),
            benchmark: GpuBenchmark {
                eligible: true,
                adapter: "GPU".into(),
                cpu_nodes_per_second: 100.,
                gpu_nodes_per_second: 200.,
                cpu_workers: 4,
                message: String::new(),
            },
        };
        assert!(valid(&record, "cpu/gpu/driver"));
        assert!(!valid(&record, "new-driver"));
        record.benchmark.gpu_nodes_per_second = 118.;
        assert!(valid(&record, "cpu/gpu/driver"));
        record.version = 2;
        record.benchmark.gpu_nodes_per_second = 200.;
        assert!(!valid(&record, "cpu/gpu/driver"), "old shallow calibrations must expire");
        record.version = VERSION;
        record.benchmark.gpu_nodes_per_second = 50.;
        assert!(!valid(&record, "cpu/gpu/driver"));
        record.benchmark = GpuBenchmark::unavailable("No adapter");
        assert!(valid(&record, "cpu/gpu/driver"));
        record.version += 1;
        assert!(!valid(&record, "cpu/gpu/driver"));
    }
}
