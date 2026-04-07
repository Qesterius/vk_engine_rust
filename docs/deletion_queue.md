# Vulkan Resource Cleanup: Deletion Queues

## 1. The Problem

Rust's `Drop` runs immediately when a value goes out of scope. For Vulkan resources (buffers, images, pipelines) this is dangerous because the GPU may still be reading them. We cannot destroy a buffer that an in-flight command buffer is referencing.

We also cannot destroy child resources (pipeline, image view) before the parent they depend on (device, render pass).

Both problems are solved by deferring destruction through deletion queues.

---

## 2. Two Queues, Two Lifetimes

`Device` owns two kinds of queues:

```rust
pub static_deletion_queue: Mutex<DeletionQueue>
pub dynamic_deletion_queues: Vec<Mutex<DeletionQueue>>  // one per frame-in-flight slot
```

### Static Queue

For resources that live the entire engine lifetime. Created once, destroyed once at shutdown. It could be handled purely by Drop, but I wanted to keep it cohesive with dynamic and I liked this pattern. It might to prove useful in future where it could be required to be this way. 

| Examples | Why static |
|---|---|
| `VkCommandPool` | Created in `Device::new`, lives forever |
| Any one-time setup resource | Never destroyed at runtime |

**Flush point**: `Device::drop`, after `device_wait_idle`. Flushed in reverse push order (LIFO) so children are always destroyed before parents.


### Dynamic Queues

For resources that can be created and destroyed at runtime — mesh buffers, staging buffers, any per-entity GPU data. There is one queue per frame-in-flight slot (`MAX_FRAMES_IN_FLIGHT`).

This implementation has few important features:
1. It controls when vulkan data is freed, avoiding use after free with synchronizing it with frames in flight
2. allows to free memory for resources used by entities we remove from scene as soon as they become unused
3. keeps only arc<> to device, so also doesnt have its memory overhead and prevents dropping device until all its dynamic resources are freed


| Examples | Why dynamic |
|---|---|
| `Mesh` vertex + index buffers | Entities can be despawned at runtime |
| Staging buffers | Temporary, used for one upload |

**Flush point**: The start of the next frame that reuses slot N, right after `wait_for_fences[N]`. The fence wait *is* the guarantee — it proves the GPU finished all work submitted last time this slot was used.

---

## 3. The Core Invariant

Slot N's dynamic queue is flushed only after `wait_for_fences[N]` returns. At that point the GPU has finished every command buffer submitted in the previous use of slot N. Those commands cannot be referencing the queued resources anymore.

>Frames in flight are 3, but you can see example here assuming like there'd be 2

```
Frame 0 → slot 0: submit work
Frame 1 → slot 1: submit work
Frame 2 → slot 0: wait_for_fences[0]   ← GPU finished frame 0
                  store current_sync_idx = 0 on Device
                  flush dynamic_deletion_queues[0] ← safe: GPU is done
                  ... record and submit new work for frame 2 ...
```

---

## 4. `current_sync_idx` — Bridging Drop and the Render Loop

`Mesh::Drop` runs on the CPU side, potentially mid-frame. It has no access to the ECS world — only to `self.device: Arc<Device>`. It needs to know which dynamic queue to push into.

`Device` holds:
```rust
pub current_sync_idx: AtomicUsize
```

`RenderingManager::render()` updates this immediately after the fence wait, before recording any commands:
```rust
unsafe { device.wait_for_fences(&[self.in_flight_fences[sync_idx]], true, u64::MAX) }?;

self.device.current_sync_idx.store(sync_idx, Ordering::Relaxed);
self.device.flush_dynamic_deletion_queue(sync_idx);
```

`Mesh::Drop` reads it:
```rust
let slot = self.device.current_sync_idx.load(Ordering::Relaxed);
self.device.dynamic_deletion_queues[slot].lock().unwrap().push(move || unsafe {
    d.destroy_buffer(vb, None);
    // ...
});
```

At runtime, `slot` is the slot whose fence was just waited on — the safest slot to defer into, because it won't be reused until the next full cycle, and when it is, the fence wait will fire again before the flush.

---

## 5. Shutdown Sequence

```
App::exiting()
  → engine.shutdown()                   // device_wait_idle — GPU fully idle
  → self.engine.take()                  // world drops
      → Mesh::Drop fires for each entity
          → pushes to dynamic_deletion_queues[current_sync_idx]
          (any slot is safe: GPU is idle)
      → RenderingManager::Drop fires
          → device_wait_idle (again, harmless)
          → destroys pipeline, semaphores, fences, uniform buffers
  → Device::Drop fires
      → device_wait_idle (harmless)
      → flush all dynamic_deletion_queues  // mesh buffers destroyed here
      → flush static_deletion_queue        // command pool destroyed here
      → destroy_device()
  → RenderingInstance::Drop fires
      → destroy_debug_utils_messenger
      → destroy_instance
```

The `engine.shutdown()` call before `engine.take()` is essential. Without it, mesh `Drop` may fire before the GPU is idle (bevy_ecs world drop order for resources vs components is not guaranteed).

---

## 6. Future: Asset Manager Integration

When the asset manager is implemented, runtime unloading should explicitly push to the dynamic queue *before* the mesh entity is removed from the world. The pattern:

```rust
// In an asset unload system:
fn unload_mesh(world: &mut World, entity: Entity) {
    if let Some(mesh) = world.get::<Mesh>(entity) {
        let slot = mesh.device.current_sync_idx.load(Ordering::Relaxed);
        // Push explicit destruction into the correct slot
        let d  = mesh.device.logical_device.clone();
        let vb = mesh.vertex_buffer;
        // ...
        mesh.device.dynamic_deletion_queues[slot].lock().unwrap().push(move || unsafe {
            d.destroy_buffer(vb, None);
            // ...
        });
    }
    // Now despawn — Mesh::Drop fires but buffers are already queued,
    // so double-push is a concern. The asset manager should take ownership
    // of buffer handles before despawning to prevent this.
    world.despawn(entity);
}
```

The exact pattern (taking handles out of Mesh before despawn vs. a flag to skip Drop) can be decided when the asset manager is built.

---

## 7. Rule Summary

| Resource type | Queue to use | When flushed |
|---|---|---|
| Engine-lifetime (command pool, etc.) | `static_deletion_queue` | `Device::drop` after `device_wait_idle` |
| Runtime entity data (mesh buffers, etc.) | `dynamic_deletion_queues[sync_idx]` | Render loop after `wait_for_fences[sync_idx]`, and at shutdown |
| Per-frame infrastructure (UBOs, sync objects) | Direct destruction in `RenderingManager::drop` | Safe: `device_wait_idle` called first in the same `Drop` impl |
| Swapchain-tied (framebuffers, semaphores on resize) | Direct destruction during resize | Safe because resize is done outside render submissions |
| Temporary (shader modules) | Direct destruction after use | Safe because only used during pipeline creation |

> **Why not the dynamic queue for UBOs/sync objects?**
> The dynamic queue exists for resources with *unpredictable* lifetimes — things that can be destroyed mid-session when an entity is despawned, where the GPU may still be referencing them.
> Per-frame infrastructure (uniform buffers, semaphores, fences) has a *known* lifetime: created at renderer init, destroyed at renderer shutdown. `RenderingManager::drop` calls `device_wait_idle` before touching them, so the GPU is provably idle at that exact point. Direct destruction is both correct and simpler here — no queue needed.
