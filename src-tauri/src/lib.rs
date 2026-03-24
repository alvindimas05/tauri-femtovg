use tauri::{AppHandle, Manager, RunEvent, WebviewWindow, WindowEvent};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

enum RenderMsg {
    Resize { width: u32, height: u32 },
    Paint,
    Suspend,
    Resume,
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
        .plugin(tauri_plugin_tauri_femtovg::init())
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

        RunEvent::WindowEvent {
            label: _,
            event: WindowEvent::Focused(focused),
            ..
        } => {
            if let Some(state) = app_handle.try_state::<RenderState>() {
                if focused {
                    state.send_msg(RenderMsg::Resume);
                    state.send_msg(RenderMsg::Paint);
                } else {
                    state.send_msg(RenderMsg::Suspend);
                }
            }
        }

        RunEvent::Resumed => {
            if let Some(state) = app_handle.try_state::<RenderState>() {
                state.send_msg(RenderMsg::Resume);
                state.send_msg(RenderMsg::Paint);
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
                RenderMsg::Suspend | RenderMsg::Resume => {}
                RenderMsg::Exit => break,
            }
        }
    });

    app.manage(RenderState { tx });
    Ok(())
}

// wgpu initialization
#[cfg(not(target_os = "linux"))]
fn create_surface<'a>(
    instance: &wgpu::Instance,
    window: &tauri::WebviewWindow,
) -> Result<wgpu::Surface<'static>, Box<dyn std::error::Error>> {
    #[cfg(target_os = "android")]
    let surface = {
        use jni::objects::{JClass, JObject, JValue};
        use jni::JNIEnv;
        use raw_window_handle::{
            AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
        };
        use std::ffi::c_void;

        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }?;
        let mut env = vm.attach_current_thread()?;

        let context = unsafe { JObject::from_raw(ctx.context() as jni::sys::jobject) };

        let class_context = env.find_class("android/content/Context")?;
        let get_class_loader_method =
            env.get_method_id(class_context, "getClassLoader", "()Ljava/lang/ClassLoader;")?;

        let class_loader = unsafe {
            env.call_method_unchecked(
                &context,
                get_class_loader_method,
                jni::signature::ReturnType::Object,
                &[],
            )
        }?
        .l()?;

        let class_class_loader = env.find_class("java/lang/ClassLoader")?;
        let load_class_method = env.get_method_id(
            class_class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
        )?;

        // Note: Package name might have hyphens replaced by underscores in Dalvik
        let class_name_str = env.new_string("com.plugin.tauri_femtovg.ExamplePlugin")?;

        let mut android_surface_obj: JObject = JObject::null();
        println!("create_surface: Waiting for surface class load...");

        for _ in 0..50 {
            let plugin_class_value = unsafe {
                env.call_method_unchecked(
                    &class_loader,
                    load_class_method,
                    jni::signature::ReturnType::Object,
                    &[JValue::Object(&class_name_str).as_jni()],
                )
            };

            if let Ok(val) = plugin_class_value {
                let plugin_class_obj = val.l()?;
                let plugin_class: JClass = plugin_class_obj.into();

                let field_id = env.get_static_field_id(
                    &plugin_class,
                    "surface",
                    "Landroid/view/Surface;",
                )?;
                let surface_obj_res = env.get_static_field_unchecked(
                    &plugin_class,
                    field_id,
                    jni::signature::JavaType::Object("Landroid/view/Surface;".to_string()),
                );

                if let Ok(obj_val) = surface_obj_res {
                    let obj = obj_val.l()?;
                    if !obj.is_null() {
                        println!("create_surface: Found valid surface object");
                        android_surface_obj = obj;
                        break;
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        if android_surface_obj.is_null() {
            return Err("Timed out waiting for Android Surface".into());
        }

        let native_window = unsafe {
            ndk::native_window::NativeWindow::from_surface(
                env.get_native_interface(),
                android_surface_obj.as_raw(),
            )
        }
        .ok_or("Failed to create native window from surface")?;

        let _native_window_ref = native_window.ptr().as_ptr();

        std::mem::forget(native_window);

        let handle = AndroidNdkWindowHandle::new(
            std::ptr::NonNull::new(_native_window_ref as *mut c_void).unwrap(),
        );
        let raw_window_handle = RawWindowHandle::AndroidNdk(handle);

        let display_handle = AndroidDisplayHandle::new();
        let raw_display_handle = RawDisplayHandle::Android(display_handle);

        unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })?
        }
    };

    #[cfg(not(target_os = "android"))]
    let surface = instance.create_surface(window.clone()).unwrap();

    let surface: wgpu::Surface<'static> = unsafe { std::mem::transmute(surface) };
    Ok(surface)
}

#[cfg(not(target_os = "linux"))]
fn init_renderer(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::async_runtime::block_on;

    let window: WebviewWindow = app.get_webview_window("main").unwrap();
    
    // On Android, use JNI to get screen size instead of tao's window size
    #[cfg(target_os = "android")]
    let size = {
        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
        let mut env = vm.attach_current_thread().unwrap();
        let context = unsafe { jni::objects::JObject::from_raw(ctx.context() as jni::sys::jobject) };
        let window_service = env.new_string("window").unwrap();
        let window_manager = env
            .call_method(
                &context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[jni::objects::JValue::Object(&window_service)],
            ).unwrap().l().unwrap();

        let display = env
            .call_method(&window_manager, "getDefaultDisplay", "()Landroid/view/Display;", &[])
            .unwrap().l().unwrap();

        let metrics_class = env.find_class("android/util/DisplayMetrics").unwrap();
        let display_metrics = env.new_object(metrics_class, "()V", &[]).unwrap();

        env.call_method(
            &display,
            "getRealMetrics",
            "(Landroid/util/DisplayMetrics;)V",
            &[jni::objects::JValue::Object(&display_metrics)],
        ).unwrap();
        
        let width = env.get_field(&display_metrics, "widthPixels", "I").unwrap().i().unwrap() as u32;
        let height = env.get_field(&display_metrics, "heightPixels", "I").unwrap().i().unwrap() as u32;
        tauri::PhysicalSize::new(width, height)
    };
    
    #[cfg(not(target_os = "android"))]
    let size = window
        .inner_size()
        .expect("Failed to get window inner size");

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        flags: wgpu::InstanceFlags::empty(),
        ..Default::default()
    });

    let surface = create_surface(&instance, &window).expect("Failed to create surface");

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
    let thread_instance = instance;
    let thread_window = window;
    
    std::thread::spawn(move || {
        let renderer =
            femtovg::renderer::WGPURenderer::new(thread_device.clone(), thread_queue.clone());
        let mut canvas = femtovg::Canvas::new(renderer).expect("Cannot create femtovg canvas");
        canvas.set_size(size.width, size.height, 1.0);

        let mut surface_opt = Some(surface);

        for msg in rx {
            match msg {
                RenderMsg::Resize { width, height } => {
                    config.width = width;
                    config.height = height;
                    if let Some(surface) = &surface_opt {
                        surface.configure(&thread_device, &config);
                    }
                    canvas.set_size(width, height, 1.0);
                }

                RenderMsg::Paint => {
                    if let Some(surface) = &surface_opt {
                        let frame = match surface.get_current_texture() {
                            Ok(f) => f,
                            Err(_) => continue,
                        };
                        draw_triangle(&mut canvas);
                        let cmd_buffer = canvas.flush_to_surface(&frame.texture);
                        thread_queue.submit(std::iter::once(cmd_buffer));
                        frame.present();
                    }
                }

                RenderMsg::Suspend => {
                    #[cfg(target_os = "android")]{
                        surface_opt = None;
                        println!("Render loop: WGPU Surface suspended/dropped");
                    }
                }

                RenderMsg::Resume => {
                    #[cfg(target_os = "android")]
                    if surface_opt.is_none() {
                        println!("Render loop: Resuming WGPU surface...");
                        match create_surface(&thread_instance, &thread_window) {
                            Ok(new_surface) => {
                                new_surface.configure(&thread_device, &config);
                                surface_opt = Some(new_surface);
                                println!("Render loop: WGPU Surface recreated");
                            }
                            Err(e) => {
                                println!("Render loop: Failed to recreate surface: {}", e);
                            }
                        }
                    }
                }

                RenderMsg::Exit => break,
            }
        }
    });

    app.manage(RenderState { tx });
    Ok(())
}
