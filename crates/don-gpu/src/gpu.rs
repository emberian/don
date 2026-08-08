//! wgpu compute port of the flow-field relaxation.
//!
//! The host side deliberately mirrors what a training loop would actually do: buffers are
//! allocated once for a batch shape and reused, uploads and downloads are timed separately
//! from compute, and convergence is detected on-device with an occasional flag readback
//! rather than a per-iteration round trip.
//!
//! Everything is `u32`. See the crate docs for why that is the whole determinism story.

use crate::cpu::MAX_COST;
use crate::field::{FieldBatch, COST_BLOCKED};
use bytemuck::{Pod, Zeroable};
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt as _;

/// Workgroup tile edge; must match `TILE` in `shaders/flowfield.wgsl`.
pub const TILE: u32 = 8;
/// A dispatch dimension is capped at 65535 by WebGPU, so long batches are split.
const MAX_DISPATCH: u32 = 65535;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    width: u32,
    height: u32,
    fields: u32,
    inner_steps: u32,
    field_offset: u32,
    stride: u32,
    pad0: u32,
    pad1: u32,
}

/// Device handle plus the two compiled pipelines. Cheap to clone (wgpu handles are
/// reference counted), expensive to create — make one per process.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub limits: wgpu::Limits,
    relax_layout: wgpu::BindGroupLayout,
    relax_pipeline: wgpu::ComputePipeline,
    dir_layout: wgpu::BindGroupLayout,
    dir_pipeline: wgpu::ComputePipeline,
}

/// Why a GPU solve could not be attempted. Callers in tests treat this as "skip", not
/// "fail" — headless CI has no adapter and that is not a bug in this crate.
#[derive(Debug)]
pub enum GpuUnavailable {
    NoAdapter(String),
    NoDevice(String),
}

impl std::fmt::Display for GpuUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuUnavailable::NoAdapter(e) => write!(f, "no wgpu adapter: {e}"),
            GpuUnavailable::NoDevice(e) => write!(f, "no wgpu device: {e}"),
        }
    }
}

impl Gpu {
    pub fn new() -> Result<Gpu, GpuUnavailable> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| GpuUnavailable::NoAdapter(e.to_string()))?;

        // Take the adapter's own limits, not the conservative downlevel defaults: the
        // default `max_storage_buffer_binding_size` is 128 MiB, which caps the batch at a
        // few hundred fields and would make the crossover measurement a measurement of
        // wgpu's defaults instead of the hardware.
        let limits = adapter.limits();
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("don-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| GpuUnavailable::NoDevice(e.to_string()))?;

        let relax_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("flowfield"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/flowfield.wgsl").into()),
        });
        let dir_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("directions"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/directions.wgsl").into()),
        });

        let relax_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("relax-bgl"),
            entries: &[
                uniform_entry(0),
                storage_entry(1, true),  // cost
                storage_entry(2, true),  // src
                storage_entry(3, false), // dst
                storage_entry(4, false), // flags
            ],
        });
        let dir_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dir-bgl"),
            entries: &[
                uniform_entry(0),
                storage_entry(1, true),
                storage_entry(2, true),
                storage_entry(3, false),
            ],
        });

        let relax_pipeline = make_pipeline(&device, &relax_layout, &relax_module, "relax");
        let dir_pipeline = make_pipeline(&device, &dir_layout, &dir_module, "directions");

        Ok(Gpu {
            device,
            queue,
            info,
            limits,
            relax_layout,
            relax_pipeline,
            dir_layout,
            dir_pipeline,
        })
    }

    /// Bytes of device storage a batch of this shape needs (cost + two distance buffers +
    /// the direction buffer). Useful for deciding whether a batch size is even attemptable
    /// before the allocation fails.
    pub fn batch_bytes(width: u32, height: u32, fields: u32) -> u64 {
        4 * (width as u64) * (height as u64) * (fields as u64) * 4
    }

    pub fn solver(&self, width: u32, height: u32, fields: u32) -> FlowSolver<'_> {
        FlowSolver::new(self, width, height, fields)
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    module: &wgpu::ShaderModule,
    entry: &str,
) -> wgpu::ComputePipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: &[bgl],
        push_constant_ranges: &[],
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&layout),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

/// Knobs for a GPU solve.
#[derive(Clone, Copy, Debug)]
pub struct SolveOptions {
    /// Jacobi steps performed in workgroup memory per global round. Higher values trade a
    /// staler halo for less global bandwidth; the fixed point is unchanged either way.
    pub inner_steps: u32,
    /// Global rounds per submission before the convergence flag is read back. A readback is
    /// a full pipeline stall, so this is the sync-cost knob.
    pub rounds_per_poll: u32,
    /// Hard cap so a pathological field cannot hang the process.
    pub max_rounds: u32,
}

impl Default for SolveOptions {
    fn default() -> Self {
        // Measured optimum on an Apple M2 Max across 64x64 and 256x256 grids
        // (`flowbench --c`): 8 shared-memory steps per global round, and 8 global rounds
        // between convergence-flag readbacks. Both are hardware-dependent — 8 inner steps
        // is roughly where the stale halo stops paying, and 8 rounds per poll is where the
        // readback stall stops dominating. Retune per device.
        SolveOptions {
            inner_steps: 8,
            rounds_per_poll: 8,
            max_rounds: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SolveStats {
    pub rounds: u32,
    pub polls: u32,
    pub converged: bool,
    pub upload: Duration,
    pub compute: Duration,
    pub download: Duration,
}

/// Buffers and bind groups for one batch shape, reusable across many solves.
pub struct FlowSolver<'g> {
    gpu: &'g Gpu,
    pub width: u32,
    pub height: u32,
    pub fields: u32,
    cost: wgpu::Buffer,
    dist: [wgpu::Buffer; 2],
    dirs: wgpu::Buffer,
    flags: wgpu::Buffer,
    flags_staging: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// One params uniform per field-chunk, since `field_offset` differs per chunk.
    chunk_params: Vec<wgpu::Buffer>,
    /// `[parity][chunk]` — parity 0 reads dist[0] and writes dist[1].
    relax_groups: [Vec<wgpu::BindGroup>; 2],
    /// Which buffer currently holds the answer.
    parity: usize,
}

impl<'g> FlowSolver<'g> {
    fn new(gpu: &'g Gpu, width: u32, height: u32, fields: u32) -> FlowSolver<'g> {
        let cells = (width as u64) * (height as u64) * (fields as u64);
        let bytes = cells * 4;
        let d = &gpu.device;
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let mk = |label: &str, usage| {
            d.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        let cost = mk("cost", storage);
        let dist = [mk("dist0", storage), mk("dist1", storage)];
        let dirs = mk("dirs", storage);
        let flags = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flags"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let flags_staging = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flags-staging"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let readback = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let nchunks = fields.div_ceil(MAX_DISPATCH).max(1) as usize;
        let mut chunk_params = Vec::with_capacity(nchunks);
        for c in 0..nchunks {
            let p = Params {
                width,
                height,
                fields,
                inner_steps: 1,
                field_offset: c as u32 * MAX_DISPATCH,
                stride: 0,
                pad0: 0,
                pad1: 0,
            };
            chunk_params.push(d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: bytemuck::bytes_of(&p),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }));
        }

        let mut relax_groups = [Vec::with_capacity(nchunks), Vec::with_capacity(nchunks)];
        for parity in 0..2usize {
            for c in 0..nchunks {
                relax_groups[parity].push(d.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("relax-bg"),
                    layout: &gpu.relax_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: chunk_params[c].as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: cost.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: dist[parity].as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: dist[1 - parity].as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 4, resource: flags.as_entire_binding() },
                    ],
                }));
            }
        }

        FlowSolver {
            gpu,
            width,
            height,
            fields,
            cost,
            dist,
            dirs,
            flags,
            flags_staging,
            readback,
            chunk_params,
            relax_groups,
            parity: 0,
        }
    }

    fn cells(&self) -> u64 {
        (self.width as u64) * (self.height as u64) * (self.fields as u64)
    }

    /// Upload the batch's cost and initial distance columns.
    pub fn upload(&mut self, b: &FieldBatch) -> Duration {
        assert_eq!((b.width, b.height, b.fields), (self.width, self.height, self.fields));
        assert!(
            b.cost.iter().all(|&c| c == COST_BLOCKED || c <= MAX_COST),
            "entry costs must be <= MAX_COST or exactly COST_BLOCKED"
        );
        let t = Instant::now();
        self.gpu.queue.write_buffer(&self.cost, 0, bytemuck::cast_slice(&b.cost));
        self.gpu.queue.write_buffer(&self.dist[0], 0, bytemuck::cast_slice(&b.dist));
        self.gpu.queue.write_buffer(&self.dist[1], 0, bytemuck::cast_slice(&b.dist));
        self.parity = 0;
        self.gpu.queue.submit([]);
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        t.elapsed()
    }

    /// Relax to the fixed point. Returns round counts and the compute wall time.
    ///
    /// The number of rounds can depend on `rounds_per_poll` (we may overshoot convergence
    /// by up to a poll group) but the *result* cannot: extra rounds at the fixed point are
    /// no-ops by construction.
    pub fn run(&mut self, opts: SolveOptions) -> SolveStats {
        let mut stats = SolveStats::default();
        // publish inner_steps into every chunk's params
        for (c, buf) in self.chunk_params.iter().enumerate() {
            let p = Params {
                width: self.width,
                height: self.height,
                fields: self.fields,
                inner_steps: opts.inner_steps.max(1),
                field_offset: c as u32 * MAX_DISPATCH,
                stride: 0,
                pad0: 0,
                pad1: 0,
            };
            self.gpu.queue.write_buffer(buf, 0, bytemuck::bytes_of(&p));
        }

        let gx = self.width.div_ceil(TILE);
        let gy = self.height.div_ceil(TILE);
        let nchunks = self.chunk_params.len();
        let t = Instant::now();

        while stats.rounds < opts.max_rounds {
            let group = opts.rounds_per_poll.max(1).min(opts.max_rounds - stats.rounds);
            let mut enc = self
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("relax") });
            enc.clear_buffer(&self.flags, 0, None);
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("relax"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.gpu.relax_pipeline);
                for _ in 0..group {
                    for c in 0..nchunks {
                        let zlo = c as u32 * MAX_DISPATCH;
                        let z = (self.fields - zlo).min(MAX_DISPATCH);
                        pass.set_bind_group(0, &self.relax_groups[self.parity][c], &[]);
                        pass.dispatch_workgroups(gx, gy, z);
                    }
                    self.parity ^= 1;
                }
            }
            enc.copy_buffer_to_buffer(&self.flags, 0, &self.flags_staging, 0, 4);
            self.gpu.queue.submit([enc.finish()]);

            let changed = self.read_flag();
            stats.rounds += group;
            stats.polls += 1;
            if !changed {
                stats.converged = true;
                break;
            }
        }
        stats.compute = t.elapsed();
        stats
    }

    fn read_flag(&self) -> bool {
        let slice = self.flags_staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let _ = rx.recv();
        let v = {
            let view = slice.get_mapped_range();
            u32::from_le_bytes([view[0], view[1], view[2], view[3]])
        };
        self.flags_staging.unmap();
        v != 0
    }

    /// Copy the solved distance column back into the batch.
    pub fn download(&self, b: &mut FieldBatch) -> Duration {
        let t = Instant::now();
        self.copy_into(&self.dist[self.parity], bytemuck::cast_slice_mut(&mut b.dist));
        t.elapsed()
    }

    /// Run the direction-extraction kernel and copy the result back.
    pub fn directions(&self, out: &mut [u32]) {
        assert_eq!(out.len() as u64, self.cells());
        let total = self.cells() as u32;
        let wg = total.div_ceil(64).max(1);
        let wg_x = wg.min(MAX_DISPATCH);
        let wg_y = wg.div_ceil(wg_x);
        let p = Params {
            width: self.width,
            height: self.height,
            fields: self.fields,
            inner_steps: 0,
            field_offset: 0,
            stride: wg_x * 64,
            pad0: 0,
            pad1: 0,
        };
        let pbuf = self
            .gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("dir-params"),
                contents: bytemuck::bytes_of(&p),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bg = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dir-bg"),
            layout: &self.gpu.dir_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: pbuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.cost.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: self.dist[self.parity].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: self.dirs.as_entire_binding() },
            ],
        });
        let mut enc = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("dirs") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("dirs"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.gpu.dir_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }
        self.gpu.queue.submit([enc.finish()]);
        self.copy_into(&self.dirs, bytemuck::cast_slice_mut(out));
    }

    fn copy_into(&self, src: &wgpu::Buffer, out: &mut [u8]) {
        let bytes = self.cells() * 4;
        let mut enc = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_buffer_to_buffer(src, 0, &self.readback, 0, bytes);
        self.gpu.queue.submit([enc.finish()]);
        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let _ = rx.recv();
        out.copy_from_slice(&slice.get_mapped_range()[..bytes as usize]);
        self.readback.unmap();
    }
}

/// Convenience: allocate, upload, solve, download. Allocation is not free, so a training
/// loop should hold a [`FlowSolver`] instead of calling this per tick.
pub fn solve_batch_gpu(gpu: &Gpu, b: &mut FieldBatch, opts: SolveOptions) -> SolveStats {
    let mut s = gpu.solver(b.width, b.height, b.fields);
    let upload = s.upload(b);
    let mut stats = s.run(opts);
    let download = s.download(b);
    stats.upload = upload;
    stats.download = download;
    stats
}
