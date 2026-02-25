# Multiexecution & Resource Synchronization

CPU and GPU are essentially separate units of execution. The GPU renders to the screen while the CPU processes logic and pushes draw requests. Because they run at different speeds, we must synchronize them to avoid race conditions. In our engine, synchronization is split into three categories: **CPU-to-GPU**, **GPU-to-GPU**, and **Internal GPU Memory Access**.

## 1. Synchronization Primitives

### Fences (`ash::vk::Fence`)

Fences are the **"CPU Brake."** They allow the GPU to signal the CPU when a task is complete. We use these to stop the CPU from "overrunning" the GPU. When the CPU waits on a fence, it enters a sleep state until a hardware signal from the GPU interrupts it.

* **Scope:** GPU $\rightarrow$ CPU.

### Semaphores (`ash::vk::Semaphore`)

Semaphores synchronize work **entirely inside the GPU**. They are "tokens" passed between different GPU queues or the Presentation Engine. The CPU never "waits" on these; it simply tells the GPU: "Don't start Task B until Task A signals this semaphore."

* **Scope:** GPU $\rightarrow$ GPU / OS.

### Pipeline Barriers (`ash::vk::ImageMemoryBarrier`)

While fences and semaphores synchronize **execution**, Barriers synchronize **memory**. Modern GPUs are highly parallel; a barrier tells the GPU hardware to flush caches and change how it "interprets" a block of memory (e.g., shifting an image from "Generic Memory" to "Ready for Clear Color").

---

## 2. The Three-Way Handshake

Our engine uses a fixed number of **Frame Slots** (e.g., 2) to manage CPU work, while the number of **Swapchain Images** is determined dynamically by the hardware (usually `min_image_count + 1`).

### A. The Execution Sync (Frame Based)

We use a fixed number of objects based on `MAX_FRAMES_IN_FLIGHT` to manage the CPU's work-ahead.

* **`in_flight_fence`**: Ensures the CPU doesn't record into a Command Buffer that the GPU is currently reading.
* **`image_available_semaphore`**: The OS signals this when it has physically released a swapchain image. The GPU waits on this before it starts its clear/render commands.

### B. The Ownership Sync (Image Based)

The Monitor often holds an image longer than the GPU takes to render. To handle this, we tie specific synchronization objects to the **Image Index** returned by the swapchain, not the Frame Slot.

* **`rendering_finished_semaphore`**: One per swapchain image. The GPU signals this when rendering is complete. The Monitor waits on it before displaying the image.
* **`images_in_flight_fences`**: A mapping array that tracks which `in_flight_fence` is currently protecting which physical image. This ensures the CPU won't reuse a piece of memory that is still being scanned out by the monitor.

---

## 3. Order of Operations (The Render Loop)

1. **Fence Wait:** CPU waits for `in_flight_fence[slot]`. (Wait for the GPU to finish the "slot").
2. **Acquisition:** CPU calls `acquire_next_image`. It receives an `image_index` and assigns an `image_available_semaphore`.
3. **Resource Check:** CPU checks `images_in_flight[image_index]`. If a fence is mapped to this image, it waits. This bridges the gap between the **Monitor** and the CPU.
4. **Barrier 1:** GPU transitions the image to `TRANSFER_DST_OPTIMAL`. (Prepares memory for writing).
5. **Submission:** * **Wait:** `image_available_semaphore`.
* **Signal:** `rendering_finished_semaphore[image_index]`.
* **Signal:** `in_flight_fence[slot]`.


6. **Barrier 2:** GPU transitions the image to `PRESENT_SRC_KHR`. (Prepares memory for the monitor).
7. **Present:** Monitor waits for `rendering_finished_semaphore[image_index]` then displays the pixels.

---

### Why we use Image-Indexed Semaphores

On high-performance hardware, the CPU can loop through its available Frame Slots faster than the Monitor's refresh rate. If we tied the "Finished" semaphore to the Frame Slot, we would risk "double-signaling" a semaphore that the Monitor is still using. By using an index-based semaphore, every physical image has its own signal flag, ensuring the Monitor and GPU never collide on the same sync object.

