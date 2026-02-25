# Copilot Instructions for `rust-game-engine`

This document is intended for AI coding agents working with the repository.  It lists the
"big picture" layout, conventions, workflows and project‑specific idioms that help you
become productive quickly.

---

## 🧱 Architecture Overview

1. **Entry point**: `src/main.rs` creates a `winit` event loop and owns an
   `RenderingState`.  All window events and the render loop are handled through this
   file.
2. **RenderingState ⇄ VulkanContext**:  `RenderingState::new` sets up Vulkan and
   returns a `VulkanContext` plus a `ash::Device`.  The context bundles all GPU
   resources (surface, swapchain, queues, semaphores, etc.).
3. **Cleanup management**:  `src/cleanup.rs` defines `DeletionQueue`, a simple
   stack of closures for destroying Vulkan handles in reverse order.  Almost every
   `init(...)` helper pushes destruction closures into this queue.
4. **Utilities**:  `src/utils.rs` has a handful of helpers, e.g. `vk_to_cstr` for
   converting Vulkan fixed‑size strings.  Use them rather than rewriting the logic.
5. **Configuration**:  `src/config.rs` contains constants such as
   `VALIDATION_ENABLED` (enabled in debug builds via `cfg!(debug_assertions)`),
   application/engine names, etc.
6. **Documentation**:  `docs/` contains design notes for the pipeline, GPU/CPU
   synchronization, tech‑stack, etc.  Consult these files when adding features
   or reasoning about GPU behaviour.

All Rust code is synchronous; rendering is driven from `winit` callbacks, and
`unsafe` is used liberally for Vulkan calls.  The platform abstraction is minimal;
there is some `cfg!(target_os = "macos")` logic for portability extensions.

---

## 🔧 Common Workflows

* **Build and run locally**:
  ```sh
  cargo run                           # Linux/host execution
  RUST_LOG=debug cargo run            # verbose logging (validation layers also log)
  ```
* **Cross‑compile to Windows** (requires `.cargo/config.toml` as described in
  `docs/tech-stack.md`):
  ```sh
  cargo build --target x86_64-pc-windows-gnu
  # test the resulting exe under Wine if needed
  ```
* **Enable validation**: compile in debug (`cargo build` or `cargo run` default)
  or set the `VALIDATION_ENABLED` constant manually (not recommended).  The
  validation layer string is `"VK_LAYER_KHRONOS_validation"` and the loader is
  automatically added in `create_instance`.
* **Logging**: `env_logger` is initialized in `main()`.  Use `log::{error,info,`...
  macros everywhere.

> 📝 There are currently no automated tests.  New code should include `#[cfg(test)]`
  modules and use `cargo test` where appropriate.

---

## 🧩 Project‑Specific Patterns

* **Deletion queue**: always push a *clone* of the Vulkan loader/device before
  moving it into the closure.  Example:
  ```rust
  let logical_device_clone = logical_device.clone();
  deletion_queue.push(move || unsafe {
      logical_device_clone.destroy_semaphore(semaphore, None);
  });
  ```
  Flush the queue in `RenderingState::destroy` (done in `main.rs`).

* **`anyhow::Result`**: all fallible functions return `anyhow::Result<T>`.
  Propagate errors with `?` and wrap with `anyhow!("message")` when you need a
  custom message.  Panic only in the initialization path if the application
  can't continue.

* **Unsafe helpers**: helper functions such as `pick_physical_device`,
  `create_swapchain`, etc. are `unsafe` and often call low‑level Vulkan API
  directly.  Follow the existing naming style and error handling when you add
  similar helpers.

* **Queue family indices**: stored in a simple struct `QueueFamilyIndices` with a
  `get` method.  When you need graphics or transfer queues add similar helpers
  or extend the struct.

* **Swapchain helpers**: `get_swapchain_surface_format`,
  `get_swapchain_present_mode` and `get_swapchain_extent` encapsulate common logic
  and are called from `create_swapchain`.

* **Filesystem layout**: put renderer‑specific logic in `src/vulkan_context.rs`
  or new modules (e.g. `src/renderer.rs`) and then `mod` them in `main.rs`.

* **OS differences**: use `cfg!(target_os)` or `#[cfg(target_os = "...")]`
  guards rather than `cfg!(windows)` macros sprinkled through the code.

* **Build constants**: the project uses a handful of constants (application
  name, version, validation toggle).  Add new constants in `config.rs` rather
  than hard‑coding them elsewhere.

* **Clippy / formatting**: follow Rust 2024 edition conventions and `rustfmt`
  defaults; there is no workspace `rustfmt.toml` yet but feel free to introduce
  one for formatting rules.

---

## 🔗 Integration & Dependencies

* **Crates used**:
  * `ash` (Vulkan bindings)
  * `winit` (window & event loop)
  * `ash-window` & `raw-window-handle` (surface creation)
  * `gpu-allocator` (GPU memory management; currently unused but planned)
  * `anyhow`, `log`, `env_logger` for error/log handling
* **External tools**: Rust toolchain via `rustup`, Vulkan SDK (headers/layers),
  `mingw-w64` for Windows cross‑linking.  See `docs/tech-stack.md` for more.
* **Cross‑platform**: the code is written with Linux as the base target; macOS
  support needs portability extensions.  Windows is reached via cross‑compile.

---

## 💡 Tips for AI Agents

* Read `docs/*.md` when touching Vulkan‑related code – they capture reasoning
  that isn't obvious from the source (e.g. why a fencing scheme was chosen).
* When adding rendering functionality, start by implementing
  `VulkanContext::create_pipeline` and filling out the empty `render()` in
  `RenderingState`.
* Keep the top‑level `main.rs` simple; do not put Vulkan logic there.  Use the
  context helpers instead and expose just the minimal API that `RenderingState`
  needs.
* Logging at `info` level is expected for normal operations; use `warn` or
  `error` for recoverable/fatal mishaps and propagate errors upward.
* If you touch unsafe code, mirror the surrounding style (e.g. chaperone with
  `unsafe { ... }` blocks and document any invariants in comments).

---

Please review and let me know if any section is unclear or missing. I'm
ready to iterate based on your feedback.