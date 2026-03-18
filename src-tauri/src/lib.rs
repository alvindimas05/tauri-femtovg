use std::sync::mpsc;

use tauri::{async_runtime::block_on, AppHandle, Manager, RunEvent, WebviewWindow, WindowEvent};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

enum RenderMsg {
    Resize { width: u32, height: u32 },
    Paint,
    Exit,
}

struct RenderState {
    tx: mpsc::SyncSender<RenderMsg>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(init_renderer)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(handle_run);
}

fn handle_run(app_handle: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::WindowEvent {
            label: _,
            event: WindowEvent::Resized(size),
            ..
        } => {
            let state = app_handle.state::<RenderState>();
            let _ = state.tx.send(RenderMsg::Resize {
                width: if size.width > 0 { size.width } else { 1 },
                height: if size.height > 0 { size.height } else { 1 },
            });
        }

        RunEvent::MainEventsCleared => {
            let state = app_handle.state::<RenderState>();
            let _ = state.tx.send(RenderMsg::Paint);
        }

        RunEvent::ExitRequested { .. } | RunEvent::Exit => {
            if let Some(state) = app_handle.try_state::<RenderState>() {
                let _ = state.tx.send(RenderMsg::Exit);
            }
        }

        _ => {}
    }
}

fn init_renderer(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let window: WebviewWindow = app.get_webview_window("main").unwrap();
    let size = window.inner_size().expect("Failed to get window inner size");

    let instance = wgpu::Instance::default();
    let surface = instance.create_surface(window).unwrap();

    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
    }))
    .expect("Failed to find an appropriate adapter");

    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
    }))
    .expect("Failed to create device");

    let swapchain_capabilities = surface.get_capabilities(&adapter);
    let swapchain_format = swapchain_capabilities.formats[0];

    let alpha_mode = if swapchain_capabilities.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
        wgpu::CompositeAlphaMode::PreMultiplied
    } else if swapchain_capabilities.alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
        wgpu::CompositeAlphaMode::PostMultiplied
    } else {
        swapchain_capabilities.alpha_modes[0]
    };

    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: swapchain_format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &config);

    let (tx, rx) = mpsc::sync_channel::<RenderMsg>(64);

    let thread_device = device.clone();
    let thread_queue = queue.clone();
    std::thread::spawn(move || {
        let renderer = femtovg::renderer::WGPURenderer::new(thread_device.clone(), thread_queue.clone());
        let mut canvas = femtovg::Canvas::new(renderer).expect("Cannot create femtovg canvas");
        canvas.set_size(size.width, size.height, 1.0);

        for msg in rx {
            match msg {
                RenderMsg::Resize { width, height } => {
                    config.width = width;
                    config.height = height;
                    surface.configure(&thread_device, &config);
                    canvas.set_size(width, height, 1.0);
                }

                RenderMsg::Paint => {
                    let frame = match surface.get_current_texture() {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let cmd_buffer = draw_triangle(&mut canvas, &frame);
                    thread_queue.submit(std::iter::once(cmd_buffer));
                    frame.present();
                }

                RenderMsg::Exit => break,
            }
        }
    });

    app.manage(RenderState { tx });
    Ok(())
}

fn draw_triangle<R: femtovg::Renderer<Surface = wgpu::Texture, CommandBuffer = wgpu::CommandBuffer>>(
    canvas: &mut femtovg::Canvas<R>,
    frame: &wgpu::SurfaceTexture,
) -> wgpu::CommandBuffer {
    let w = canvas.width() as f32;
    let h = canvas.height() as f32;

    let bg_color = femtovg::Color::rgba(0, 0, 0, 0);
    canvas.clear_rect(0, 0, canvas.width(), canvas.height(), bg_color);

    let cx = w / 2.0;
    let top = (h * 0.15, cx);
    let bl  = (h * 0.85, cx - w * 0.35);
    let br  = (h * 0.85, cx + w * 0.35);

    let mut path = femtovg::Path::new();
    path.move_to(top.1, top.0);
    path.line_to(br.1,  br.0);
    path.line_to(bl.1,  bl.0);
    path.close();

    let paint = femtovg::Paint::color(femtovg::Color::rgb(220, 30, 30));
    canvas.fill_path(&path, &paint);

    canvas.flush_to_surface(&frame.texture)
}