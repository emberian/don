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
    /// Index into the *active list* of the first field this dispatch chunk covers. Not a
    /// field index: the active list is compacted as fields converge.
    slot_offset: u32,
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
                storage_entry(4, false), // flags — one per field
                storage_entry(5, true),  // active list: slot -> field
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
    ///
    /// With [`SolveOptions::per_field_convergence`] on this is rounded **up to an even
    /// number**; see [`FlowSolver::run`] for why parity has to line up at a poll boundary.
    pub rounds_per_poll: u32,
    /// Hard cap so a pathological field cannot hang the process.
    pub max_rounds: u32,
    /// Drop converged fields out of the dispatch instead of running the whole batch until
    /// the slowest field is done.
    ///
    /// `false` reproduces the original batch-wide-flag behaviour exactly, and exists so the
    /// cost of *not* compacting can be measured on the same code rather than argued about.
    pub per_field_convergence: bool,
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
            per_field_convergence: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SolveStats {
    pub rounds: u32,
    pub polls: u32,
    pub converged: bool,
    pub upload: Duration,
    pub compute: Duration,
    pub download: Duration,
    /// Sum over polls of the number of fields still active — the actual work done, in
    /// field-rounds. Compare against `rounds * fields`, which is what a batch-wide flag
    /// costs. The ratio is exactly the waste the batch-wide flag was hiding.
    pub field_rounds: u64,
    /// `rounds * fields`: what this solve would have cost with one flag for the batch.
    pub field_rounds_uncompacted: u64,
    /// Round at which each field was first observed converged (a poll-group multiple).
    /// Empty when `per_field_convergence` is off.
    pub converged_at: Vec<u32>,
}

impl SolveStats {
    /// Mean and max over `converged_at`, the two numbers §4e of the architecture doc is
    /// about: the CPU pays the mean because each field stops on its own, and an
    /// uncompacted GPU pays the max for every field.
    pub fn convergence_mean_max(&self) -> (f64, u32) {
        if self.converged_at.is_empty() {
            return (self.rounds as f64, self.rounds);
        }
        let sum: u64 = self.converged_at.iter().map(|&r| r as u64).sum();
        (
            sum as f64 / self.converged_at.len() as f64,
            self.converged_at.iter().copied().max().unwrap_or(0),
        )
    }
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
    /// One `u32` per field, OR-ed by every workgroup that changed a cell.
    flags: wgpu::Buffer,
    flags_staging: wgpu::Buffer,
    active: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// One params uniform per dispatch chunk, since `slot_offset` differs per chunk.
    chunk_params: Vec<wgpu::Buffer>,
    /// `[parity][chunk]` — parity 0 reads dist[0] and writes dist[1].
    relax_groups: [Vec<wgpu::BindGroup>; 2],
    /// Which buffer currently holds the answer.
    parity: usize,
    /// Host mirror of the active list; `active_len` entries are meaningful.
    active_host: Vec<u32>,
    active_len: u32,
    /// Scratch for the per-field flag readback, so polling allocates nothing.
    flag_host: Vec<u32>,
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
        let flag_bytes = (fields as u64) * 4;
        let flags = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flags"),
            size: flag_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let flags_staging = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flags-staging"),
            size: flag_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let active_host: Vec<u32> = (0..fields).collect();
        let active = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("active"),
            contents: bytemuck::cast_slice(&active_host),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
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
                slot_offset: c as u32 * MAX_DISPATCH,
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
                        wgpu::BindGroupEntry { binding: 5, resource: active.as_entire_binding() },
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
            active,
            readback,
            chunk_params,
            relax_groups,
            parity: 0,
            active_host,
            active_len: fields,
            flag_host: vec![0; fields as usize],
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
        self.reset_active();
        self.gpu.queue.submit([]);
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        t.elapsed()
    }

    /// Restore the active list to "every field", and push it to the device.
    fn reset_active(&mut self) {
        for (i, s) in self.active_host.iter_mut().enumerate() {
            *s = i as u32;
        }
        self.active_len = self.fields;
        self.gpu
            .queue
            .write_buffer(&self.active, 0, bytemuck::cast_slice(&self.active_host));
    }

    /// Relax to the fixed point. Returns round counts and the compute wall time.
    ///
    /// The number of rounds can depend on `rounds_per_poll` (we may overshoot convergence
    /// by up to a poll group) but the *result* cannot: extra rounds at the fixed point are
    /// no-ops by construction.
    ///
    /// # Why a converged field can be dropped, and why that is not an approximation
    ///
    /// Fields are independent — no cell of field `f` ever reads a cell of field `g`. So
    /// "this field changed nothing this round" is a statement about `f` alone. It is also a
    /// statement that `f` is at its **true** fixed point, not merely at a fixed point of
    /// the temporally-blocked operator: values inside the workgroup loop are non-increasing
    /// (`nd = min(sd[idx], …)`), so a cell whose final value equals its loaded value never
    /// moved at any inner step — in particular not at the *first* inner step, whose halo is
    /// read fresh from `src`. No change at the first inner step, for every tile of the
    /// field, is exactly "one true Jacobi sweep changed nothing". Once there, min-plus
    /// relaxation is idempotent, so further rounds could only reproduce the same bits.
    ///
    /// # Why the poll group has to be even when compacting
    ///
    /// The kernel ping-pongs: round `r` reads `dist[p]` and writes `dist[1-p]`. A dropped
    /// field stops being written, so its answer freezes in whichever buffer it last landed
    /// in — and the download reads only one of them. Forcing an **even** number of rounds
    /// per poll makes parity identical at every poll boundary, so every field's latest
    /// value always sits in `dist[0]`, whether or not it is still active. That is one extra
    /// round at most, against copying whole converged fields between buffers.
    pub fn run(&mut self, opts: SolveOptions) -> SolveStats {
        let mut stats = SolveStats::default();
        let compact = opts.per_field_convergence;
        // publish inner_steps into every chunk's params
        for (c, buf) in self.chunk_params.iter().enumerate() {
            let p = Params {
                width: self.width,
                height: self.height,
                fields: self.fields,
                inner_steps: opts.inner_steps.max(1),
                slot_offset: c as u32 * MAX_DISPATCH,
                stride: 0,
                pad0: 0,
                pad1: 0,
            };
            self.gpu.queue.write_buffer(buf, 0, bytemuck::bytes_of(&p));
        }
        if compact {
            stats.converged_at = vec![0; self.fields as usize];
        }

        let gx = self.width.div_ceil(TILE);
        let gy = self.height.div_ceil(TILE);
        // Even-only arithmetic when compacting keeps parity aligned at every poll boundary.
        let per_poll = if compact {
            opts.rounds_per_poll.max(1).next_multiple_of(2)
        } else {
            opts.rounds_per_poll.max(1)
        };
        let cap = if compact { opts.max_rounds.next_multiple_of(2) } else { opts.max_rounds };
        let t = Instant::now();

        while stats.rounds < cap && self.active_len > 0 {
            let group = per_poll.min(cap - stats.rounds);
            let nchunks = (self.active_len.div_ceil(MAX_DISPATCH).max(1)) as usize;
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
                        let z = (self.active_len - zlo).min(MAX_DISPATCH);
                        pass.set_bind_group(0, &self.relax_groups[self.parity][c], &[]);
                        pass.dispatch_workgroups(gx, gy, z);
                    }
                    self.parity ^= 1;
                }
            }
            enc.copy_buffer_to_buffer(
                &self.flags,
                0,
                &self.flags_staging,
                0,
                (self.fields as u64) * 4,
            );
            self.gpu.queue.submit([enc.finish()]);

            stats.field_rounds += (self.active_len as u64) * (group as u64);
            stats.rounds += group;
            stats.polls += 1;
            self.read_flags();

            if compact {
                // Stream compaction on the field dimension: keep the still-changing fields,
                // in increasing field order. Order is irrelevant to the answer (fields are
                // independent) but keeping it sorted keeps memory access sequential and
                // makes the active list reproducible run to run.
                let mut n = 0usize;
                for i in 0..self.active_len as usize {
                    let f = self.active_host[i];
                    if self.flag_host[f as usize] != 0 {
                        self.active_host[n] = f;
                        n += 1;
                    } else {
                        stats.converged_at[f as usize] = stats.rounds;
                    }
                }
                if n != self.active_len as usize {
                    self.active_len = n as u32;
                    if n > 0 {
                        self.gpu.queue.write_buffer(
                            &self.active,
                            0,
                            bytemuck::cast_slice(&self.active_host[..n]),
                        );
                    }
                }
                if n == 0 {
                    stats.converged = true;
                    break;
                }
            } else if !self.flag_host[..self.fields as usize].iter().any(|&f| f != 0) {
                stats.converged = true;
                break;
            }
        }
        stats.compute = t.elapsed();
        stats.field_rounds_uncompacted = (stats.rounds as u64) * (self.fields as u64);
        stats
    }

    /// Read the per-field changed flags into `flag_host`. One stall per poll, exactly as
    /// before — the flag buffer grew from 4 bytes to `4 * fields`, which at 4096 fields is
    /// 16 KiB and is not what the stall costs.
    fn read_flags(&mut self) {
        let slice = self.flags_staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let _ = rx.recv();
        {
            let view = slice.get_mapped_range();
            self.flag_host.copy_from_slice(bytemuck::cast_slice(&view));
        }
        self.flags_staging.unmap();
    }

    /// Fields still being dispatched. Falls to zero at convergence.
    pub fn active_fields(&self) -> u32 {
        self.active_len
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
            slot_offset: 0,
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
