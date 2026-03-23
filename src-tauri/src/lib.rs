use tauri::{AppHandle, Manager, RunEvent, WebviewWindow, WindowEvent};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

enum RenderMsg {
    Resize { width: u32, height: u32 },
    Paint,
    Exit,
}
#[cfg(target_os = "linux")]
struct RenderState {
    tx: async_channel::Sender<RenderMsg>,
}

#[cfg(not(target_os = "linux"))]
struct RenderState {
    tx: std::sync::mpsc::SyncSender<RenderMsg>,
}

impl RenderState {
    fn send_msg(&self, msg: RenderMsg) {
        let _ = self.tx.send(msg);
    }
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
            if let Some(state) = app_handle.try_state::<RenderState>() {
                state.send_msg(RenderMsg::Resize {
                    width: if size.width > 0 { size.width } else { 1 },
                    height: if size.height > 0 { size.height } else { 1 },
                });
            }
        }

        RunEvent::MainEventsCleared => {
            if let Some(state) = app_handle.try_state::<RenderState>() {
                state.send_msg(RenderMsg::Paint);
            }
        }

        RunEvent::ExitRequested { .. } | RunEvent::Exit => {
            if let Some(state) = app_handle.try_state::<RenderState>() {
                let _ = state.tx.send(RenderMsg::Exit);
            }
        }

        _ => {}
    }
}

fn draw_triangle<R: femtovg::Renderer>(canvas: &mut femtovg::Canvas<R>) {
    let w = canvas.width() as f32;
    let h = canvas.height() as f32;

    let bg_color = femtovg::Color::rgba(0, 0, 0, 0);
    canvas.clear_rect(0, 0, canvas.width(), canvas.height(), bg_color);

    let cx = w / 2.0;
    let top = (h * 0.15, cx);
    let bl = (h * 0.85, cx - w * 0.35);
    let br = (h * 0.85, cx + w * 0.35);

    let mut path = femtovg::Path::new();
    path.move_to(top.1, top.0);
    path.line_to(br.1, br.0);
    path.line_to(bl.1, bl.0);
    path.close();

    let paint = femtovg::Paint::color(femtovg::Color::rgb(220, 30, 30));
    canvas.fill_path(&path, &paint);
}

// Linux / GTK initialization
#[cfg(target_os = "linux")]
fn init_renderer(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    let window: WebviewWindow = app.get_webview_window("main").unwrap();

    let vbox = window.default_vbox().unwrap();

    // The Webview is already packed inside vbox by Tauri.
    // We can extract its children, reparent it into a GTK overlay alongside GLArea.
    let children = vbox.children();
    let webview_widget = children.first().unwrap().clone();
    vbox.remove(&webview_widget);

    // Instead of using `vbox` which messes up `wry`'s `parent().parent()` expectations,
    // we'll replace the GTK window's root child (`vbox`) with an `Overlay`.
    let gtk_window = vbox.parent().unwrap().downcast::<gtk::Window>().unwrap();
    gtk_window.remove(&vbox);

    let overlay = gtk::Overlay::new();
    let gl_area = gtk::GLArea::new();
    gl_area.set_has_alpha(true);
    gl_area.set_has_stencil_buffer(true);
    gl_area.set_auto_render(true);

    let canvas_state: Rc<RefCell<Option<femtovg::Canvas<femtovg::renderer::OpenGl>>>> =
        Rc::new(RefCell::new(None));
    let canvas_state_realize = canvas_state.clone();
    let canvas_state_render = canvas_state.clone();

    gl_area.connect_realize(move |gl_area| {
        gl_area.make_current();
        if gl_area.error().is_some() {
            return;
        }

        let renderer = unsafe {
            femtovg::renderer::OpenGl::new_from_function(|s| {
                let mut ptr = std::ptr::null();
                let name = std::ffi::CString::new(s).unwrap();
                if let Ok(lib) = libloading::Library::new("libGL.so.1") {
                    if let Ok(sym) = lib
                        .get::<unsafe extern "C" fn(*const i8) -> *const std::ffi::c_void>(
                            b"glXGetProcAddress\0",
                        )
                    {
                        ptr = sym(name.as_ptr());
                    }
                }
                if ptr.is_null() {
                    if let Ok(lib) = libloading::Library::new("libEGL.so.1") {
                        if let Ok(sym) =
                            lib.get::<unsafe extern "C" fn(*const i8) -> *const std::ffi::c_void>(
                                b"eglGetProcAddress\0",
                            )
                        {
                            ptr = sym(name.as_ptr());
                        }
                    }
                }
                ptr
            })
        }
        .expect("Cannot create femtovg OpenGL renderer");

        let mut canvas = femtovg::Canvas::new(renderer).expect("Cannot create femtovg canvas");

        let alloc = gl_area.allocation();
        canvas.set_size(alloc.width() as u32, alloc.height() as u32, 1.0);

        *canvas_state_realize.borrow_mut() = Some(canvas);
    });

    gl_area.connect_render(move |_gl_area, _gl_context| {
        if let Some(canvas) = canvas_state_render.borrow_mut().as_mut() {
            draw_triangle(canvas);
            canvas.flush();
        }
        gtk::glib::Propagation::Proceed
    });

    gl_area.connect_resize(move |_gl_area, width, height| {
        if let Some(canvas) = canvas_state.borrow_mut().as_mut() {
            canvas.set_size(width as u32, height as u32, 1.0);
        }
    });

    overlay.add(&gl_area);

    // Put webview on top of GLArea directly into the Overlay
    // This retains `webview` -> `overlay` -> `gtk_window` hierarchy
    webview_widget.set_halign(gtk::Align::Fill);
    webview_widget.set_valign(gtk::Align::Fill);
    overlay.add_overlay(&webview_widget);

    gtk_window.add(&overlay);
    overlay.show_all();

    let (tx, rx) = async_channel::unbounded();

    let gl_area_clone = gl_area.clone();
    gtk::glib::MainContext::default().spawn_local(async move {
        while let Ok(msg) = rx.recv().await {
            match msg {
                RenderMsg::Resize { .. } => {
                    // Resize handled by GTK layout
                }
                RenderMsg::Paint => {
                    gl_area_clone.queue_render();
                }
                RenderMsg::Exit => break,
            }
        }
    });

    app.manage(RenderState { tx });
    Ok(())
}

// wgpu initialization
#[cfg(not(target_os = "linux"))]
fn init_renderer(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::async_runtime::block_on;

    let window: WebviewWindow = app.get_webview_window("main").unwrap();
    let size = window
        .inner_size()
        .expect("Failed to get window inner size");

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

    let alpha_mode = if swapchain_capabilities
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
    {
        wgpu::CompositeAlphaMode::PreMultiplied
    } else if swapchain_capabilities
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
    {
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

    let (tx, rx) = std::sync::mpsc::sync_channel::<RenderMsg>(64);

    let thread_device = device.clone();
    let thread_queue = queue.clone();
    std::thread::spawn(move || {
        let renderer =
            femtovg::renderer::WGPURenderer::new(thread_device.clone(), thread_queue.clone());
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
                    draw_triangle(&mut canvas);
                    let cmd_buffer = canvas.flush_to_surface(&frame.texture);
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
