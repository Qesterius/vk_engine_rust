# Asset Manager

## Glossary

**Descriptor** — a GPU-side handle pointing to a resource (image, buffer, sampler); tells the shader where in memory to find it.  
**Descriptor Set** — a grouped collection of descriptors bound to the pipeline as a unit.  
**Descriptor Set Layout** — a schema declaring what descriptor types and counts occupy each binding slot; created once and reused.  
**Descriptor Pool** — a fixed-capacity heap from which descriptor sets are allocated.  
**Binding** — a numbered slot within a descriptor set layout, matched to `layout(binding = N)` in GLSL.  
**Set Index** — which descriptor set slot in the pipeline layout a set is bound to, matched to `layout(set = N)` in GLSL.  
**Sampler** — a Vulkan object encoding how the GPU reads from an image: filter mode, address mode, anisotropy, mip settings.  
**Image / Image View** — `vk::Image` is raw GPU memory holding pixel data; `vk::ImageView` is a typed window into it that shaders reference.  
**Push Constants** — a small register block (≥128 bytes) updated per draw via a single command; no descriptor or memory mapping involved.  
**Pipeline Layout** — declares which descriptor set layouts and push constant ranges a pipeline can access.  
**Bindless** — a pattern where all resources of a type live in one large descriptor array; shaders select which to use by indexing rather than by rebinding.

---

## Overview

The asset manager loads textures from disk onto the GPU and exposes them to shaders through Vulkan's descriptor system. In Vulkan, shaders cannot access GPU resources directly — the CPU must declare every resource through a descriptor set before a draw call. The naive model creates one descriptor set per texture and rebinds it per draw; this scales poorly with object count.

The driving constraint is that descriptor binding is expensive relative to push constant updates. The design eliminates per-draw descriptor work by placing all textures in one shared array descriptor (bindless), bound once per frame. Each draw call selects its texture by pushing an integer index. Samplers are stored in a parallel small array; the shader combines image and sampler at sample time.

Vulkan 1.0 supports dynamic indexing of image arrays via the core feature `shaderSampledImageArrayDynamicIndexing` — no extensions required. The index must be **uniform** across a draw call (same for all invocations). Non-uniform indexing (e.g. ray tracing, virtual textures) requires `VK_EXT_descriptor_indexing` / Vulkan 1.2 `descriptorIndexing`.

---

## Core Components

### Texture Array

`VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE` array at set 1, binding 0. Fixed capacity `MAX_TEXTURES = 1024`, declared at layout creation time.

All slots are pre-filled at init with a 1×1 white `RGBA8` placeholder image, keeping every slot valid without requiring the `PARTIALLY_BOUND` flag from `VK_EXT_descriptor_indexing`. All slot indices are pre-populated into a `free_slots: Vec<u32>`. `load_texture` pops a slot; `unload_texture` returns it via deferred reclaim. Each write uses `vkUpdateDescriptorSets` with `dstArrayElement = slot` — a partial update that touches only one array entry.

**Alternatives:**

| Approach | Pros | Cons |
|---|---|---|
| One descriptor set per texture *(baseline)* | Simple | `vkCmdBindDescriptorSets` per draw; pool grows with texture count |
| Texture atlas | Zero descriptor switching | Non-trivial packing; mip generation complexity; sparse memory waste |
| Bindless array *(used)* | Bind once per frame; no extensions needed for uniform indexing | Array size fixed at layout creation; variable-count requires `VK_EXT_descriptor_indexing` |

**Limitations:** the array size is fixed at layout creation time. Exceeding `MAX_TEXTURES` is a hard error. For variable-size arrays, `VK_EXT_descriptor_indexing` with `VARIABLE_DESCRIPTOR_COUNT` would be needed.

---

### Sampler Pool

`VK_DESCRIPTOR_TYPE_SAMPLER` array at set 1, binding 1. Three fixed entries initialized at startup.

A Vulkan sampler is a distinct object from an image. It encodes filter mode, address mode, anisotropy, mip bias, and comparison op. `COMBINED_IMAGE_SAMPLER` bundles both into one descriptor slot (OpenGL-compatible). Separating them into `SAMPLED_IMAGE` + `SAMPLER` is more idiomatic Vulkan and matches how DirectX 12 and Metal model resources — any image can be sampled with any sampler at draw time without rebinding.

The shader combines them at sample time using the Vulkan GLSL constructor:
```glsl
texture(sampler2D(textures[texture_index], samplers[sampler_index]), uv)
```
This syntax requires `shaderc` to be configured with `set_target_env(TargetEnv::Vulkan, Vulkan1_0)`. Without it, compilation fails — the default target is OpenGL, which does not support separate sampler/texture objects.

| Index | Variant | Filter | Address mode | Anisotropy | Use case |
|---|---|---|---|---|---|
| 0 | `LinearRepeat` | Linear | Repeat | 16× | World geometry, environment textures |
| 1 | `LinearClamp` | Linear | Clamp to edge | 16× | UI, sprite sheets, skybox faces |
| 2 | `NearestClamp` | Nearest | Clamp to edge | off | Pixel art, G-buffer reads |

Shadow map samplers (`compareEnable = true`, `compareOp = LESS`) are a distinct type and belong in shadow pass setup, not this pool.

**Alternatives:**

| Approach | Pros | Cons |
|---|---|---|
| Single sampler *(baseline)* | Trivial | Wrong filtering for pixel art, sprites, shadow reads |
| `COMBINED_IMAGE_SAMPLER` per texture | Texture carries its own sampler | Cannot resample the same image differently per pass; OpenGL-style |
| Separate image array + sampler array *(used)* | Any image × any sampler at draw time; pass-agnostic | Requires Vulkan GLSL mode; two bindings instead of one |

**Limitations:** sampler array size (`N_SAMPLERS = 3`) and texture array size (`MAX_TEXTURES = 1024`) are declared as integer literals in the shader source. There is no compile-time link to the Rust constants — changing either requires manual shader update.

---

### Push Constants

`MeshPushConstants` is the per-draw data block sent via `vkCmdPushConstants`. It lives in a small GPU-side register bank, updated with a single driver command — no descriptor writes, no memory mapping, no explicit sync.

| Field | Type | Offset | Size |
|---|---|---|---|
| `model` | `mat4` | 0 | 64 bytes |
| `texture_index` | `uint` | 64 | 4 bytes |
| `sampler_index` | `uint` | 68 | 4 bytes |
| **Total** | | | **72 bytes** |

Stage flags are `VERTEX | FRAGMENT`. Both stages must declare an identical push constant block in GLSL — a mismatch between stages sharing a pipeline layout is a validation error. The vertex shader carries the indices purely to satisfy this requirement; only the fragment shader reads them.

**Alternatives:**

| Approach | Pros | Cons |
|---|---|---|
| Per-object UBO *(baseline)* | No size limit | Descriptor write or dynamic offset per draw; CPU/GPU buffer contention |
| Push constants *(used)* | Lowest overhead; no allocation; no sync | 128-byte Vulkan minimum; unsuitable for large per-object payloads |
| Instance buffer | Enables GPU-driven / indirect draw | Requires instanced draw infrastructure |

**Limitations:** current 72-byte usage leaves 56 bytes of the guaranteed minimum. Adding skeletal animation blend weights or material parameters will eventually exhaust this budget and require a fallback to per-object UBOs or a storage buffer.

---

### Descriptor Set Layout Convention

Descriptor sets are bound independently per set index. Fixing set assignments engine-wide allows passes to bind only the sets they need without interfering with others.

| Set | Contents | Owner | Bind frequency |
|---|---|---|---|
| 0 | Global UBO — camera `view`, `proj` | `RenderingManager` | Once per frame |
| 1 | Bindless texture array + sampler pool | `AssetManager` | Once per frame |
| 2 | G-buffer attachments *(planned)* | Lighting pass | Once per pass |
| 3 | Per-pass extras *(planned)* | Individual passes | Once per pass |

---

## Cross-Cutting Concerns

### GPU Memory

The placeholder image (1×1 RGBA8) is allocated once at `AssetManager` init and lives for the process lifetime. Each loaded texture allocates a `vk::DeviceMemory` owned by `Texture` and freed in `AssetManager::drop`. The staging buffer used during upload is allocated and freed within each `load_texture` call — it does not persist.

### Unload Lifecycle

`unload_texture` is currently implemented but not called at runtime. There is no mid-game unloading in use. The expected caller is a future scene system that unloads assets when a scene is torn down — not individual entities.

Automatic unloading via `Drop` is not straightforward: `Texture` would need to call back into `AssetManager` (to write the placeholder and reclaim the slot), but `AssetManager` owns the `Arc<Texture>`. A drop-based design would require either a separate cleanup-context `Arc` (new pattern, inconsistent with the rest of the codebase) or a `Weak<Mutex<AssetManager>>` back-reference. Neither is warranted until a scene system exists and real unload semantics are defined.

One viable future design: a time-to-live field on the texture tracking idle frames. When only the `AssetManager`'s own `Arc` remains (no entity holds a reference) and the TTL expires, the asset manager unloads it during its periodic flush. This makes unloading demand-driven without requiring Drop callbacks.

### CPU/GPU Sync

Texture upload uses single-time command buffers (`begin_single_time_commands` / `end_single_time_commands`) that submit and wait for completion synchronously. No explicit barrier is needed between upload and first use because the wait-idle on submission guarantees visibility. Partial descriptor updates (`vkUpdateDescriptorSets`) are safe to issue while the set is bound to an in-flight frame only if the slot being written is not accessed by that frame — guaranteed for loads because new slots are written before the frame that first uses them.

On unload, the placeholder is written back to the descriptor slot immediately (safe — placeholder image is always valid). The slot index is held in `pending_reclaim` for `MAX_FRAMES_IN_FLIGHT` frames before returning to `free_slots`, ensuring no in-flight frame can reference a reused slot. GPU resources (image, image view, memory) are destroyed via the device's per-frame dynamic deletion queue, flushed at the start of the frame that reuses that sync slot.

### Validation / Driver Constraints

`shaderSampledImageArrayDynamicIndexing` is a Vulkan 1.0 core feature but must be explicitly enabled at logical device creation — it is not on by default. Without it, dynamic indexing into a descriptor array produces undefined behaviour with no guaranteed validation error on all drivers.

### Performance

Descriptor binding cost is eliminated per draw — both sets are bound once at frame start. The dominant per-draw cost is one `vkCmdPushConstants` call (72 bytes). Pre-filling 1024 descriptor slots at init costs 1024 `vkUpdateDescriptorSets` calls at startup; this is a one-time cost.

---

## Key Types

| Type | Owns | Lifetime |
|---|---|---|
| `Texture` | `vk::Image`, `vk::ImageView`, `vk::DeviceMemory`, slot index, sampler index | Until `AssetManager::drop` |
| `TextureHandle` | `Arc<Texture>` | ECS component on entity |
| `SamplerType` | — (`Copy` enum, index via `index()`) | Stack |
| `AssetManager` | Descriptor set, layout, pool, sampler objects, placeholder image, texture cache, `free_slots`, `pending_reclaim` | ECS `Resource`, process lifetime |
| `DescriptorLayoutBuilder` | Vec of binding infos | Temporary builder, consumed by `build()` |

---

## Implementation

**`Texture`** — removed `descriptor_set: vk::DescriptorSet`; added `index: u32` and `sampler_index: u32`. Constructor no longer takes `Device` or a descriptor set.

**`AssetManager`** — replaced per-texture descriptor pool + per-texture set with a single `bindless_descriptor_set`. Replaced `next_slot: u32` with `free_slots: Vec<u32>` (pre-filled with all slot indices at init) and `pending_reclaim: Vec<(u32, usize)>`. Added `samplers: Vec<vk::Sampler>` and placeholder image fields. `load_texture` pops from `free_slots`; `unload_texture` writes the placeholder back to the descriptor slot, queues the slot in `pending_reclaim`, and schedules GPU resource destruction via the device deletion queue. `flush_pending_reclaims(current_frame)` is called each frame by the render loop to move safe slots back to `free_slots`.

**`DescriptorLayoutBuilder`** — added `add_binding_array(binding, type, stage, count)`. `add_binding` delegates to it with `count: 1`.

**`MeshPushConstants`** — added `texture_index: u32` and `sampler_index: u32` after `model`. Stage flags changed from `VERTEX` to `VERTEX | FRAGMENT`.

**Render loop** — bindless set bound once per frame at set index 1. Per-entity `vkCmdBindDescriptorSets` removed. Push constant payload extended with texture and sampler indices.

**Shaders** — fragment shader: `sampler2D` binding replaced with `texture2D textures[1024]` (binding 0) and `sampler samplers[3]` (binding 1); sample via `sampler2D(textures[i], samplers[j])` constructor. Vertex shader: push constant block extended to match fragment stage declaration. `build.rs`: shaderc target set to `TargetEnv::Vulkan` / `Vulkan1_0`.

**Unchanged:** global UBO (set 0), mesh loading, vertex/index buffers, ECS component structure, swapchain and frame synchronisation.
