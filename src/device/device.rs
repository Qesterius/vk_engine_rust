use anyhow::{Context, Result, anyhow};
use ash::vk;
use log::{info, warn};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::MAX_FRAMES_IN_FLIGHT;
use crate::rendering::{
    cleanup::DeletionQueue, rendering_canvas::swapchain::SwapchainSupportDetails,
    rendering_instance::RenderingInstance,
};
use crate::utils;

const DEVICE_EXTENSIONS: &[&std::ffi::CStr] = &[ash::khr::swapchain::NAME];

pub struct Device {
    pub logical_device: ash::Device,
    pub physical_device: vk::PhysicalDevice,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub graphics_queue: vk::Queue,
    pub present_queue: vk::Queue,
    pub queue_family_indices: QueueFamilyIndices,
    pub command_pool: vk::CommandPool,
    /// The frame-in-flight slot index currently being rendered.
    /// Drop functions cant have extra arguments so it cannot be a resource
    /// its used to register resource cleanup functions in the correct dynamic deletion queue.
    pub current_sync_idx: AtomicUsize,
    /// Lifetime-of-engine resources (command pool, etc.).
    pub static_deletion_queue: Mutex<DeletionQueue>,
    /// Per-frame-in-flight queues for resources destroyed at runtime (mesh buffers etc.).
    /// Slot N is flushed at the start of the next frame that reuses slot N, right after
    /// the fence wait — guaranteeing the GPU is no longer referencing those resources.
    pub dynamic_deletion_queues: Vec<Mutex<DeletionQueue>>,
}

impl Device {
    pub fn new(
        rendering_instance: &RenderingInstance,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
    ) -> Result<Arc<Self>> {
        let instance = &rendering_instance.instance;

        let (physical_device, queue_family_indices) =
            unsafe { pick_physical_device(instance, surface, surface_loader) }?;

        let (logical_device, graphics_queue, present_queue) =
            create_logical_device(instance, physical_device, &queue_family_indices)?;

        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(physical_device) };

        let mut dynamic_deletion_queues = Vec::new();
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            let mut q = Mutex::new(DeletionQueue::new());
            q.lock()
                .unwrap()
                .push(|| info!("Flushed unused resources from previous frame"));
            dynamic_deletion_queues.push(q);
        }
        let mut static_deletion_queue = Mutex::new(DeletionQueue::new());
        static_deletion_queue
            .lock()
            .unwrap()
            .push(|| info!("Flushed all static vulkan resources"));

        let command_pool = setup_command_pool(
            &logical_device,
            &queue_family_indices,
            &mut static_deletion_queue.lock().unwrap(),
        )?;

        Ok(Arc::new(Self {
            logical_device,
            physical_device,
            memory_properties,
            graphics_queue,
            present_queue,
            queue_family_indices,
            command_pool,
            current_sync_idx: AtomicUsize::new(0),
            static_deletion_queue,
            dynamic_deletion_queues,
        }))
    }

    pub fn flush_dynamic_deletion_queue(&self, frame_index: usize) {
        self.dynamic_deletion_queues[frame_index]
            .lock()
            .unwrap()
            .flush();
    }
    pub fn flush_static_deletion_queue(&self) {
        self.static_deletion_queue.lock().unwrap().flush();
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            // Ensure the GPU is idle before destroying resources
            let _ = self.logical_device.device_wait_idle();

            // Flush deletion queue (CommandPool, etc.)
            for i in 0..MAX_FRAMES_IN_FLIGHT {
                self.flush_dynamic_deletion_queue(i);
            }
            self.flush_static_deletion_queue();

            // Finally, destroy the device
            self.logical_device.destroy_device(None);
            info!("Vulkan Device destroyed.");
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct QueueFamilyIndices {
    pub graphics: u32,
    pub present: u32,
}

impl QueueFamilyIndices {
    pub unsafe fn get(
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
    ) -> Result<Self> {
        let properties =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

        let mut graphics = None;
        let mut present = None;

        for (index, info) in properties.iter().enumerate() {
            let i = index as u32;
            if info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                graphics = Some(i);
            }
            if unsafe {
                surface_loader.get_physical_device_surface_support(physical_device, i, surface)?
            } {
                present = Some(i);
            }
            if let (Some(g), Some(p)) = (graphics, present) {
                return Ok(Self {
                    graphics: g,
                    present: p,
                });
            }
        }
        Err(anyhow!("Device does not support required queue families!"))
    }
}

unsafe fn check_physical_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    surface_loader: &ash::khr::surface::Instance,
) -> Result<QueueFamilyIndices> {
    let indices =
        unsafe { QueueFamilyIndices::get(instance, physical_device, surface, surface_loader)? };
    unsafe { check_device_extension_support(instance, physical_device)? };

    let details =
        unsafe { SwapchainSupportDetails::get(physical_device, surface, surface_loader)? };
    if details.formats.is_empty() || details.present_modes.is_empty() {
        return Err(anyhow!(
            "GPU supports Swapchain, but has no compatible formats/modes"
        ));
    }
    Ok(indices)
}

unsafe fn check_device_extension_support(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
) -> Result<()> {
    let available_extensions =
        unsafe { instance.enumerate_device_extension_properties(physical_device)? };
    let available_names: HashSet<String> = available_extensions
        .iter()
        .map(|ext| {
            utils::vk_to_cstr(&ext.extension_name)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    for &extension in DEVICE_EXTENSIONS {
        let name = extension.to_string_lossy().into_owned();
        if !available_names.contains(&name) {
            return Err(anyhow!("Missing required extension: {}", name));
        }
    }
    Ok(())
}

unsafe fn pick_physical_device(
    instance: &ash::Instance,
    surface: vk::SurfaceKHR,
    surface_loader: &ash::khr::surface::Instance,
) -> Result<(vk::PhysicalDevice, QueueFamilyIndices)> {
    let physical_devices = unsafe { instance.enumerate_physical_devices()? };
    for &physical_device in physical_devices.iter() {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let name = utils::vk_to_cstr(&properties.device_name);

        match unsafe { check_physical_device(instance, physical_device, surface, surface_loader) } {
            Ok(indices) => {
                info!("Selected GPU: {:?}", name);
                return Ok((physical_device, indices));
            }
            Err(e) => {
                warn!("Skipping GPU {:?}: {}", name, e);
            }
        }
    }
    Err(anyhow!("Failed to pick any suitable device!"))
}

fn create_logical_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    indices: &QueueFamilyIndices,
) -> Result<(ash::Device, vk::Queue, vk::Queue)> {
    let mut unique_indices = HashSet::new();
    unique_indices.insert(indices.graphics);
    unique_indices.insert(indices.present);

    let queue_priorities = [1.0_f32];
    let queue_create_infos: Vec<vk::DeviceQueueCreateInfo> = unique_indices
        .iter()
        .map(|&i| {
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(i)
                .queue_priorities(&queue_priorities)
        })
        .collect();

    let mut extensions_names_ptr: Vec<*const std::ffi::c_char> =
        DEVICE_EXTENSIONS.iter().map(|ext| ext.as_ptr()).collect();

    // Handling the Portability Subset (required for macOS/MoltenVK)
    let available_extensions =
        unsafe { instance.enumerate_device_extension_properties(physical_device)? };
    let portability_supported = available_extensions
        .iter()
        .any(|ext| utils::vk_to_cstr(&ext.extension_name) == vk::KHR_PORTABILITY_SUBSET_NAME);

    if portability_supported {
        info!("Adding Portability Subset extension.");
        extensions_names_ptr.push(vk::KHR_PORTABILITY_SUBSET_NAME.as_ptr());
    }

    let features = vk::PhysicalDeviceFeatures::default();
    let info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_features(&features)
        .enabled_extension_names(&extensions_names_ptr);

    let device = unsafe {
        instance
            .create_device(physical_device, &info, None)
            .context("Failed to create logical device")?
    };

    let graphics_queue = unsafe { device.get_device_queue(indices.graphics, 0) };
    let present_queue = unsafe { device.get_device_queue(indices.present, 0) };

    Ok((device, graphics_queue, present_queue))
}

fn setup_command_pool(
    logical_device: &ash::Device,
    indices: &QueueFamilyIndices,
    deletion_queue: &mut DeletionQueue,
) -> Result<vk::CommandPool> {
    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
        .queue_family_index(indices.graphics);

    let command_pool = unsafe {
        logical_device
            .create_command_pool(&pool_info, None)
            .context("Failed to create command pool")?
    };

    let logical_device_clone = logical_device.clone();
    deletion_queue.push(move || unsafe {
        logical_device_clone.destroy_command_pool(command_pool, None);
        info!("Command Pool destroyed.");
    });

    Ok(command_pool)
}
