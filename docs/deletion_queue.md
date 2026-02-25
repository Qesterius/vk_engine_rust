# Rust Context: Memory & Ownership

## 1. Manual Cleanup in a Safety-First Language

Rust has no Garbage Collector (GC). Instead, it uses **Ownership**. When a variable's owner goes out of scope, Rust automatically drops it. However, Rust only knows about CPU memory. It cannot "see" GPU resources. Therefore, we must manually tell the GPU to free resources before the Rust handles themselves are dropped.

## 2. The `move` Keyword

In our `DeletionQueue`, we use `move || { ... }` closures.

* **Default Behavior**: Closures "borrow" references to variables.
* **The Problem**: Our `init` function creates variables on the stack. If the closure only *borrows* them, those references become invalid (dangling) as soon as `init` finishes.
* **The Solution**: The `move` keyword forces the closure to **take ownership** of its environment. This ensures the data (like the `logical_device` or `semaphore` handles) lives inside the closure on the heap, staying valid until the `DeletionQueue` is flushed.

## 3. Copy vs. Move Semantics

How `move` affects our variables depends on their type:

* **Handles (Copy)**: Types like `vk::Semaphore` or `vk::Fence` are **Transparent Wrappers** around `u64` integers. They implement the `Copy` trait. When "moved" into a closure, they are actually just bit-copied. You can still use the original handle variable in your `VulkanContext`.
* **Loaders (Non-Copy)**: The `ash::Device` (logical device) is a heavy struct of function pointers. It is **not** `Copy`. We must call `.clone()` to create a new reference-counted handle to the device before moving it into the closure.

---

# Vulkan Resource Cleanup Strategy

## The Problem: Dependency Graphs

Vulkan resources are hierarchical. You cannot destroy a **Parent** (e.g., `vk::Device`) while a **Child** (e.g., `vk::ImageView`) still exists. In a complex engine, the order of creation is often the only way to track the safe order of destruction.

## The Mechanism: Deletion Queue

We use a **First-In, Last-Out (FILO)** stack to automate this. By pushing a "cleanup task" immediately after creating a resource, we ensure the reverse-order destruction required by the Vulkan spec.

### 1. Leaf Objects (Automated)

These are objects created via the Logical Device.

* **Examples**: `vk::ImageView`, `vk::SwapchainKHR`, `vk::Semaphore`.
* **Implementation**: We use `Box<dyn FnOnce()>` to store these on the **Heap**. Since every closure has a unique size and type, `Box<dyn...>` (Dynamic Dispatch) allows us to store them in a single `Vec`.
* **Reverse Execution**: `self.deletors.drain(..).rev()` ensures that if we created `A` then `B`, we destroy `B` then `A`.

### 2. Pillar Objects (Manual)

These are the "roots" of the engine. They must be destroyed manually in the `RenderingState::destroy` function *after* the deletion queue is flushed:

1. **Logical Device**: Must remain alive to execute the `vkDestroy...` calls in the queue.
2. **Surface/Window**: Destroyed after the device is idle.
3. **Instance**: The absolute root; destroyed last.
