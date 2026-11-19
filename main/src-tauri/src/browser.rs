//! JSON command boundary used by the Emscripten worker and host-side parity tests.
use super::*;
use serde_json::{Value, json};

pub fn events() -> String {
    tauri::drain_events()
}
pub fn dispatch(input: &str) -> String {
    let result = serde_json::from_str::<Value>(input).map_err(|e| e.to_string()).and_then(|message| {
        let command = message["command"].as_str().ok_or("Missing command")?;
        let args = &message["args"];
        let app = AppHandle;
        let solver = || app.state::<SolverState>();
        let batch_state = || app.state::<batch::BatchState>();
        fn field<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
            serde_json::from_value(args[key].clone()).map_err(|e| format!("{key}: {e}"))
        }
        fn value<T: Serialize>(result: Result<T, String>) -> Result<Value, String> {
            result.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        if matches!(command, "delete_pruning_table" | "reset_app_data" | "unload_pruning_table")
            && solver().current_cancel.lock().map_err(|_| "solver lock poisoned")?.is_some() {
            return Err("Stop the active operation before changing stored tables.".into());
        }
        match command {
            "solve_fto" => value(futures_executor::block_on(solve_fto(field(args, "request")?, solver(), app.clone()))),
            "generate_pruning_table" => value(futures_executor::block_on(generate_pruning_table(field(args, "request")?, solver(), app.clone()))),
            "generate_bloom_table" => value(futures_executor::block_on(bloom_cache::generate_bloom_table(field(args, "request")?, solver(), app.clone()))),
            "stop_solve" => value(stop_solve(solver())),
            "get_solver_status" => value(get_solver_status(solver(), batch_state())),
            "unload_pruning_table" => value(unload_pruning_table(solver())),
            "pruning_cache_status" => value(pruning_cache_status(solver())),
            "list_pruning_tables" => value(list_pruning_tables(app.clone())),
            "delete_pruning_table" => value(delete_pruning_table(field(args, "id")?, solver(), app.clone())),
            "reset_app_data" => value(reset_app_data(solver(), app.clone())),
            "validate_facelets" => Ok(serde_json::to_value(validate_facelets(field(args, "facelets")?)).unwrap()),
            "start_batch" => value(futures_executor::block_on(batch::start_batch(field(args, "request")?, solver(), batch_state(), app.clone()))),
            "pause_batch" => value(futures_executor::block_on(batch::pause_batch(batch_state(), solver()))),
            "batch_status" => Ok(json!(batch::batch_status(batch_state()))),
            "get_gpu_profile" => Ok(json!(fto_core::acceleration::GpuBenchmark::unavailable(
                "This browser version uses CPU table generation. GPU acceleration is not available yet."))),
            "set_generation_gpu" => value(generation::set_generation_gpu(field(args, "enabled")?, solver(), app.clone())),
            _ => Err(format!("Unknown browser command: {command}")),
        }
    });
    match result {
        Ok(value) => json!({"ok": true, "value": value}),
        Err(error) => json!({"ok": false, "error": error}),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_boundary_validates_facelets_and_rejects_unknown_commands() {
        let colors: Vec<_> = (0..72).map(|i| i / 9).collect();
        let result: Value = serde_json::from_str(&dispatch(
            &json!({"command":"validate_facelets", "args":{"facelets":colors}}).to_string(),
        ))
        .unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["value"]["errors"], json!([]));
        assert!(dispatch(r#"{"command":"missing"}"#).contains("Unknown browser command"));
        assert!(dispatch("bad json").contains("\"ok\":false"));
    }
}
