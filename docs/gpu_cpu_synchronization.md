# Multiexecution & Resource Synchronization

## 1. The Core Shift: Why `MAX_FRAMES_IN_FLIGHT`?

In our previous logic, we tried to tie semaphores to the **Swapchain Image Index**. We have moved away from this because the number of swapchain images can change (e.g., from 2 to 3) during a resize, but our **Processing Slots** (Frames in Flight) are constant.

We now treat synchronization as **"Pipelines of Work."** If `MAX_FRAMES_IN_FLIGHT` is 2, we have two "conveyor belts." Each belt has its own Fence, its own "Image Available" Semaphore, and its own "Render Finished" Semaphore.

### Why this works:
A Semaphore is just a GPU-side signal. When we tell the GPU to "Wait for Image Available" and then "Signal Render Finished," it doesn't matter which physical image is being used. The GPU just needs to know that **Slot A** is busy. By the time the CPU tries to reuse **Slot A** (2 frames later), the `in_flight_fence` ensures that the previous work—including the semaphore signals—is 100% complete.

---

## 2. The Updated Synchronization Setup

### A. Frame-Based Objects (`MAX_FRAMES_IN_FLIGHT`)
These are the "Permanent" sync objects stored in `RenderingState`.
* **`image_available_semaphores`**: GPU waits here. Signals when the OS gives us *any* image to draw on.
* **`rendering_finished_semaphores`**: OS/Present waits here. Signals when the GPU is done drawing to *that* image.
* **`frame_in_flight_fences`**: CPU waits here. Signals when the entire GPU "Slot" (Command Buffer + Semaphores) is done.

### B. Image-Based Mapping (`swapchain_image_count`)
* **`images_in_flight_fences`**: This is **not** a collection of unique fences. It is a **List of Pointers (References)**. 
    * If `image_index` 0 is being rendered by `sync_frame_index` 1, then `images_in_flight_fences[0]` points to `frame_in_flight_fences[1]`.
    * This bridges the gap: it tells the CPU "Don't start a new frame on Image 0 if the Fence for the frame currently using Image 0 hasn't signaled yet."



---

## 3. The New Order of Operations (The Render Loop)

1.  **CPU Slot Wait:** `device.wait_for_fences([frame_in_flight_fences[sync_frame_index]])`. 
    * *Effect:* The CPU stops until the "Conveyor Belt" slot is empty.
2.  **Acquisition:** `acquire_next_image`. 
    * *Input:* `image_available_semaphores[sync_frame_index]`. 
    * *Output:* `image_index`.
3.  **The Image Shield:** Check `images_in_flight_fences[image_index]`. 
    * If it's not null, the CPU waits for *that* fence. This ensures we don't accidentally start writing to a physical image that is still being presented by a *different* frame slot.
4.  **The Link:** `images_in_flight_fences[image_index] = frame_in_flight_fences[sync_frame_index]`. 
    * *Effect:* We "lock" this physical image to this frame slot's fence.
5.  **Submission:**
    * **Wait:** `image_available_semaphores[sync_frame_index]`.
    * **Signal:** `rendering_finished_semaphores[sync_frame_index]`.
    * **Fence:** `frame_in_flight_fences[sync_frame_index]`.
6.  **Presentation:**
    * **Wait:** `rendering_finished_semaphores[sync_frame_index]`.
    * *Effect:* The Monitor waits until the GPU signals it's done before flipping the pixels.
