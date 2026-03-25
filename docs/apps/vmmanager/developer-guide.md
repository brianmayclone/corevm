# vmmanager — Developer Guide

This guide covers the internal architecture of the vmmanager desktop application.

## Tech Stack

| Technology | Purpose |
|-----------|---------|
| **egui / eframe** | Immediate-mode GUI framework |
| **glow (OpenGL)** | Framebuffer rendering |
| **libcorevm** | VM engine (direct Rust API) |
| **png** | Snapshot image encoding |
| **uuid** | VM identifiers |

## Source Structure

```
apps/vmmanager/src/
├── main.rs                 Entry point, HW check, window setup, error dialogs
├── app.rs                  Main application state and event loop (~1800 lines)
├── config.rs               VM configuration management
│
├── engine/                 VM lifecycle & platform abstraction
│   ├── vm.rs               VM creation, execution, control
│   ├── platform.rs         Linux platform-specific code
│   ├── input.rs            Keyboard/mouse input handling
│   ├── diagnostics.rs      Troubleshooting and logging
│   ├── iso_detect.rs       ISO file detection
│   ├── evdev_input.rs      Linux evdev keyboard input
│   └── evdev_mouse.rs      Linux evdev mouse input
│
└── ui/                     Graphical components
    ├── components/         Reusable widgets
    │   ├── display.rs      VGA framebuffer display
    │   ├── sidebar.rs      VM list sidebar
    │   ├── statusbar.rs    Metrics status bar
    │   ├── toolbar.rs      Action toolbar
    │   └── ...
    ├── dialogs/            Popup dialogs
    │   ├── create_vm.rs    VM creation wizard
    │   ├── add_disk.rs     Disk creation dialog
    │   ├── settings.rs     VM settings dialog
    │   ├── snapshots.rs    Snapshot management
    │   └── ...
    └── theme.rs            Color scheme and styling
```

## Architecture

### Application Loop (egui)

vmmanager uses egui's immediate-mode rendering:

1. `eframe::run_native()` creates the window and starts the event loop
2. Each frame, `App::update()` is called
3. The update function builds the entire UI from scratch each frame
4. egui diffs the output and only repaints changed regions

### State Management

`App` struct in `app.rs` holds all application state:
- List of VM configurations
- Currently selected VM
- Running VM instances (handle, framebuffer, metrics)
- Dialog state (open/closed, form values)
- UI preferences

### VM Engine Integration

vmmanager links directly against libcorevm as a Rust crate:

1. **Create:** Calls `libcorevm::vm::Vm::new()` with configuration
2. **Configure:** Sets up devices via libcorevm API (not FFI)
3. **Run:** Spawns a thread that calls the runtime execution loop
4. **Display:** Reads the framebuffer pointer and uploads to OpenGL texture
5. **Input:** Injects keyboard scancodes and mouse events via libcorevm API
6. **Stop:** Sends stop signal through the control interface

### Framebuffer Rendering

The VGA framebuffer is rendered as an OpenGL texture:

1. libcorevm maintains a raw pixel buffer (RGBA)
2. Each frame, the buffer is read and uploaded to an OpenGL texture
3. egui renders the texture in the display area
4. Resolution changes are detected and the texture is resized

### Platform-Specific Code

| Module | Linux |
|--------|-------|
| `platform.rs` | KVM availability check |
| `evdev_input.rs` | evdev keyboard capture |
| `evdev_mouse.rs` | evdev mouse capture |
| `input.rs` | Fallback keyboard/mouse |

### Configuration

VM configurations are stored as files on disk. `config.rs` handles:
- Serialization/deserialization (serde)
- Default values for new VMs
- Migration of old config formats

## Adding a New Dialog

1. Create a new file in `src/ui/dialogs/`
2. Implement the dialog as a struct with state + `show()` method
3. Add state field to `App` in `app.rs`
4. Call `dialog.show(ctx)` in the update loop when the dialog is open
5. Handle dialog results in the update function

## Adding a New UI Component

1. Create a new file in `src/ui/components/`
2. Implement as a function or struct that takes `&mut egui::Ui`
3. Call from the appropriate place in `app.rs`

## Dependencies

| Crate | Purpose |
|-------|---------|
| `eframe` | Application framework (windowing, event loop) |
| `egui` | Immediate-mode GUI |
| `egui_extras` | Additional widgets |
| `glow` | OpenGL bindings |
| `libcorevm` | VM engine |
| `png` | Image encoding |
| `uuid` | VM identifiers |
| `arboard` | Clipboard access |
| `libc` | Linux system calls (Linux only) |
