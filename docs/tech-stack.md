This document summarizes your environment setup on **Garuda Linux** and the architectural path for your Rust-based Vulkan engine.

---

## **Project Specification: Rust Vulkan Engine**

### **1. Tools & Environment Summary**

We have configured your system to act as a Linux-based development workstation capable of producing Windows-native binaries.

| Tool | Purpose | Status |
| --- | --- | --- |
| **Rustup** | The toolchain manager for Rust. | Installed via `pacman`. |
| **Stable Toolchain** | The default compiler version for reliability. | Set as default. |
| **Vulkan SDK** | Headers and tools for Vulkan development. | Installed (`vulkan-devel`). |
| **Validation Layers** | Crucial debugging tools that catch API errors. | Installed. |
| **MinGW-w64** | The GCC-based cross-linker for Windows. | Installed. |

### **2. Essential Libraries (Crates)**

Your `Cargo.toml` will use these "modern headers" to interact with the hardware:

* **`ash`**: Low-level, high-performance Vulkan bindings.
* **`winit`**: Modern windowing and input (handles Wayland/X11 and Windows).
* **`gpu-allocator`**: Manages the complex task of memory allocation on the GPU.
* **`raw-window-handle`**: The bridge between your window (`winit`) and your graphics API (`ash`).

---

## **3. Cross-Compilation Workflow**

One of the primary reasons for choosing Rust is the streamlined cross-compilation. Here is how your "Linux to Windows" pipeline works:

### **A. Configuration**

Rust needs to know which linker to use when building for Windows. In your project root, you should create a folder named `.cargo/` and a file inside it named `config.toml`:

```toml
# .cargo/config.toml
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-gcc-ar"

```

### **B. The Build Process**

To generate a Windows executable (`.exe`), you simply run:

```bash
cargo build --target x86_64-pc-windows-gnu

```

### **C. How it works under the hood**

1. **Frontend:** `cargo` downloads the source code for all your dependencies.
2. **Compilation:** `rustc` compiles the code into machine code for the Windows x86_64 architecture.
3. **Linking:** The `mingw-w64` tool we installed takes those pieces and packages them into a Windows-compatible Portable Executable (`.exe`) format.
4. **Testing:** Since you are on Linux, you can actually run and test this `.exe` immediately using **Wine** without leaving your terminal.

---

## **4. Strategic Architecture: Vulkan 1.3+**

Since we are starting in 2026, we are bypassing the "Legacy" Vulkan (1.0/1.1) methods. Your engine will utilize **Dynamic Rendering**.

* **Legacy:** Required "Render Passes" and "Framebuffers" to be defined upfront (very rigid).
* **Modern (1.3+):** You simply call `begin_rendering` directly in your command buffer. This is much closer to how Modern C++, DirectX 12, and Metal operate.
