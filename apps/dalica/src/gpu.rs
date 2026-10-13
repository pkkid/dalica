//! Which GPU backend draws the window (wgpu, through eframe).
//!
//! eframe asks wgpu for every primary backend the build contains. On Windows that is Vulkan and
//! DX12, and wgpu creates the Vulkan instance first and keeps that order among adapters of the same
//! kind, so the same GPU is reached through its Vulkan driver whenever one is installed. Vulkan
//! presentation on Windows is the driver's own path past the desktop compositor, and on some
//! drivers the window flickers on every repaint; egui repaints on every pointer move, so the whole
//! window flickered while the mouse moved over it (the 0.1.0 report). DX12 presents through DXGI,
//! the path the compositor is built for, so Windows uses DX12 alone. The DX12 backend always offers
//! at least the WARP software adapter, so the window still opens on a machine without a GPU driver.
//!
//! DX12 shaders are compiled with FXC, which ships with Windows: wgpu's default looks for a
//! `dxcompiler.dll` on the DLL search path first, and another program's copy there can fail the
//! device or the first shader.
//!
//! `WGPU_BACKEND` (`dx12`, `vulkan`, `metal`, comma lists) and `WGPU_DX12_COMPILER` (`fxc`, `dxc`,
//! `staticdxc`, `auto`) still choose, on every OS. Elsewhere nothing changes: macOS uses Metal and
//! Linux Vulkan, as eframe picks them.

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use wgpu::{Backends, Dx12Compiler};

/// The OS family, as far as the window's GPU backend cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    /// macOS and Linux.
    Other,
}

impl Platform {
    /// The OS this build runs on.
    pub fn host() -> Platform {
        if cfg!(target_os = "windows") { Platform::Windows } else { Platform::Other }
    }
}

/// The backends the window may use on `platform`: what `WGPU_BACKEND` (`wgpu_backend`) names, if
/// it names any; else DX12 alone on Windows and eframe's own choice elsewhere.
pub fn backends(platform: Platform, wgpu_backend: Option<&str>) -> Backends {
    if let Some(v) = wgpu_backend {
        let asked = Backends::from_comma_list(v);
        if !asked.is_empty() {
            return asked;
        }
        log::warn!("dalica: WGPU_BACKEND={v} names no backend; the default is used");
    }
    match platform {
        Platform::Windows => Backends::DX12,
        Platform::Other => Backends::PRIMARY | Backends::GL,
    }
}

/// The DX12 shader compiler: what `WGPU_DX12_COMPILER` (`env`) names, else FXC.
pub fn dx12_compiler(env: Option<&str>) -> Dx12Compiler {
    match env.map(str::parse::<Dx12Compiler>) {
        Some(Ok(c)) => c,
        Some(Err(why)) => {
            log::warn!("dalica: WGPU_DX12_COMPILER: {why}; FXC is used");
            Dx12Compiler::Fxc
        }
        None => Dx12Compiler::Fxc,
    }
}

/// eframe's window renderer settings with the backends and DX12 compiler above; everything else
/// (the surface's present mode and frame latency included) stays eframe's.
pub fn options(platform: Platform, wgpu_backend: Option<&str>, dx12: Option<&str>) -> WgpuConfiguration {
    let mut c = eframe::NativeOptions::default().wgpu_options;
    if let WgpuSetup::CreateNew(n) = &mut c.wgpu_setup {
        n.instance_descriptor.backends = backends(platform, wgpu_backend);
        n.instance_descriptor.backend_options.dx12.shader_compiler = dx12_compiler(dx12);
    }
    c
}

/// [`options`] for this OS and environment.
pub fn host_options() -> WgpuConfiguration {
    let var = |name: &str| std::env::var(name).ok();
    options(Platform::host(), var("WGPU_BACKEND").as_deref(), var("WGPU_DX12_COMPILER").as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(c: &WgpuConfiguration) -> (Backends, Dx12Compiler) {
        match &c.wgpu_setup {
            WgpuSetup::CreateNew(n) => (n.instance_descriptor.backends, n.instance_descriptor.backend_options.dx12.shader_compiler.clone()),
            WgpuSetup::Existing(_) => panic!("the window creates its own device"),
        }
    }

    #[test]
    fn windows_draws_through_dx12_alone() {
        // With Vulkan in the set, wgpu picks the Vulkan driver of the same GPU (it is created first),
        // and Vulkan presentation flickered the window on every repaint.
        assert_eq!(backends(Platform::Windows, None), Backends::DX12);
        let (b, compiler) = created(&options(Platform::Windows, None, None));
        assert_eq!(b, Backends::DX12);
        assert!(!b.contains(Backends::VULKAN));
        assert!(matches!(compiler, Dx12Compiler::Fxc), "{compiler:?}");
    }

    #[test]
    fn other_platforms_keep_eframes_choice() {
        assert_eq!(backends(Platform::Other, None), Backends::PRIMARY | Backends::GL);
        assert!(backends(Platform::Other, None).contains(Backends::VULKAN | Backends::METAL));
    }

    #[test]
    fn wgpu_backend_still_chooses() {
        assert_eq!(backends(Platform::Windows, Some("vulkan")), Backends::VULKAN);
        assert_eq!(backends(Platform::Windows, Some("Vulkan, DX12")), Backends::VULKAN | Backends::DX12);
        assert_eq!(backends(Platform::Other, Some("metal")), Backends::METAL);
        // Nothing it knows: the default, not an empty set that opens no window.
        assert_eq!(backends(Platform::Windows, Some("")), Backends::DX12);
        assert_eq!(backends(Platform::Windows, Some("bogus")), Backends::DX12);
        assert_eq!(backends(Platform::Other, Some("auto")), Backends::PRIMARY | Backends::GL);
    }

    #[test]
    fn the_dx12_compiler_is_fxc_unless_asked() {
        assert!(matches!(dx12_compiler(None), Dx12Compiler::Fxc));
        assert!(matches!(dx12_compiler(Some("nonsense")), Dx12Compiler::Fxc));
        assert!(matches!(dx12_compiler(Some("StaticDxc")), Dx12Compiler::StaticDxc));
        assert!(matches!(dx12_compiler(Some("dxc")), Dx12Compiler::DynamicDxc { .. }));
        assert!(matches!(dx12_compiler(Some("auto")), Dx12Compiler::Auto));
    }

    #[test]
    fn the_surface_stays_eframes() {
        // Only the backends and the compiler change: the present mode and frame latency are eframe's.
        let ours = options(Platform::Windows, None, None);
        assert_eq!(ours.surface, eframe::NativeOptions::default().wgpu_options.surface);
    }
}
