//! The desktop command handlers use this small host adapter in browser builds.
//! Solver state, cancellation and scheduling remain shared with the desktop app.
use serde::Serialize;
use std::{
    any::{Any, TypeId},
    collections::{HashMap, VecDeque},
    ops::Deref,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Default)]
pub struct AppHandle;
pub struct State<'a, T>(Arc<T>, std::marker::PhantomData<&'a T>);
impl<T> Deref for State<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
static STATES: OnceLock<Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>> = OnceLock::new();
static EVENTS: Mutex<VecDeque<serde_json::Value>> = Mutex::new(VecDeque::new());
pub trait Manager {
    fn state<T: Default + Send + Sync + 'static>(&self) -> State<'static, T>;
    fn try_state<T: Default + Send + Sync + 'static>(&self) -> Option<State<'static, T>> {
        Some(self.state())
    }
}
impl Manager for AppHandle {
    fn state<T: Default + Send + Sync + 'static>(&self) -> State<'static, T> {
        let mut states = STATES.get_or_init(Default::default).lock().unwrap();
        let state = states
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Arc::new(T::default()))
            .clone();
        State(state.downcast::<T>().unwrap(), std::marker::PhantomData)
    }
}
pub trait Emitter {
    fn emit<T: Serialize>(&self, name: &str, payload: T) -> Result<(), String>;
}
impl Emitter for AppHandle {
    fn emit<T: Serialize>(&self, name: &str, payload: T) -> Result<(), String> {
        let event = serde_json::json!({"event": name, "payload": payload});
        EVENTS
            .lock()
            .map_err(|_| "event queue poisoned")?
            .push_back(event);
        Ok(())
    }
}
pub fn drain_events() -> String {
    serde_json::to_string(&EVENTS.lock().unwrap().drain(..).collect::<Vec<_>>()).unwrap()
}
pub struct Paths;
impl AppHandle {
    pub fn path(&self) -> Paths {
        Paths
    }
}
impl Paths {
    pub fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok("/fto".into())
    }
    pub fn app_config_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok("/fto/config".into())
    }
    pub fn download_dir(&self) -> Result<std::path::PathBuf, String> {
        Err("Use the browser download action".into())
    }
}

// Every operation runs off the browser's UI thread. These handlers only await
// blocking jobs, so a small executor is sufficient; no native I/O reactor needed.
pub mod async_runtime {
    pub fn spawn_blocking<F, T>(f: F) -> impl std::future::Future<Output = Result<T, String>> + Send
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        // Start now even when callers drop the future (the single-solve handlers).
        let handle = std::thread::spawn(f);
        async move { handle.join().map_err(|_| "worker panicked".to_owned()) }
    }
    pub fn spawn<F>(future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        std::thread::spawn(move || futures_executor::block_on(future));
    }
}
