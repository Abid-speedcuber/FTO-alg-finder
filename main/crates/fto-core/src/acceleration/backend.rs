use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;

pub(crate) struct Gpu {
    pub name: String,
    pub kind: wgpu::DeviceType,
    pub budget: u64,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub binding_limit: u64,
    pub dispatch_limit: u32,
    failure: Arc<std::sync::Mutex<Option<String>>>,
}
pub(crate) fn active(cancel: Option<&AtomicBool>) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        Err("solve cancelled".into())
    } else {
        Ok(())
    }
}
fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}
pub(crate) fn words(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
impl Gpu {
    pub fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let mut adapters = instance.enumerate_adapters(wgpu::Backends::all());
        adapters.sort_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu => 2,
            _ => 3,
        });
        let mut errors = Vec::new();
        for adapter in adapters {
            let info = adapter.get_info();
            if info.device_type == wgpu::DeviceType::Cpu
                && std::env::var_os("FTO_GPU_ALLOW_SOFTWARE").is_none()
            {
                continue;
            }
            let supported = adapter.limits();
            if supported.max_storage_buffers_per_shader_stage < 6 {
                continue;
            }
            let mut limits = wgpu::Limits::default();
            limits.max_storage_buffers_per_shader_stage = 6;
            limits.max_compute_invocations_per_workgroup = 64;
            limits.max_compute_workgroup_size_x = 64;
            limits.max_compute_workgroup_size_y = 1;
            limits.max_compute_workgroup_size_z = 1;
            limits.max_compute_workgroup_storage_size = 0;
            limits.max_storage_buffer_binding_size = supported.max_storage_buffer_binding_size;
            limits.max_buffer_size = supported.max_buffer_size;
            limits.max_compute_workgroups_per_dimension =
                supported.max_compute_workgroups_per_dimension;
            let result = pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("FTO bounded compute"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                },
                None,
            ));
            match result {
                Ok((device, queue)) => {
                    let failure = Arc::new(std::sync::Mutex::new(None));
                    let errors = failure.clone();
                    device.on_uncaptured_error(Box::new(move |error| {
                        if let Ok(mut state) = errors.lock() {
                            *state = Some(error.to_string());
                        }
                    }));
                    let lost = failure.clone();
                    device.set_device_lost_callback(move |reason, message| {
                        if let Ok(mut state) = lost.lock() {
                            *state = Some(format!("GPU device lost: {reason:?}: {message}"));
                        }
                    });
                    let binding_limit = u64::from(supported.max_storage_buffer_binding_size)
                        .min(supported.max_buffer_size);
                    let available = super::available_memory_bytes();
                    // These are working buffers, not a ceiling on Bloom storage.
                    // No portable API reports currently free dedicated VRAM.
                    let budget = if info.device_type != wgpu::DeviceType::DiscreteGpu {
                        available / 3
                    } else {
                        available / 2
                    }
                    .min(1024 * 1024 * 1024)
                    .max(8 * 1024 * 1024);
                    return Ok(Self {
                        name: info.name,
                        kind: info.device_type,
                        budget,
                        device,
                        queue,
                        binding_limit,
                        failure,
                        dispatch_limit: supported
                            .max_compute_workgroups_per_dimension
                            .saturating_mul(64),
                    });
                }
                Err(e) => errors.push(format!("{}: {e}", info.name)),
            }
        }
        Err(if errors.is_empty() {
            "no compatible hardware compute adapter".into()
        } else {
            errors.join("; ")
        })
    }
    fn begin_errors(&self) {
        self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
    }
    fn end_errors(&self) -> Result<(), String> {
        let validation = pollster::block_on(self.device.pop_error_scope());
        let memory = pollster::block_on(self.device.pop_error_scope());
        if let Some(error) = validation.or(memory) {
            Err(error.to_string())
        } else {
            Ok(())
        }
    }
    fn buffer(&self, label: &str, values: &[u32], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: &bytes(values),
                usage,
            })
    }
    fn empty(&self, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size.max(4),
            usage,
            mapped_at_creation: false,
        })
    }
    fn pipeline(&self, label: &str, source: &str) -> wgpu::ComputePipeline {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        self.device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
    }
    fn bind(&self, pipeline: &wgpu::ComputePipeline, buffers: &[&wgpu::Buffer]) -> wgpu::BindGroup {
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("FTO compute buffers"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        })
    }
    fn submit(
        &self,
        pipeline: &wgpu::ComputePipeline,
        bind: &wgpu::BindGroup,
        count: u32,
        copy: Option<(&wgpu::Buffer, &wgpu::Buffer, u64)>,
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("FTO short compute batch"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
        }
        if let Some((source, target, size)) = copy {
            encoder.copy_buffer_to_buffer(source, 0, target, 0, size);
        }
        self.queue.submit(Some(encoder.finish()));
    }
    fn healthy(&self) -> Result<(), String> {
        if let Some(error) = self
            .failure
            .lock()
            .map_err(|_| "GPU error lock poisoned")?
            .as_ref()
        {
            Err(error.clone())
        } else {
            Ok(())
        }
    }
    fn wait(&self, cancel: Option<&AtomicBool>) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.queue.on_submitted_work_done(move || {
            let _ = tx.send(());
        });
        // Poll rather than block the app's worker indefinitely. An in-flight dispatch
        // must finish before resources can be reused, even after cancellation.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.healthy()?;
            if Instant::now() >= deadline {
                return Err("GPU batch timed out; using CPU".into());
            }
            self.device.poll(wgpu::Maintain::Poll);
            match rx.try_recv() {
                Ok(()) => return active(cancel),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("GPU completion callback disconnected".into());
                }
                Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }
    fn read(
        &self,
        buffer: &wgpu::Buffer,
        length: u64,
        cancel: Option<&AtomicBool>,
    ) -> Result<Vec<u32>, String> {
        self.wait(cancel)?;
        let (tx, rx) = mpsc::channel();
        buffer
            .slice(..length)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.healthy()?;
            if Instant::now() >= deadline {
                return Err("GPU readback timed out; using CPU".into());
            }
            self.device.poll(wgpu::Maintain::Poll);
            match rx.try_recv() {
                Ok(result) => {
                    result.map_err(|e| e.to_string())?;
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("GPU map callback disconnected".into());
                }
                Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        let result = words(&buffer.slice(..length).get_mapped_range());
        buffer.unmap();
        active(cancel)?;
        Ok(result)
    }
}
struct BatchSize {
    items: u32,
    max: u32,
    target_ms: f64,
    integrated: bool,
}
impl BatchSize {
    fn new(gpu: &Gpu, max: u32) -> Self {
        Self {
            items: 256.min(max),
            max,
            target_ms: if gpu.kind != wgpu::DeviceType::DiscreteGpu {
                8.
            } else {
                16.
            },
            integrated: gpu.kind != wgpu::DeviceType::DiscreteGpu,
        }
    }
    fn observe(&mut self, elapsed: Duration) {
        let ms = elapsed.as_secs_f64() * 1000.;
        let factor = (self.target_ms / ms.max(0.1)).clamp(0.25, 2.);
        self.items = ((self.items as f64 * factor) as u32).clamp(1, self.max);
        // Leave scheduling room for the desktop compositor. A duty-cycle target,
        // not a promise about power draw or driver scheduling.
        std::thread::sleep(
            elapsed
                .mul_f64(if self.integrated { 0.2 } else { 0.05 })
                .min(Duration::from_millis(4)),
        );
    }
}

// Implementations below share adapter selection, allocation checks and pacing.

impl Gpu {
    pub(crate) fn pruning(
        &self,
        name: &str,
        dimensions: &[u32],
        transitions: &[u32],
        root: usize,
        moves: usize,
        cancel: Option<&AtomicBool>,
        report: Option<&crate::pruning::PruningReporter<'_>>,
    ) -> Result<Option<Vec<u8>>, String> {
        active(cancel)?;
        let total = dimensions
            .iter()
            .try_fold(1_u64, |n, &s| n.checked_mul(u64::from(s)))
            .ok_or("GPU coordinate size overflow")?;
        if total > u64::from(u32::MAX)
            || dimensions.len() > 13
            || moves == 0
            || root as u64 >= total
        {
            return Ok(None);
        }
        let bit_bytes = total.div_ceil(32) * 4;
        let transition_bytes = transitions.len() as u64 * 4;
        if bit_bytes > self.binding_limit
            || transition_bytes > self.binding_limit
            || bit_bytes * 3 + transition_bytes + 4 * 1024 * 1024 > self.budget
        {
            super::log(&format!(
                "GPU working set for {name} exceeds the safe device budget; using CPU"
            ));
            return Ok(None);
        }
        let gpu_working = bit_bytes * 3 + transition_bytes + 4 * 1024 * 1024;
        let host_extra = bit_bytes * 2
            + 8 * 1024 * 1024
            + if self.kind != wgpu::DeviceType::DiscreteGpu {
                gpu_working
            } else {
                bit_bytes
            };
        super::check_host_allocation(total + host_extra)?;
        let mut table = Vec::new();
        table
            .try_reserve_exact(total as usize)
            .map_err(|_| "not enough RAM for pruning table")?;
        table.resize(total as usize, 255);
        table[root] = 0;
        let mut parameters = vec![
            total as u32,
            moves as u32,
            dimensions.len() as u32,
            0,
            0,
            0,
            0,
            0,
        ];
        let mut offset = 0;
        for &dimension in dimensions {
            parameters.extend([dimension, offset]);
            offset += dimension * moves as u32;
        }
        if offset as usize != transitions.len() {
            return Err("GPU transition table length mismatch".into());
        }
        let rw = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC;
        self.begin_errors();
        let pipeline = self.pipeline("FTO exact BFS", include_str!("pruning.wgsl"));
        let host_transitions = transitions;
        let transitions = self.buffer(
            "coordinate transitions",
            transitions,
            wgpu::BufferUsages::STORAGE,
        );
        let params = self.buffer(
            "BFS parameters",
            &parameters,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let visited = self.empty("BFS visited bits", bit_bytes, rw);
        let input_capacity = self
            .dispatch_limit
            .min(1_048_576)
            .min((self.binding_limit / 4) as u32);
        let frontier = self.empty("BFS frontier IDs", u64::from(input_capacity) * 4, rw);
        let next = self.empty("BFS next bits", bit_bytes, rw);
        let readback = self.empty(
            "BFS bounded readback",
            bit_bytes,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        self.end_errors()?;
        let root_flag = (1_u32 << (root % 32)).to_le_bytes();
        self.queue
            .write_buffer(&visited, (root / 32 * 4) as u64, &root_flag);

        let mut batch = BatchSize::new(self, input_capacity);
        let mut host_frontier = vec![0_u32; (bit_bytes / 4) as usize];
        host_frontier[root / 32] = 1 << (root % 32);
        let mut reached = 1;
        let mut expanded = 0;
        let mut frontier_count = 1;
        let mut calibrated = super::forced();
        super::log(&format!(
            "generating {name} on {} (GPU BFS working set {:.1} MiB)",
            self.name,
            (bit_bytes * 3 + transition_bytes + 4 * 1024 * 1024) as f64 / 1048576.
        ));
        for depth in 0..254_u8 {
            active(cancel)?;
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.clear_buffer(&next, 0, None);
            self.queue.submit(Some(encoder.finish()));
            let bind = self.bind(
                &pipeline,
                &[&transitions, &params, &visited, &frontier, &next],
            );
            let mut last_report = Instant::now();
            let mut pending = Vec::with_capacity(batch.max as usize);
            let mut word = 0_usize;
            let mut remaining_bits = host_frontier[0];
            loop {
                active(cancel)?;
                while pending.len() < batch.items as usize && word < host_frontier.len() {
                    if remaining_bits == 0 {
                        if word & 8191 == 0 {
                            active(cancel)?;
                        }
                        word += 1;
                        if word < host_frontier.len() {
                            remaining_bits = host_frontier[word];
                        }
                        continue;
                    }
                    let bit = remaining_bits.trailing_zeros();
                    remaining_bits &= remaining_bits - 1;
                    pending.push(word as u32 * 32 + bit);
                }
                if pending.is_empty() {
                    break;
                }
                parameters[4] = pending.len() as u32;
                self.queue.write_buffer(&frontier, 0, &bytes(&pending));
                self.queue.write_buffer(&params, 0, &bytes(&parameters));
                let started = Instant::now();
                self.submit(&pipeline, &bind, pending.len() as u32, None);
                self.wait(cancel)?;
                let gpu_time = started.elapsed();
                if !calibrated
                    && (pending.len() >= 16_384
                        || (pending.len() >= 1024 && gpu_time.as_millis() >= 16))
                {
                    let cpu_started = Instant::now();
                    let mut changed = Vec::new();
                    let sample_count = pending.len().min(4096);
                    for &index in pending.iter().take(sample_count) {
                        let mut coords = [0_u32; 13];
                        let mut remaining = index;
                        for c in (0..dimensions.len()).rev() {
                            coords[c] = remaining % dimensions[c];
                            remaining /= dimensions[c];
                        }
                        for m in 0..moves {
                            let mut child = 0;
                            let mut offset = 0;
                            for (c, &size) in dimensions.iter().enumerate() {
                                child = child * size
                                    + host_transitions[offset + coords[c] as usize * moves + m];
                                offset += size as usize * moves;
                            }
                            if table[child as usize] == 255 {
                                table[child as usize] = depth + 1;
                                changed.push(child as usize);
                            }
                        }
                    }
                    let cpu_time = cpu_started.elapsed();
                    for index in changed {
                        table[index] = 255;
                    }
                    calibrated = true;
                    let gpu_per_state = gpu_time.as_secs_f64() / pending.len() as f64;
                    let cpu_per_state = cpu_time.as_secs_f64() / sample_count as f64;
                    if gpu_per_state > cpu_per_state * 0.8 {
                        return Err(format!(
                            "GPU pruning pilot is slower than CPU ({:.0} vs {:.0} ns/state)",
                            gpu_per_state * 1e9,
                            cpu_per_state * 1e9
                        ));
                    }
                }
                batch.observe(started.elapsed());
                pending.clear();
                if last_report.elapsed() >= Duration::from_millis(200) {
                    if let Some(report) = report {
                        report(crate::pruning::PruningProgress {
                            name: name.into(),
                            depth: depth as usize,
                            expanded,
                            reached,
                            total: total as usize,
                        });
                    }
                    last_report = Instant::now();
                }
            }
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(&next, 0, &readback, 0, bit_bytes);
            self.queue.submit(Some(encoder.finish()));
            let frontier_bits = self.read(&readback, bit_bytes, cancel)?;
            expanded += frontier_count;
            frontier_count = 0;
            for (word, mut bits) in frontier_bits.iter().copied().enumerate() {
                if word & 8191 == 0 {
                    active(cancel)?;
                }
                while bits != 0 {
                    let bit = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    let index = word * 32 + bit;
                    if index >= table.len() || table[index] != 255 {
                        return Err("GPU BFS returned an invalid or duplicate discovery".into());
                    }
                    table[index] = depth + 1;
                    frontier_count += 1;
                }
            }
            reached += frontier_count;
            if let Some(report) = report {
                report(crate::pruning::PruningProgress {
                    name: name.into(),
                    depth: depth as usize + 1,
                    expanded,
                    reached,
                    total: total as usize,
                });
            }
            if frontier_count == 0 || reached == total as usize {
                return Ok(Some(table));
            }
            host_frontier = frontier_bits;
        }
        Err("pruning distance exceeds the table's byte representation".into())
    }
}

pub(crate) struct BloomCompute {
    gpu: Arc<Gpu>,
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    params: wgpu::Buffer,
    jobs: wgpu::Buffer,
    output: wgpu::Buffer,
    counters: wgpu::Buffer,
    readback: wgpu::Buffer,
    parameters: Vec<u32>,
    max_jobs: usize,
    pub tail_depth: u8,
    pub last_elapsed: Duration,
    pub last_paths: u64,
    pub occupied: u64,
    filter: wgpu::Buffer,
    filter_bytes: u64,
    readback_bytes: u64,
    nodes_per_job: u32,
    pub pending: Vec<[u32; 11]>,
    batch: BatchSize,
}
impl BloomCompute {
    pub fn new(
        gpu: Arc<Gpu>,
        transitions: &[u32],
        mut parameters: Vec<u32>,
        depth: u8,
        filter_bytes: u64,
    ) -> Result<Option<Self>, String> {
        let moves = parameters[1];
        let mut tail_depth = 0;
        let mut nodes_per_job = 1_u32;
        let mut layer = 1_u32;
        while tail_depth < depth {
            let Some(next_layer) = layer.checked_mul(moves) else {
                break;
            };
            let Some(next_count) = nodes_per_job.checked_add(next_layer) else {
                break;
            };
            if next_count > 256 {
                break;
            }
            tail_depth += 1;
            layer = next_layer;
            nodes_per_job = next_count;
        }
        if tail_depth == 0 {
            return Ok(None);
        }
        let capacity = (gpu.budget / 64)
            .min(gpu.binding_limit / 16)
            .min(u64::from(gpu.dispatch_limit))
            .min(1_048_576) as u32;
        if capacity < nodes_per_job
            || transitions.len() as u64 * 4 > gpu.binding_limit
            || transitions.len() as u64 * 4 + u64::from(capacity) * 48 > gpu.budget
        {
            return Ok(None);
        }
        let max_jobs = (capacity / nodes_per_job) as usize;
        let capacity = max_jobs as u32 * nodes_per_job;
        parameters[2] = u32::from(tail_depth);
        parameters[3] = nodes_per_job;
        parameters[7] = capacity;
        let output_bytes = u64::from(capacity) * 16;
        let resident = filter_bytes > 0
            && filter_bytes <= gpu.binding_limit
            && filter_bytes + transitions.len() as u64 * 4 + u64::from(capacity) * 48 <= gpu.budget;
        if resident {
            parameters[16] = if super::forced() { 1 } else { 2 };
            let bits = filter_bytes * 8;
            parameters[17] = bits as u32;
            parameters[18] = (bits >> 32) as u32;
            parameters[19] = u32::from(bits.is_power_of_two());
            parameters[20] = (bits - 1) as u32;
            parameters[21] = ((bits - 1) >> 32) as u32;
        }
        gpu.begin_errors();
        let pipeline = gpu.pipeline("FTO near-goal enumeration", include_str!("bloom.wgsl"));
        let transitions = gpu.buffer(
            "Bloom move transitions",
            transitions,
            wgpu::BufferUsages::STORAGE,
        );
        let params = gpu.buffer(
            "Bloom parameters",
            &parameters,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let jobs = gpu.empty(
            "Bloom prefix jobs",
            max_jobs as u64 * 44,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let output = gpu.empty(
            "Bloom stable hashes",
            output_bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let counters = gpu.empty(
            "Bloom completion counters",
            16,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        );
        let readback = gpu.empty(
            "Bloom bounded readback",
            output_bytes + 16,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let filter = gpu.empty(
            "GPU Bloom bit array",
            if resident { filter_bytes } else { 4 },
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        gpu.end_errors()?;
        let bind = gpu.bind(
            &pipeline,
            &[&transitions, &params, &jobs, &output, &counters, &filter],
        );
        let mut batch = BatchSize::new(&gpu, capacity);
        batch.items = batch.items.max(nodes_per_job);
        super::log(&format!(
            "Bloom GPU enumeration on {}; {}-move tail, {}",
            gpu.name,
            tail_depth,
            if resident {
                format!(
                    "GPU-resident {:.1} MiB filter",
                    filter_bytes as f64 / 1048576.
                )
            } else {
                format!(
                    "system-RAM filter, bounded {:.1} MiB transfer buffers",
                    output_bytes as f64 / 1048576.
                )
            }
        ));
        Ok(Some(Self {
            gpu,
            pipeline,
            bind,
            params,
            jobs,
            output,
            counters,
            readback,
            parameters,
            max_jobs,
            tail_depth,
            last_elapsed: Duration::ZERO,
            last_paths: 0,
            occupied: 0,
            filter,
            filter_bytes: if resident { filter_bytes } else { 0 },
            readback_bytes: output_bytes + 16,
            nodes_per_job,
            pending: Vec::new(),
            batch,
        }))
    }
    pub fn resident(&self) -> bool {
        self.filter_bytes > 0
    }
    pub fn retain_gpu(&mut self) {
        if self.resident() {
            self.parameters[16] = 1;
        }
    }
    pub fn false_positive_rate(&self) -> f64 {
        if self.filter_bytes == 0 {
            0.
        } else {
            (self.occupied as f64 / (self.filter_bytes as f64 * 8.)).powi(7)
        }
    }
    pub fn merge_filter(
        &self,
        target: &mut [u64],
        cancel: Option<&AtomicBool>,
    ) -> Result<(), String> {
        if !self.resident() {
            return Ok(());
        }
        if target.len() as u64 * 8 != self.filter_bytes {
            return Err("GPU Bloom filter length mismatch".into());
        }
        let chunk = (self.readback_bytes.min(16 * 1024 * 1024) / 8 * 8).max(8);
        let mut offset = 0;
        while offset < self.filter_bytes {
            active(cancel)?;
            let size = chunk.min(self.filter_bytes - offset);
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(&self.filter, offset, &self.readback, 0, size);
            self.gpu.queue.submit(Some(encoder.finish()));
            let words = self.gpu.read(&self.readback, size, cancel)?;
            for (i, word) in words.chunks_exact(2).enumerate() {
                if i & 8191 == 0 {
                    active(cancel)?;
                }
                target[offset as usize / 8 + i] |= u64::from(word[0]) | (u64::from(word[1]) << 32);
            }
            offset += size;
        }
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.pending.len() >= (self.batch.items / self.nodes_per_job).max(1) as usize
    }
    pub fn flush(&mut self, cancel: Option<&AtomicBool>) -> Result<Vec<(u64, u64)>, String> {
        active(cancel)?;
        self.last_paths = 0;
        self.last_elapsed = Duration::ZERO;
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        if self.pending.len() > self.max_jobs {
            return Err("Bloom GPU job buffer overflow".into());
        }
        let jobs: Vec<u32> = self.pending.iter().flat_map(|s| *s).collect();
        self.parameters[4] = self.pending.len() as u32;
        self.parameters[5] = 0;
        self.parameters[6] = self.pending.len() as u32 * self.nodes_per_job;
        self.gpu.queue.write_buffer(&self.jobs, 0, &bytes(&jobs));
        self.gpu
            .queue
            .write_buffer(&self.params, 0, &bytes(&self.parameters));
        self.gpu.queue.write_buffer(&self.counters, 0, &[0; 8]);
        let started = Instant::now();
        self.gpu
            .submit(&self.pipeline, &self.bind, self.parameters[6], None);
        // Measure the resident kernel separately from the temporary pilot hash
        // readback: normal resident batches transfer only their counters.
        self.gpu.wait(cancel)?;
        let kernel_time = started.elapsed();
        let resident_only = self.parameters[16] == 1;
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.counters, 0, &self.readback, 0, 16);
        let size = if resident_only {
            0
        } else {
            u64::from(self.parameters[6]) * 16
        };
        if size > 0 {
            encoder.copy_buffer_to_buffer(&self.output, 0, &self.readback, 16, size);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        let words = self.gpu.read(&self.readback, size + 16, cancel)?;
        self.last_elapsed = if self.filter_bytes > 0 {
            kernel_time
        } else {
            started.elapsed()
        };
        self.batch.observe(started.elapsed());
        self.batch.items = self.batch.items.max(self.nodes_per_job);
        let count = words[0] as usize;
        self.last_paths = count as u64;
        self.occupied = u64::from(words[2]) | (u64::from(words[3]) << 32);
        if words[1] != 0 || count > self.parameters[6] as usize {
            return Err("GPU Bloom output overflow; no incomplete table will be saved".into());
        }
        let mut hashes = Vec::new();
        if !resident_only {
            hashes.reserve(count);
            for h in words[4..4 + count * 4].chunks_exact(4) {
                hashes.push((
                    u64::from(h[0]) | u64::from(h[1]) << 32,
                    u64::from(h[2]) | u64::from(h[3]) << 32,
                ));
            }
        }
        self.pending.clear();
        Ok(hashes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an actual GPU compute device"]
    fn gpu_smoke_exact_bfs_and_cancellation() {
        let gpu = Gpu::new().expect(
            "GPU test requires a hardware adapter (or explicitly FTO_GPU_ALLOW_SOFTWARE=1)",
        );
        println!(
            "GPU TEST: {} {:?}, binding={} MiB working={} MiB",
            gpu.name,
            gpu.kind,
            gpu.binding_limit / 1048576,
            gpu.budget / 1048576
        );
        let dimensions = [9, 7];
        let mut transitions = Vec::new();
        for size in dimensions {
            for value in 0..size {
                transitions.extend([(value + 1) % size, (value + size - 1) % size]);
            }
        }
        let actual = gpu
            .pruning("test cycles", &dimensions, &transitions, 4, 2, None, None)
            .unwrap()
            .unwrap();
        let mut expected = vec![255_u8; 63];
        expected[4] = 0;
        let mut queue = std::collections::VecDeque::from([4_usize]);
        while let Some(index) = queue.pop_front() {
            for m in 0..2 {
                let child = transitions[index / 7 * 2 + m] as usize * 7
                    + transitions[18 + index % 7 * 2 + m] as usize;
                if expected[child] == 255 {
                    expected[child] = expected[index] + 1;
                    queue.push_back(child);
                }
            }
        }
        assert_eq!(actual, expected);
        let cancel = AtomicBool::new(true);
        assert!(
            gpu.pruning(
                "cancel",
                &dimensions,
                &transitions,
                4,
                2,
                Some(&cancel),
                None
            )
            .is_err()
        );
        // Compile and dispatch the Bloom shader with a small complete binary tree.
        let mut parameters = vec![0; 38];
        parameters[1] = 2;
        for last in 0..3 {
            parameters[32 + last * 2] = 3;
        }
        let mut bloom = BloomCompute::new(Arc::new(gpu), &[0, 0], parameters, 3, 0)
            .unwrap()
            .unwrap();
        bloom.pending.push([0; 11]);
        let hashes = bloom.flush(None).unwrap();
        assert_eq!(hashes.len(), 15);
        assert!(hashes.iter().all(|h| h == &hashes[0]));
    }
}
