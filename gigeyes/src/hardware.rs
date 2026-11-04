//! Which local AI this PC can run alongside Gig Performer, from its graphics card and memory.

/// What GigEyes found on this PC.
#[derive(Clone, Debug, PartialEq)]
pub struct Hardware {
    /// The graphics card with the most dedicated memory, if any.
    pub gpu: Option<Gpu>,
    pub ram_gb: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vendor: Vendor,
    pub vram_gb: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
// Built by the Windows detection code; elsewhere only tests use the other brands.
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

/// How the model runs: llama.cpp's CUDA build on NVIDIA, its Vulkan build on other cards, or the CPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backend {
    Cuda,
    Vulkan,
    Cpu,
}

/// A local model GigEyes can use. All are Qwen3 instruct models from Qwen's own GGUF builds, Apache-2.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    pub name: &'static str,
    pub repo: &'static str,
    pub file: &'static str,
    /// Download size of the file.
    pub size_gb: f32,
}

// Sizes from Hugging Face's file listing. Room for the context (~1 GB) is added when choosing.
pub const QWEN3_14B: Model = Model {
    name: "Qwen3 14B",
    repo: "Qwen/Qwen3-14B-GGUF",
    file: "Qwen3-14B-Q4_K_M.gguf",
    size_gb: 9.00,
};
pub const QWEN3_8B: Model = Model {
    name: "Qwen3 8B",
    repo: "Qwen/Qwen3-8B-GGUF",
    file: "Qwen3-8B-Q4_K_M.gguf",
    size_gb: 5.03,
};
pub const QWEN3_4B: Model = Model {
    name: "Qwen3 4B",
    repo: "Qwen/Qwen3-4B-GGUF",
    file: "Qwen3-4B-Q4_K_M.gguf",
    size_gb: 2.50,
};
pub const QWEN3_1_7B: Model = Model {
    name: "Qwen3 1.7B",
    repo: "Qwen/Qwen3-1.7B-GGUF",
    file: "Qwen3-1.7B-Q8_0.gguf",
    size_gb: 1.83,
};

/// Memory a model needs on top of its file: the context window and runtime buffers.
const CONTEXT_GB: f32 = 1.0;
/// Video memory left for Windows, the desktop and plugin windows.
const VRAM_RESERVE_GB: f32 = 1.5;
/// System memory left for Windows, Gig Performer and its plugins.
const RAM_RESERVE_GB: f32 = 8.0;
/// Graphics cards below this are integrated chips that share system memory: use the CPU path.
const MIN_DEDICATED_GB: f32 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// None when the PC can't spare the memory: research then shows sources without AI reading them.
    pub model: Option<Model>,
    pub backend: Backend,
    /// One sentence a person can read, e.g. "Runs on your GeForce RTX 4070 Laptop GPU (8 GB)…".
    pub reason: String,
}

/// The biggest model that fits after leaving room for Gig Performer, on the GPU when there's one worth using.
pub fn choose(hw: &Hardware) -> Choice {
    let fits = |model: &Model, budget: f32| model.size_gb + CONTEXT_GB <= budget;
    if let Some(gpu) = hw.gpu.as_ref().filter(|g| g.vram_gb >= MIN_DEDICATED_GB) {
        let budget = gpu.vram_gb - VRAM_RESERVE_GB;
        if let Some(model) = [QWEN3_14B, QWEN3_8B, QWEN3_4B, QWEN3_1_7B]
            .into_iter()
            .find(|m| fits(m, budget))
        {
            let backend = if gpu.vendor == Vendor::Nvidia {
                Backend::Cuda
            } else {
                Backend::Vulkan
            };
            return Choice {
                model: Some(model),
                backend,
                reason: format!(
                    "{} runs on your {} ({:.0} GB), leaving {:.1} GB of it free.",
                    model.name,
                    gpu.name,
                    gpu.vram_gb,
                    gpu.vram_gb - model.size_gb - CONTEXT_GB
                ),
            };
        }
    }
    // On the CPU, bigger models are too slow to wait for, so stop at 4B.
    let budget = hw.ram_gb - RAM_RESERVE_GB;
    let model = [QWEN3_4B, QWEN3_1_7B].into_iter().find(|m| fits(m, budget));
    let reason = match model {
        Some(m) => format!(
            "{} runs on the processor, using {:.1} GB of your {:.0} GB of memory. A graphics card with 4 GB or more would make it faster.",
            m.name,
            m.size_gb + CONTEXT_GB,
            hw.ram_gb
        ),
        None => format!(
            "With {:.0} GB of memory there isn't room for a local AI next to Gig Performer. Research still lists its sources.",
            hw.ram_gb
        ),
    };
    Choice {
        model,
        backend: Backend::Cpu,
        reason,
    }
}

/// Read the graphics cards and memory of this PC.
#[cfg(windows)]
pub fn detect() -> Hardware {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    const GB: f32 = 1024.0 * 1024.0 * 1024.0;

    let mut memory = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `memory` is a valid MEMORYSTATUSEX with dwLength set, as the API requires.
    let ram_gb = if unsafe { GlobalMemoryStatusEx(&mut memory) }.is_ok() {
        memory.ullTotalPhys as f32 / GB
    } else {
        0.0
    };

    let mut gpus = vec![];
    // SAFETY: plain COM calls; every returned interface is owned and released by the windows crate.
    if let Ok(factory) = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
        for i in 0.. {
            let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
                break;
            };
            let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue; // Microsoft's software renderer, not a card
            }
            let end = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            gpus.push(Gpu {
                name: String::from_utf16_lossy(&desc.Description[..end])
                    .trim()
                    .to_string(),
                vendor: match desc.VendorId {
                    0x10DE => Vendor::Nvidia,
                    0x1002 => Vendor::Amd,
                    0x8086 => Vendor::Intel,
                    _ => Vendor::Other,
                },
                vram_gb: desc.DedicatedVideoMemory as f32 / GB,
            });
        }
    }
    let gpu = gpus
        .into_iter()
        .max_by(|a, b| a.vram_gb.total_cmp(&b.vram_gb));
    Hardware { gpu, ram_gb }
}

/// Outside Windows (development only): memory from /proc/meminfo, no graphics card.
#[cfg(not(windows))]
pub fn detect() -> Hardware {
    let ram_kb: f32 = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|m| {
            m.lines()
                .find(|l| l.starts_with("MemTotal:"))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()
        })
        .unwrap_or(0.0);
    Hardware {
        gpu: None,
        ram_gb: ram_kb / 1024.0 / 1024.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(vendor: Vendor, vram_gb: f32, ram_gb: f32) -> Hardware {
        Hardware {
            gpu: Some(Gpu {
                name: "Test GPU".into(),
                vendor,
                vram_gb,
            }),
            ram_gb,
        }
    }

    #[test]
    fn picks_the_biggest_model_that_leaves_room_for_gig_performer() {
        // The user's laptop: RTX 4070 Laptop, 8 GB video memory, 16 GB RAM.
        let laptop = choose(&pc(Vendor::Nvidia, 8.0, 16.0));
        assert_eq!(
            (laptop.model, laptop.backend),
            (Some(QWEN3_8B), Backend::Cuda)
        );
        assert_eq!(
            choose(&pc(Vendor::Nvidia, 12.0, 32.0)).model,
            Some(QWEN3_14B)
        );
        assert_eq!(choose(&pc(Vendor::Nvidia, 6.0, 16.0)).model, Some(QWEN3_4B));
        // 4 GB minus the reserve leaves 2.5 GB, under even 1.7B plus context: the processor path wins.
        let small = choose(&pc(Vendor::Nvidia, 4.0, 16.0));
        assert_eq!((small.model, small.backend), (Some(QWEN3_4B), Backend::Cpu));
        let amd = choose(&pc(Vendor::Amd, 16.0, 32.0));
        assert_eq!((amd.model, amd.backend), (Some(QWEN3_14B), Backend::Vulkan));
    }

    #[test]
    fn falls_back_to_the_processor_and_says_why() {
        let no_gpu = Hardware {
            gpu: None,
            ram_gb: 16.0,
        };
        assert_eq!(
            (choose(&no_gpu).model, choose(&no_gpu).backend),
            (Some(QWEN3_4B), Backend::Cpu)
        );
        // An integrated chip with a sliver of dedicated memory is no better than the CPU path.
        assert_eq!(choose(&pc(Vendor::Intel, 0.5, 32.0)).backend, Backend::Cpu);
        assert_eq!(
            choose(&Hardware {
                gpu: None,
                ram_gb: 11.0
            })
            .model,
            Some(QWEN3_1_7B)
        );
        let tight = choose(&Hardware {
            gpu: None,
            ram_gb: 8.0,
        });
        assert_eq!(tight.model, None);
        assert!(tight.reason.contains("isn't room"), "{}", tight.reason);
    }

    #[test]
    fn detect_reads_some_memory() {
        assert!(detect().ram_gb > 0.5);
    }
}
