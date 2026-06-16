use serde::Serialize;

// Estado de la captura para la UI. En la Fase 1 solo sirve para verificar que el
// bucle WGC corre y medir su impacto: cuántos frames llegan y a qué resolución.
#[derive(Serialize, Clone, Default)]
pub struct CaptureStatus {
    pub running: bool,
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub seconds: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct MonitorInfo {
    pub id: String,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub thumb: Option<String>,
}

#[derive(Serialize, Clone, Default)]
pub struct AudioInput {
    pub id: String,
    pub name: String,
}

#[cfg(target_os = "windows")]
pub use win::{
    list_audio_inputs, list_monitors, replay_active, save_replay, start, start_replay, status,
    stop, stop_replay,
};

#[cfg(not(target_os = "windows"))]
pub fn list_monitors() -> Vec<MonitorInfo> {
    Vec::new()
}

#[cfg(not(target_os = "windows"))]
pub fn list_audio_inputs() -> Vec<AudioInput> {
    Vec::new()
}

#[cfg(not(target_os = "windows"))]
pub fn start(
    _monitor_id: String,
    _out_dir: String,
    _fps: u32,
    _quality: String,
    _resolution: u32,
) -> Result<(), String> {
    Err("La captura solo está disponible en Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn stop() -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn status() -> CaptureStatus {
    CaptureStatus::default()
}

#[cfg(not(target_os = "windows"))]
pub fn start_replay(
    _monitor_id: String,
    _out_dir: String,
    _seconds: u32,
    _fps: u32,
    _quality: String,
    _resolution: u32,
) -> Result<(), String> {
    Err("El replay solo está disponible en Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn stop_replay() {}

#[cfg(not(target_os = "windows"))]
pub fn save_replay() -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn replay_active() -> bool {
    false
}

#[cfg(target_os = "windows")]
mod win {
    use std::collections::VecDeque;
    use std::mem::ManuallyDrop;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Condvar, Mutex, Once};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use windows::core::{IInspectable, Interface, Result, BOOL, GUID, HSTRING, PCWSTR};
    use windows::Devices::Enumeration::{DeviceClass, DeviceInformation};
    use windows::Foundation::TypedEventHandler;
    use windows::Graphics::Capture::{
        Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
    };
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, RECT, SYSTEMTIME};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_FLAG,
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
        D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_RESOURCE_MISC_FLAG, D3D11_SDK_VERSION,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors,
        GetDC, GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt,
        BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HALFTONE, HDC, HMONITOR, MONITORINFO,
        MONITORINFOEXW, SRCCOPY,
    };
    use windows::Win32::Media::MediaFoundation::*;

    // Valor Win32 de MONITORINFOF_PRIMARY (no lo genera el crate windows).
    const MONITORINFOF_PRIMARY: u32 = 1;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::SystemInformation::GetLocalTime;
    use windows::Win32::System::Variant::{VARIANT, VT_UI4};
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        GW_OWNER,
    };

    use super::{AudioInput, CaptureStatus, MonitorInfo};

    #[derive(Default)]
    struct Stats {
        frames: AtomicU64,
        width: AtomicU32,
        height: AtomicU32,
    }

    struct Running {
        stop: Arc<(Mutex<bool>, Condvar)>,
        handle: Option<JoinHandle<()>>,
        stats: Arc<Stats>,
        started: Instant,
        result: Arc<Mutex<Option<String>>>,
    }

    static STATE: Mutex<Option<Running>> = Mutex::new(None);

    // Media Foundation se inicializa una sola vez por proceso; no se apaga porque
    // vive lo que dure la app.
    static MF_INIT: Once = Once::new();
    fn ensure_mf() {
        MF_INIT.call_once(|| unsafe {
            let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
        });
    }

    pub fn list_monitors() -> Vec<MonitorInfo> {
        // Un único DC del escritorio virtual sirve para fotografiar todas las
        // pantallas (cada una vive en su trozo de coordenadas del rcMonitor).
        let screen_dc = unsafe { GetDC(None) };
        let monitors = enum_monitors()
            .into_iter()
            .enumerate()
            .filter_map(|(i, hmon)| monitor_info(hmon, i, screen_dc))
            .collect();
        if !screen_dc.is_invalid() {
            unsafe { ReleaseDC(None, screen_dc) };
        }
        monitors
    }

    // Entradas de audio (micrófonos) del sistema, con su nombre amigable. Vía
    // WinRT DeviceInformation: da el nombre sin pedir permiso de micrófono. Se
    // hace en un hilo MTA propio para no depender del apartamento del llamante.
    pub fn list_audio_inputs() -> Vec<AudioInput> {
        std::thread::spawn(|| {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let out = audio_inputs().unwrap_or_default();
            unsafe { CoUninitialize() };
            out
        })
        .join()
        .unwrap_or_default()
    }

    fn audio_inputs() -> Result<Vec<AudioInput>> {
        let collection = DeviceInformation::FindAllAsyncDeviceClass(DeviceClass::AudioCapture)?.get()?;
        let mut out = Vec::new();
        for device in &collection {
            out.push(AudioInput {
                id: device.Id()?.to_string(),
                name: device.Name()?.to_string(),
            });
        }
        Ok(out)
    }

    pub fn start(
        target: String,
        out_dir: String,
        fps: u32,
        quality: String,
        resolution: u32,
    ) -> std::result::Result<(), String> {
        let mut guard = STATE.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }

        let fps = clamp_fps(fps);
        let factor = bitrate_factor(&quality);
        let stats = Arc::new(Stats::default());
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let result = Arc::new(Mutex::new(None));
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<(), String>>();

        let stats_t = stats.clone();
        let stop_t = stop.clone();
        let result_t = result.clone();
        let handle = std::thread::Builder::new()
            .name("flashback-capture".into())
            .spawn(move || {
                capture_thread(
                    target, out_dir, fps, factor, resolution, stop_t, stats_t, result_t, ready_tx,
                )
            })
            .map_err(|e| e.to_string())?;

        // El hilo construye el pipeline WGC + encoder y reporta éxito o error antes
        // de ponerse a recibir frames; así start() puede devolver un fallo real.
        match ready_rx.recv() {
            Ok(Ok(())) => {
                *guard = Some(Running {
                    stop,
                    handle: Some(handle),
                    stats,
                    started: Instant::now(),
                    result,
                });
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => Err("El hilo de captura terminó inesperadamente".into()),
        }
    }

    // Devuelve la ruta del MP4 guardado (None si algo falló al finalizar el muxer).
    pub fn stop() -> Option<String> {
        let running = STATE.lock().unwrap().take();
        if let Some(mut running) = running {
            let (lock, cv) = &*running.stop;
            *lock.lock().unwrap() = true;
            cv.notify_all();
            if let Some(h) = running.handle.take() {
                let _ = h.join();
            }
            return running.result.lock().unwrap().take();
        }
        None
    }

    pub fn status() -> CaptureStatus {
        let guard = STATE.lock().unwrap();
        match guard.as_ref() {
            Some(r) => CaptureStatus {
                running: true,
                frames: r.stats.frames.load(Ordering::Relaxed),
                width: r.stats.width.load(Ordering::Relaxed),
                height: r.stats.height.load(Ordering::Relaxed),
                seconds: r.started.elapsed().as_secs_f64(),
            },
            None => CaptureStatus::default(),
        }
    }

    // El hilo de captura es dueño de los objetos COM/WGC: los crea con su propio
    // apartamento MTA y los suelta en el mismo hilo antes de salir.
    #[allow(clippy::too_many_arguments)]
    fn capture_thread(
        target: String,
        out_dir: String,
        fps: u32,
        factor: f64,
        resolution: u32,
        stop: Arc<(Mutex<bool>, Condvar)>,
        stats: Arc<Stats>,
        result: Arc<Mutex<Option<String>>>,
        ready: mpsc::Sender<std::result::Result<(), String>>,
    ) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let engine = match resolve_target_item(&target).and_then(|item| {
            build_engine(&stats, item, &out_dir, fps, factor, resolution)
                .map_err(|e| format!("{e:?}"))
        }) {
            Ok(e) => {
                let _ = ready.send(Ok(()));
                e
            }
            Err(e) => {
                let _ = ready.send(Err(e));
                unsafe { CoUninitialize() };
                return;
            }
        };

        let (lock, cv) = &*stop;
        let mut stopped = lock.lock().unwrap();
        while !*stopped {
            stopped = cv.wait(stopped).unwrap();
        }
        drop(stopped);

        // Orden de parada: cortar primero la llegada de frames (para que ningún
        // WriteSample corra contra Finalize) y luego cerrar el MP4.
        engine.shutdown();
        *result.lock().unwrap() = engine.finalize_encoder();

        drop(engine);
        unsafe { CoUninitialize() };
    }

    struct Engine {
        _device: ID3D11Device,
        frame_pool: Direct3D11CaptureFramePool,
        session: GraphicsCaptureSession,
        token: i64,
        encoder: Arc<Mutex<Encoder>>,
    }

    impl Engine {
        fn shutdown(&self) {
            let _ = self.frame_pool.RemoveFrameArrived(self.token);
            let _ = self.session.Close();
        }

        fn finalize_encoder(&self) -> Option<String> {
            self.encoder.lock().unwrap().finalize()
        }
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            let _ = self.frame_pool.RemoveFrameArrived(self.token);
            let _ = self.session.Close();
            let _ = self.frame_pool.Close();
        }
    }

    fn build_engine(
        stats: &Arc<Stats>,
        item: GraphicsCaptureItem,
        out_dir: &str,
        fps: u32,
        factor: f64,
        resolution: u32,
    ) -> Result<Engine> {
        let (device, d3d_device) = create_device()?;
        // NV12/H.264 exigen dimensiones PARES (ver nota en build_replay): la ventana de un
        // juego puede ser impar y disparaba MF_E_INVALIDMEDIATYPE. Redondear a par.
        let mut size = item.Size()?;
        size.Width = size.Width.max(2) & !1;
        size.Height = size.Height.max(2) & !1;
        let width = size.Width as u32;
        let height = size.Height as u32;
        // Se captura a nativo y el encoder escala al objetivo (downscale). El bitrate se
        // calcula sobre la resolución de salida, no la de captura.
        let (out_w, out_h) = output_dims(width, height, resolution);
        let bitrate = target_bitrate(out_w, out_h, fps, factor);

        let out_path = format!("{out_dir}\\{}", clip_filename());
        let encoder = Arc::new(Mutex::new(Encoder::new(
            &device, width, height, out_w, out_h, fps, bitrate, out_path,
        )?));

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let session = frame_pool.CreateCaptureSession(&item)?;
        // Sin el borde de captura de WGC (si el SO lo permite): no queremos esa línea de
        // color alrededor de la ventana/pantalla dentro del clip grabado.
        let _ = session.SetIsBorderRequired(false);

        // El handler corre en el pool de hilos del sistema (frame pool free-threaded):
        // recoge la textura del frame y la empuja al encoder por hardware. La textura
        // WGC se copia GPU→GPU dentro del encoder (no baja a CPU): el camino sagrado.
        // El límite de FPS descarta frames antes de tocar la GPU para no encodear de más.
        let stats = stats.clone();
        let enc = encoder.clone();
        let interval = fps_interval(fps);
        let last_kept = Arc::new(AtomicI64::new(i64::MIN));
        let handler = TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
            move |pool, _| {
                if let Some(pool) = pool.as_ref() {
                    if let Ok(frame) = pool.TryGetNextFrame() {
                        if let Ok(s) = frame.ContentSize() {
                            stats.width.store(s.Width.max(0) as u32, Ordering::Relaxed);
                            stats.height.store(s.Height.max(0) as u32, Ordering::Relaxed);
                        }
                        let t = frame.SystemRelativeTime().map(|x| x.Duration).unwrap_or(0);
                        if keep_frame(&last_kept, t, interval) {
                            if let Ok(surface) = frame.Surface() {
                                if let Ok(access) = surface.cast::<IDirect3DDxgiInterfaceAccess>() {
                                    if let Ok(tex) =
                                        unsafe { access.GetInterface::<ID3D11Texture2D>() }
                                    {
                                        let _ = enc.lock().unwrap().push(&tex, t);
                                    }
                                }
                            }
                            stats.frames.fetch_add(1, Ordering::Relaxed);
                        }
                        let _ = frame.Close();
                    }
                }
                Ok(())
            },
        );
        let token = frame_pool.FrameArrived(&handler)?;
        session.StartCapture()?;

        Ok(Engine {
            _device: device,
            frame_pool,
            session,
            token,
            encoder,
        })
    }

    // Device D3D11 con soporte BGRA (obligatorio para el interop de WGC) y de vídeo
    // (lo exige Media Foundation para codificar por hardware compartiendo el device),
    // más su equivalente WinRT IDirect3DDevice, que es lo que consume el frame pool.
    fn create_device() -> Result<(ID3D11Device, IDirect3DDevice)> {
        let mut device: Option<ID3D11Device> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
        }
        let device = device.expect("D3D11CreateDevice no devolvió device");
        let dxgi: IDXGIDevice = device.cast()?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
        let d3d_device: IDirect3DDevice = inspectable.cast()?;
        Ok((device, d3d_device))
    }

    // Encoder H.264 por hardware vía Media Foundation. El SinkWriter hospeda el MFT
    // por hardware (NVENC/AMF/QSV) y el conversor de color BGRA→NV12, ambos en GPU
    // gracias al DXGI device manager que comparte nuestro ID3D11Device. Le damos las
    // texturas BGRA de WGC y escribe el MP4 (H.264) directamente.
    struct Encoder {
        writer: IMFSinkWriter,
        stream: u32,
        ctx: ID3D11DeviceContext,
        pool: Vec<ID3D11Texture2D>,
        next: usize,
        base: i64,
        last: i64,
        has_base: bool,
        path: String,
        finalized: bool,
    }

    // El handler de FrameArrived exige Send+Sync. El Encoder solo se toca bajo el
    // Mutex y desde el callback (que WGC serializa), con el device protegido para
    // multihilo, así que moverlo entre hilos de forma sincronizada es seguro.
    unsafe impl Send for Encoder {}

    impl Encoder {
        // (in_w,in_h) = resolución de captura nativa que se le entrega; (out_w,out_h) =
        // resolución del MP4. Si difieren, el SinkWriter intercala un Video Processor que
        // escala (en GPU, vía el device manager compartido) antes del encoder H.264.
        #[allow(clippy::too_many_arguments)]
        fn new(
            device: &ID3D11Device,
            in_w: u32,
            in_h: u32,
            out_w: u32,
            out_h: u32,
            fps: u32,
            bitrate: u32,
            path: String,
        ) -> Result<Encoder> {
            ensure_mf();

            // Compartir el mismo device con MF: así el encoder lee nuestras texturas
            // sin copiarlas a CPU. El device debe estar protegido para multihilo.
            let mut token = 0u32;
            let mut manager: Option<IMFDXGIDeviceManager> = None;
            unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager)? };
            let manager = manager.unwrap();
            unsafe { manager.ResetDevice(device, token)? };

            let ctx = unsafe { device.GetImmediateContext()? };
            if let Ok(mt) = ctx.cast::<ID3D11Multithread>() {
                let _ = unsafe { mt.SetMultithreadProtected(true) };
            }

            let attrs = unsafe {
                let mut a: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut a, 3)?;
                let a = a.unwrap();
                a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
                a.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
                a.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager)?;
                a
            };

            let url = HSTRING::from(path.as_str());
            let writer = unsafe { MFCreateSinkWriterFromURL(&url, None, &attrs)? };

            // fps nominal: WGC no entrega duplicados, así que el ritmo real lo marcan los
            // timestamps por muestra; aquí es metadato/objetivo del rate control. El
            // límite real de FPS lo aplica el handler descartando frames.
            let out_type = unsafe { MFCreateMediaType()? };
            unsafe {
                out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
                out_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
                out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                out_type.SetUINT64(&MF_MT_FRAME_SIZE, pack2(out_w, out_h))?;
                out_type.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1))?;
                out_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack2(1, 1))?;
            }
            let stream = unsafe { writer.AddStream(&out_type)? };

            let in_type = unsafe { MFCreateMediaType()? };
            unsafe {
                in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)?;
                in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                in_type.SetUINT64(&MF_MT_FRAME_SIZE, pack2(in_w, in_h))?;
                in_type.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1))?;
                in_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack2(1, 1))?;
                writer.SetInputMediaType(stream, &in_type, None)?;
            }

            unsafe { writer.BeginWriting()? };

            // Anillo de texturas propias: WGC reutiliza las suyas en cuanto soltamos
            // el frame, pero el encoder es asíncrono y puede leerlas más tarde; copiar
            // a una textura nuestra (rotando varias) evita esa carrera. Van a resolución
            // de captura (in_*): el escalado a out_* lo hace el SinkWriter.
            let desc = D3D11_TEXTURE2D_DESC {
                Width: in_w,
                Height: in_h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_FLAG(0).0 as u32,
                MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
            };
            let mut pool = Vec::with_capacity(6);
            for _ in 0..6 {
                let mut t: Option<ID3D11Texture2D> = None;
                unsafe { device.CreateTexture2D(&desc, None, Some(&mut t))? };
                pool.push(t.unwrap());
            }

            Ok(Encoder {
                writer,
                stream,
                ctx,
                pool,
                next: 0,
                base: 0,
                last: 0,
                has_base: false,
                path,
                finalized: false,
            })
        }

        fn push(&mut self, src: &ID3D11Texture2D, time: i64) -> Result<()> {
            let ts = if self.has_base {
                time - self.base
            } else {
                self.has_base = true;
                self.base = time;
                0
            };

            let dst = self.pool[self.next].clone();
            self.next = (self.next + 1) % self.pool.len();
            unsafe { self.ctx.CopyResource(&dst, src) };

            let buffer =
                unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &dst, 0, false)? };
            let len = unsafe { buffer.cast::<IMF2DBuffer>()?.GetContiguousLength()? };
            unsafe { buffer.SetCurrentLength(len)? };

            let sample = unsafe { MFCreateSample()? };
            let dur = if ts > self.last { ts - self.last } else { 166_667 };
            self.last = ts;
            unsafe {
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(ts)?;
                sample.SetSampleDuration(dur)?;
                self.writer.WriteSample(self.stream, &sample)?;
            }
            Ok(())
        }

        fn finalize(&mut self) -> Option<String> {
            if self.finalized {
                return Some(self.path.clone());
            }
            self.finalized = true;
            unsafe { self.writer.Finalize().ok()? };
            Some(self.path.clone())
        }
    }

    // ===================== INSTANT REPLAY (Fase 2b) =====================
    //
    // A diferencia de la grabación manual (Fase 2a, que delega en IMFSinkWriter y no
    // expone los paquetes), aquí poseemos el MFT del encoder H.264 por hardware para
    // tee-ar sus paquetes ya codificados a un ring buffer en RAM. Como los encoders HW
    // comen NV12, intercalamos el Video Processor MFT (BGRA→NV12 en GPU). Se codifica
    // siempre en segundo plano con IDR forzado periódico; al guardar, se muxea desde el
    // último IDR a un MP4 con un sink writer en passthrough (sin recodificar).

    const CLSID_VIDEO_PROCESSOR_MFT: GUID = GUID::from_u128(0x88753b26_5b24_49bd_b2e7_0c445c78c982);

    // Valores de MF_EVENT_TYPE para los MFT asíncronos (no dependemos de cómo los
    // represente el crate).
    const ME_TRANSFORM_NEED_INPUT: u32 = 601;
    const ME_TRANSFORM_HAVE_OUTPUT: u32 = 602;

    struct Packet {
        data: Vec<u8>,
        time: i64,
        dur: i64,
        key: bool,
    }

    struct ReplayBuffer {
        packets: VecDeque<Packet>,
        seq_header: Vec<u8>,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        window_ns: i64,
    }

    impl ReplayBuffer {
        fn new(seconds: u32, width: u32, height: u32, fps: u32, bitrate: u32) -> ReplayBuffer {
            ReplayBuffer {
                packets: VecDeque::new(),
                seq_header: Vec::new(),
                width,
                height,
                fps,
                bitrate,
                window_ns: seconds.max(1) as i64 * 10_000_000,
            }
        }

        fn push(&mut self, data: Vec<u8>, time: i64, dur: i64, key: bool) {
            self.packets.push_back(Packet { data, time, dur, key });
            self.trim();
        }

        // Mantener acotado el buffer: descartar hasta el último keyframe anterior al
        // inicio de la ventana, para que siempre podamos empezar el MP4 en un IDR.
        fn trim(&mut self) {
            let Some(latest) = self.packets.back().map(|p| p.time) else {
                return;
            };
            let cutoff = latest - self.window_ns;
            let mut anchor = 0usize;
            for (i, p) in self.packets.iter().enumerate() {
                if p.time > cutoff {
                    break;
                }
                if p.key {
                    anchor = i;
                }
            }
            for _ in 0..anchor {
                self.packets.pop_front();
            }
        }
    }

    struct ReplayRunning {
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
        buffer: Arc<Mutex<ReplayBuffer>>,
        out_dir: String,
    }

    static REPLAY_STATE: Mutex<Option<ReplayRunning>> = Mutex::new(None);

    // Textura envuelta para poder cruzar el canal: solo se usa de forma serializada
    // (FrameArrived produce, el hilo de bombeo consume) con el device multihilo-protegido.
    struct SendTex(ID3D11Texture2D);
    unsafe impl Send for SendTex {}

    struct FeedCtx {
        ctx: ID3D11DeviceContext,
        ring: Vec<ID3D11Texture2D>,
        next: AtomicUsize,
        tx: mpsc::Sender<(SendTex, i64)>,
    }
    unsafe impl Send for FeedCtx {}
    unsafe impl Sync for FeedCtx {}

    pub fn start_replay(
        target: String,
        out_dir: String,
        seconds: u32,
        fps: u32,
        quality: String,
        resolution: u32,
    ) -> std::result::Result<(), String> {
        let mut guard = REPLAY_STATE.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }

        let fps = clamp_fps(fps);
        let factor = bitrate_factor(&quality);
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Stats::default());
        let buffer: Arc<Mutex<ReplayBuffer>> =
            Arc::new(Mutex::new(ReplayBuffer::new(seconds, 0, 0, fps, 0)));
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<(), String>>();

        let stop_t = stop.clone();
        let buf_t = buffer.clone();
        let handle = std::thread::Builder::new()
            .name("flashback-replay".into())
            .spawn(move || {
                replay_thread(
                    target, seconds, fps, factor, resolution, stop_t, buf_t, stats, ready_tx,
                )
            })
            .map_err(|e| e.to_string())?;

        match ready_rx.recv() {
            Ok(Ok(())) => {
                *guard = Some(ReplayRunning {
                    stop,
                    handle: Some(handle),
                    buffer,
                    out_dir,
                });
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => Err("El hilo de replay terminó inesperadamente".into()),
        }
    }

    pub fn stop_replay() {
        let running = REPLAY_STATE.lock().unwrap().take();
        if let Some(mut r) = running {
            r.stop.store(true, Ordering::SeqCst);
            if let Some(h) = r.handle.take() {
                let _ = h.join();
            }
        }
    }

    pub fn replay_active() -> bool {
        REPLAY_STATE.lock().unwrap().is_some()
    }

    // Muxea los últimos N s del ring a un MP4 desde el último IDR. Se clona lo necesario
    // bajo el lock y se libera antes de tocar disco para no frenar el hilo de codificación.
    pub fn save_replay() -> Option<String> {
        let (buffer, out_dir) = {
            let guard = REPLAY_STATE.lock().unwrap();
            let r = guard.as_ref()?;
            (r.buffer.clone(), r.out_dir.clone())
        };

        let (packets, seq_header, width, height, fps, bitrate) = {
            let buf = buffer.lock().unwrap();
            let start = buf.packets.iter().position(|p| p.key);
            let pkts: Vec<(Vec<u8>, i64, i64, bool)> = match start {
                Some(s) => buf
                    .packets
                    .iter()
                    .skip(s)
                    .map(|p| (p.data.clone(), p.time, p.dur, p.key))
                    .collect(),
                None => Vec::new(),
            };
            (
                pkts,
                buf.seq_header.clone(),
                buf.width,
                buf.height,
                buf.fps,
                buf.bitrate,
            )
        };

        // Sin keyframe en el buffer aún no se puede empezar el MP4 en un IDR.
        if packets.is_empty() {
            return None;
        }

        let path = format!("{out_dir}\\{}", clip_filename());
        match mux_replay(&path, &packets, &seq_header, width, height, fps, bitrate) {
            Ok(()) => Some(path),
            Err(_) => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_thread(
        target: String,
        seconds: u32,
        fps: u32,
        factor: f64,
        resolution: u32,
        stop: Arc<AtomicBool>,
        buffer: Arc<Mutex<ReplayBuffer>>,
        stats: Arc<Stats>,
        ready: mpsc::Sender<std::result::Result<(), String>>,
    ) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let _ = seconds;
        let pipe = match resolve_target_item(&target).and_then(|item| {
            build_replay(&buffer, &stats, item, fps, factor, resolution)
                .map_err(|e| format!("{e:?}"))
        }) {
            Ok(p) => {
                let _ = ready.send(Ok(()));
                p
            }
            Err(e) => {
                let _ = ready.send(Err(e));
                unsafe { CoUninitialize() };
                return;
            }
        };

        run_pump(&pipe, &stop, &buffer);

        // Parar: cortar frames, drenar el encoder y soltar el pipeline en este hilo.
        let _ = pipe.frame_pool.RemoveFrameArrived(pipe.token);
        let _ = pipe.session.Close();
        let _ = pipe.frame_pool.Close();
        drop(pipe);
        unsafe { CoUninitialize() };
    }

    struct ReplayPipeline {
        _device: ID3D11Device,
        _manager: IMFDXGIDeviceManager,
        converter: IMFTransform,
        encoder: IMFTransform,
        // None => encoder síncrono por software (no genera eventos): se bombea distinto.
        enc_events: Option<IMFMediaEventGenerator>,
        nv12_pool: Vec<IMFSample>,
        nv12_next: std::cell::Cell<usize>,
        converter_provides: bool,
        // Solo en el camino software: lleva el frame BGRA de GPU a CPU para alimentar
        // los MFT por software (que no leen texturas D3D).
        sw: Option<SwReadback>,
        frame_pool: Direct3D11CaptureFramePool,
        session: GraphicsCaptureSession,
        token: i64,
        rx: mpsc::Receiver<(SendTex, i64)>,
    }
    unsafe impl Send for ReplayPipeline {}

    struct SwReadback {
        ctx: ID3D11DeviceContext,
        staging: ID3D11Texture2D,
        width: u32,
        height: u32,
    }

    fn build_replay(
        buffer: &Arc<Mutex<ReplayBuffer>>,
        stats: &Arc<Stats>,
        item: GraphicsCaptureItem,
        fps: u32,
        factor: f64,
        resolution: u32,
    ) -> Result<ReplayPipeline> {
        ensure_mf();
        let (device, d3d_device) = create_device()?;
        // NV12/H.264 exigen dimensiones PARES. La ventana de un juego puede tener tamaño
        // impar (a diferencia de un monitor), lo que daba MF_E_INVALIDMEDIATYPE. Se redondea
        // a par hacia abajo y el frame pool se crea a ese tamaño (recorta ≤1px, imperceptible).
        let mut size = item.Size()?;
        size.Width = size.Width.max(2) & !1;
        size.Height = size.Height.max(2) & !1;
        let width = size.Width as u32;
        let height = size.Height as u32;
        // Captura a nativo (width/height); el conversor escala al objetivo (out_*), que es
        // la resolución codificada y guardada. El bitrate se calcula sobre la salida.
        let (out_w, out_h) = output_dims(width, height, resolution);
        let bitrate = target_bitrate(out_w, out_h, fps, factor);

        {
            let mut b = buffer.lock().unwrap();
            b.width = out_w;
            b.height = out_h;
            b.fps = fps;
            b.bitrate = bitrate;
        }

        // Device manager compartido (zero-copy GPU) y device protegido para multihilo.
        let mut token = 0u32;
        let mut manager: Option<IMFDXGIDeviceManager> = None;
        unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager)? };
        let manager = manager.unwrap();
        unsafe { manager.ResetDevice(&device, token)? };
        let ctx = unsafe { device.GetImmediateContext()? };
        if let Ok(mt) = ctx.cast::<ID3D11Multithread>() {
            let _ = unsafe { mt.SetMultithreadProtected(true) };
        }

        // Encoder primero: si no hay H.264 por hardware (o se fuerza), cae a software
        // (MFT síncrono, codifica en CPU). El resto del pipeline se adapta a ese modo.
        // El encoder trabaja ya en la resolución de salida (out_*).
        let (encoder, enc_events) = build_encoder(&manager, out_w, out_h, fps, bitrate)?;
        let software = enc_events.is_none();

        // El conversor BGRA→NV12 escala de captura (width/height) a salida (out_*): va en
        // GPU con el encoder por hardware, y en software (sin device manager, CPU) si no.
        let converter = build_converter(
            if software { None } else { Some(&manager) },
            width,
            height,
            out_w,
            out_h,
            fps,
        )?;

        // Pool de salidas NV12 (a resolución de salida) para el conversor si no las provee
        // él: GPU en el camino hardware, memoria de sistema en el software.
        let converter_provides = unsafe {
            let info = converter.GetOutputStreamInfo(0)?;
            info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0
        };
        let nv12_pool = if converter_provides {
            Vec::new()
        } else if software {
            create_nv12_cpu_samples(out_w, out_h, 16)?
        } else {
            create_nv12_samples(&device, out_w, out_h, 16)?
        };

        // Readback GPU→CPU solo en software: textura de staging + un handle al contexto
        // inmediato (compartido y protegido para multihilo) para volcar cada frame BGRA.
        let sw = if software {
            Some(SwReadback {
                ctx: unsafe { device.GetImmediateContext()? },
                staging: create_staging_bgra(&device, width, height)?,
                width,
                height,
            })
        } else {
            None
        };

        // Anillo BGRA para FrameArrived: copia GPU→GPU y manda al hilo de bombeo.
        let bgra_ring = create_bgra_textures(&device, width, height, 10)?;
        let (tx, rx) = mpsc::channel::<(SendTex, i64)>();
        let feed = Arc::new(FeedCtx {
            ctx,
            ring: bgra_ring,
            next: AtomicUsize::new(0),
            tx,
        });

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let session = frame_pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsBorderRequired(false);

        // Límite de FPS: descartar frames antes de la copia GPU y del canal para no
        // codificar de más (clave para los perfiles ligeros tipo 480p/20 FPS).
        let interval = fps_interval(fps);
        let last_kept = Arc::new(AtomicI64::new(i64::MIN));
        let stats = stats.clone();
        let feed_h = feed.clone();
        let handler = TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
            move |pool, _| {
                if let Some(pool) = pool.as_ref() {
                    if let Ok(frame) = pool.TryGetNextFrame() {
                        let t = frame.SystemRelativeTime().map(|x| x.Duration).unwrap_or(0);
                        if keep_frame(&last_kept, t, interval) {
                            if let Ok(surface) = frame.Surface() {
                                if let Ok(access) = surface.cast::<IDirect3DDxgiInterfaceAccess>() {
                                    if let Ok(tex) =
                                        unsafe { access.GetInterface::<ID3D11Texture2D>() }
                                    {
                                        let idx = feed_h.next.fetch_add(1, Ordering::Relaxed)
                                            % feed_h.ring.len();
                                        let dst = feed_h.ring[idx].clone();
                                        unsafe { feed_h.ctx.CopyResource(&dst, &tex) };
                                        let _ = feed_h.tx.send((SendTex(dst), t));
                                    }
                                }
                            }
                            stats.frames.fetch_add(1, Ordering::Relaxed);
                        }
                        let _ = frame.Close();
                    }
                }
                Ok(())
            },
        );
        let token = frame_pool.FrameArrived(&handler)?;

        // Arrancar los MFT en streaming y la sesión WGC. START_OF_STREAM solo aplica al
        // MFT asíncrono (hardware); el síncrono por software no lo necesita.
        unsafe {
            converter.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            encoder.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            if enc_events.is_some() {
                encoder.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            }
        }
        session.StartCapture()?;

        Ok(ReplayPipeline {
            _device: device,
            _manager: manager,
            converter,
            encoder,
            enc_events,
            nv12_pool,
            nv12_next: std::cell::Cell::new(0),
            converter_provides,
            sw,
            frame_pool,
            session,
            token,
            rx,
        })
    }

    // Conversor de color + escalado: entrada ARGB32 a resolución de captura (in_*),
    // salida NV12 a resolución objetivo (out_*). Si difieren, el Video Processor escala.
    fn build_converter(
        manager: Option<&IMFDXGIDeviceManager>,
        in_w: u32,
        in_h: u32,
        out_w: u32,
        out_h: u32,
        fps: u32,
    ) -> Result<IMFTransform> {
        let converter: IMFTransform =
            unsafe { CoCreateInstance(&CLSID_VIDEO_PROCESSOR_MFT, None, CLSCTX_INPROC_SERVER)? };
        // Con device manager convierte en GPU (zero-copy); sin él, en CPU (camino software).
        if let Some(manager) = manager {
            unsafe {
                let unk: windows::core::IUnknown = manager.cast()?;
                converter.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, unk.as_raw() as usize)?;
            }
        }

        let out_type = unsafe { MFCreateMediaType()? };
        unsafe {
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out_type.SetUINT64(&MF_MT_FRAME_SIZE, pack2(out_w, out_h))?;
            out_type.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1))?;
            out_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack2(1, 1))?;
        }

        let in_type = unsafe { MFCreateMediaType()? };
        unsafe {
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)?;
            in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            in_type.SetUINT64(&MF_MT_FRAME_SIZE, pack2(in_w, in_h))?;
            in_type.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1))?;
            in_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack2(1, 1))?;
        }

        unsafe {
            converter.SetInputType(0, &in_type, 0)?;
            converter.SetOutputType(0, &out_type, 0)?;
        }
        Ok(converter)
    }

    // Selecciona el encoder H.264: hardware (MFT asíncrono, zero-copy GPU) si lo hay, o
    // software (MFT síncrono, CPU) como fallback. Devuelve el generador de eventos solo
    // en el caso asíncrono; None marca el camino software. FLASHBACK_FORCE_SW_ENCODER
    // fuerza el software para poder validarlo en equipos que sí tienen hardware.
    fn build_encoder(
        manager: &IMFDXGIDeviceManager,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<(IMFTransform, Option<IMFMediaEventGenerator>)> {
        let force_sw = std::env::var_os("FLASHBACK_FORCE_SW_ENCODER").is_some();

        if !force_sw {
            if let Some(activate) = enum_encoder(MFT_ENUM_FLAG_HARDWARE)? {
                let encoder: IMFTransform = unsafe { activate.ActivateObject()? };
                // Desbloquear el MFT asíncrono y compartir el device (zero-copy GPU).
                unsafe {
                    let attrs = encoder.GetAttributes()?;
                    attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
                    let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
                    let unk: windows::core::IUnknown = manager.cast()?;
                    encoder.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, unk.as_raw() as usize)?;
                }
                configure_encoder_types(&encoder, width, height, fps, bitrate)?;
                let events: IMFMediaEventGenerator = encoder.cast()?;
                return Ok((encoder, Some(events)));
            }
        }

        // Fallback por software: MFT síncrono, sin device manager (codifica en CPU).
        let activate = enum_encoder(MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_TRANSCODE_ONLY)?
            .ok_or_else(|| windows::core::Error::from_hresult(MF_E_TOPO_CODEC_NOT_FOUND))?;
        let encoder: IMFTransform = unsafe { activate.ActivateObject()? };
        configure_encoder_types(&encoder, width, height, fps, bitrate)?;
        Ok((encoder, None))
    }

    // Primer encoder H.264 que cumple los flags, o None si no hay ninguno.
    fn enum_encoder(flags: MFT_ENUM_FLAG) -> Result<Option<IMFActivate>> {
        let info = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        unsafe {
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                flags | MFT_ENUM_FLAG_SORTANDFILTER,
                None,
                Some(&info),
                &mut activates,
                &mut count,
            )?;
        }
        if count == 0 || activates.is_null() {
            return Ok(None);
        }
        let first = unsafe { (*activates).clone() };
        // Soltar y liberar el array de activates.
        for i in 0..count as usize {
            unsafe {
                let _ = std::ptr::read(activates.add(i));
            }
        }
        unsafe { CoTaskMemFree(Some(activates as *const _)) };
        Ok(first)
    }

    // Tipos de medio del encoder (output H.264 antes que input NV12, como exigen) + IDR
    // periódico. Común a hardware y software.
    fn configure_encoder_types(
        encoder: &IMFTransform,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<()> {
        let out_type = unsafe { MFCreateMediaType()? };
        unsafe {
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            // Perfil Baseline (66): sin B-frames, así el encoder emite los paquetes en el
            // mismo orden que entran. Eso nos deja emparejar cada salida con el timestamp
            // de entrada real por FIFO en vez de fiarnos del que escupe el MFT (poco
            // fiable: a veces sale a 0 y el MP4 queda con duración nula).
            out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, 66)?;
            out_type.SetUINT64(&MF_MT_FRAME_SIZE, pack2(width, height))?;
            out_type.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1))?;
            out_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack2(1, 1))?;
            encoder.SetOutputType(0, &out_type, 0)?;
        }

        let in_type = unsafe { MFCreateMediaType()? };
        unsafe {
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            in_type.SetUINT64(&MF_MT_FRAME_SIZE, pack2(width, height))?;
            in_type.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1))?;
            in_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack2(1, 1))?;
            encoder.SetInputType(0, &in_type, 0)?;
        }

        // IDR periódico (~2 s): así guardar el replay arranca siempre en un keyframe
        // reciente y el buffer se poda con eficiencia (trim corta hasta el último IDR).
        // Best-effort vía ICodecAPI: si el encoder no expone estas propiedades se ignora
        // y queda el GOP por defecto. DefaultBPictureCount=0 refuerza Baseline (sin
        // reordenado), de lo que depende el emparejado por FIFO de timestamps.
        if let Ok(codec) = encoder.cast::<ICodecAPI>() {
            let gop = (fps.max(1) * 2).max(1);
            unsafe {
                let _ = codec.SetValue(&CODECAPI_AVEncMPVGOPSize, &variant_u32(gop));
                let _ = codec.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &variant_u32(0));
            }
        }
        Ok(())
    }

    fn run_pump(
        pipe: &ReplayPipeline,
        stop: &Arc<AtomicBool>,
        buffer: &Arc<Mutex<ReplayBuffer>>,
    ) {
        if pipe.enc_events.is_some() {
            run_pump_async(pipe, stop, buffer);
        } else {
            run_pump_sync(pipe, stop, buffer);
        }
    }

    // Bombeo del encoder por hardware (MFT asíncrono, dirigido por eventos): acumula
    // crédito de NEED_INPUT y alimenta según lo pide el encoder.
    fn run_pump_async(
        pipe: &ReplayPipeline,
        stop: &Arc<AtomicBool>,
        buffer: &Arc<Mutex<ReplayBuffer>>,
    ) {
        let events = pipe.enc_events.as_ref().expect("async pump requiere eventos");
        let mut need: i32 = 0;
        let mut base: Option<i64> = None;
        let mut seq_grabbed = false;
        let mut pending: VecDeque<(SendTex, i64)> = VecDeque::new();
        // Timestamps de entrada en el orden en que se alimentan al encoder. Como el
        // encoder va en Baseline (sin reordenar), la N-ésima salida corresponde al
        // N-ésimo timestamp aquí: así fijamos el tiempo real del frame en cada paquete.
        let mut pts_fifo: VecDeque<i64> = VecDeque::new();

        while !stop.load(Ordering::SeqCst) {
            match pipe.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(f) => pending.push_back(f),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            // Drenar eventos del encoder asíncrono sin bloquear.
            loop {
                let ev = unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) };
                let ev = match ev {
                    Ok(ev) => ev,
                    Err(_) => break,
                };
                let et = unsafe { ev.GetType().unwrap_or(0) };
                if et == ME_TRANSFORM_NEED_INPUT {
                    need += 1;
                } else if et == ME_TRANSFORM_HAVE_OUTPUT {
                    let _ = drain_encoder_output(pipe, buffer, &mut seq_grabbed, &mut pts_fifo);
                }
            }

            while need > 0 {
                let Some((tex, time)) = pending.pop_front() else {
                    break;
                };
                let rebased = match base {
                    Some(b) => time - b,
                    None => {
                        base = Some(time);
                        0
                    }
                };

                // BGRA→NV12. Si el conversor no da salida (o falla) saltamos el frame
                // pero NO descontamos `need`: el encoder sigue esperando esa entrada,
                // así que la cubre el siguiente frame (si lo descontáramos, el encoder
                // se quedaría esperando para siempre y el pipeline se atascaría).
                let nv12 = match convert_frame(pipe, &tex.0, rebased) {
                    Ok(Some(s)) => s,
                    Ok(None) => continue,
                    Err(_) => continue,
                };

                // Entregar al encoder. El MFT por hardware a veces emite NEED_INPUT del
                // frame siguiente antes de drenar la salida del anterior; si rechaza la
                // entrada (NOTACCEPTING) drenamos la salida pendiente y reintentamos.
                let mut fed_ok = false;
                for _ in 0..64 {
                    let hr = unsafe { pipe.encoder.ProcessInput(0, &nv12, 0) };
                    match hr {
                        Ok(()) => {
                            fed_ok = true;
                            break;
                        }
                        Err(e) if e.code() == MF_E_NOTACCEPTING => {
                            let n = drain_encoder_output(pipe, buffer, &mut seq_grabbed, &mut pts_fifo)
                                .unwrap_or(0);
                            if n == 0 {
                                // Nada que drenar aún: dar un respiro al hardware.
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                        Err(_) => break,
                    }
                }

                if fed_ok {
                    pts_fifo.push_back(rebased);
                    need -= 1;
                } else {
                    // No se pudo alimentar (encoder ocupado). Conservamos el crédito de
                    // `need` y reintentamos en la próxima vuelta; cortamos aquí para no
                    // martillear al encoder con el resto de frames pendientes.
                    break;
                }
            }
        }
    }

    // Bombeo del encoder por software (MFT síncrono): no hay eventos; por cada frame se
    // convierte, se entrega con ProcessInput y se drena la salida hasta NEED_MORE_INPUT.
    fn run_pump_sync(
        pipe: &ReplayPipeline,
        stop: &Arc<AtomicBool>,
        buffer: &Arc<Mutex<ReplayBuffer>>,
    ) {
        let mut base: Option<i64> = None;
        let mut seq_grabbed = false;
        let mut pts_fifo: VecDeque<i64> = VecDeque::new();

        while !stop.load(Ordering::SeqCst) {
            let (tex, time) = match pipe.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(f) => f,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let rebased = match base {
                Some(b) => time - b,
                None => {
                    base = Some(time);
                    0
                }
            };

            let nv12 = match convert_frame(pipe, &tex.0, rebased) {
                Ok(Some(s)) => s,
                _ => continue,
            };

            let mut fed_ok = false;
            for _ in 0..64 {
                match unsafe { pipe.encoder.ProcessInput(0, &nv12, 0) } {
                    Ok(()) => {
                        fed_ok = true;
                        break;
                    }
                    Err(e) if e.code() == MF_E_NOTACCEPTING => {
                        let _ = drain_encoder_output(pipe, buffer, &mut seq_grabbed, &mut pts_fifo);
                    }
                    Err(_) => break,
                }
            }
            if fed_ok {
                pts_fifo.push_back(rebased);
                let _ = drain_encoder_output(pipe, buffer, &mut seq_grabbed, &mut pts_fifo);
            }
        }
    }

    // BGRA→NV12 con el conversor (síncrono). Devuelve la muestra NV12 con su tiempo
    // ya fijado, o None si el conversor no produjo salida para esta entrada.
    //
    // Tras un ProcessInput hay que drenar con ProcessOutput **hasta** MF_E_TRANSFORM_
    // NEED_MORE_INPUT; si no, el MFT se queda "con salida pendiente" y rechaza el
    // siguiente input con NOTACCEPTING (justo lo que pasaba a partir del 2º frame).
    fn convert_frame(
        pipe: &ReplayPipeline,
        bgra: &ID3D11Texture2D,
        time: i64,
    ) -> Result<Option<IMFSample>> {
        let in_sample = make_converter_input(pipe, bgra, time)?;
        unsafe { pipe.converter.ProcessInput(0, &in_sample, 0)? };

        let mut result: Option<IMFSample> = None;
        loop {
            let mut out = MFT_OUTPUT_DATA_BUFFER::default();
            if !pipe.converter_provides {
                let idx = pipe.nv12_next.get();
                pipe.nv12_next.set((idx + 1) % pipe.nv12_pool.len().max(1));
                out.pSample = ManuallyDrop::new(Some(pipe.nv12_pool[idx].clone()));
            }
            let mut status = 0u32;
            let hr = unsafe {
                pipe.converter
                    .ProcessOutput(0, std::slice::from_mut(&mut out), &mut status)
            };
            let taken = unsafe { ManuallyDrop::take(&mut out.pSample) };
            match hr {
                Ok(()) => {
                    if result.is_none() {
                        if let Some(s) = taken {
                            unsafe {
                                s.SetSampleTime(time)?;
                                s.SetSampleDuration(166_667)?;
                            }
                            result = Some(s);
                        }
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(e) => return Err(e),
            }
        }
        Ok(result)
    }

    // Muestra de entrada (ARGB32) para el conversor. En hardware envuelve la textura BGRA
    // directamente (zero-copy GPU); en software la baja a memoria de sistema por staging.
    fn make_converter_input(
        pipe: &ReplayPipeline,
        bgra: &ID3D11Texture2D,
        time: i64,
    ) -> Result<IMFSample> {
        if let Some(sw) = &pipe.sw {
            return readback_argb(sw, bgra, time);
        }
        let buffer = unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, bgra, 0, false)? };
        let len = unsafe { buffer.cast::<IMF2DBuffer>()?.GetContiguousLength()? };
        unsafe { buffer.SetCurrentLength(len)? };
        let in_sample = unsafe { MFCreateSample()? };
        unsafe {
            in_sample.AddBuffer(&buffer)?;
            in_sample.SetSampleTime(time)?;
            in_sample.SetSampleDuration(166_667)?;
        }
        Ok(in_sample)
    }

    // Vuelca la textura BGRA de GPU a una muestra ARGB32 en memoria de sistema vía una
    // textura de staging (CPU read). Solo en el camino software; aquí sí hay copia
    // GPU→CPU porque los MFT por software no leen texturas D3D.
    fn readback_argb(sw: &SwReadback, bgra: &ID3D11Texture2D, time: i64) -> Result<IMFSample> {
        let stride = sw.width as usize * 4;
        let total = stride * sw.height as usize;
        unsafe {
            sw.ctx.CopyResource(&sw.staging, bgra);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            sw.ctx
                .Map(&sw.staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let buffer = MFCreateMemoryBuffer(total as u32)?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None)?;
            for row in 0..sw.height as usize {
                let src = (map.pData as *const u8).add(row * map.RowPitch as usize);
                std::ptr::copy_nonoverlapping(src, ptr.add(row * stride), stride);
            }
            buffer.Unlock()?;
            buffer.SetCurrentLength(total as u32)?;
            sw.ctx.Unmap(&sw.staging, 0);

            let in_sample = MFCreateSample()?;
            in_sample.AddBuffer(&buffer)?;
            in_sample.SetSampleTime(time)?;
            in_sample.SetSampleDuration(166_667)?;
            Ok(in_sample)
        }
    }

    fn drain_encoder_output(
        pipe: &ReplayPipeline,
        buffer: &Arc<Mutex<ReplayBuffer>>,
        seq_grabbed: &mut bool,
        pts_fifo: &mut VecDeque<i64>,
    ) -> Result<usize> {
        let mut drained = 0usize;
        loop {
            let mut out = MFT_OUTPUT_DATA_BUFFER::default();
            let mut status = 0u32;
            let hr = unsafe { pipe.encoder.ProcessOutput(0, std::slice::from_mut(&mut out), &mut status) };
            match hr {
                Ok(()) => {}
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(e) => return Err(e),
            }

            if !*seq_grabbed {
                if let Ok(mt) = unsafe { pipe.encoder.GetOutputCurrentType(0) } {
                    if let Some(h) = blob(&mt, &MF_MT_MPEG_SEQUENCE_HEADER) {
                        buffer.lock().unwrap().seq_header = h;
                        *seq_grabbed = true;
                    }
                }
            }

            let sample = unsafe { ManuallyDrop::take(&mut out.pSample) };
            if let Some(sample) = sample {
                if let Some((data, enc_time, dur, key)) = read_sample(&sample) {
                    // El tiempo real lo pone el FIFO de entrada; el del encoder solo
                    // sirve de respaldo si por algún motivo el FIFO se vaciara.
                    let time = pts_fifo.pop_front().unwrap_or(enc_time);
                    buffer.lock().unwrap().push(data, time, dur, key);
                    drained += 1;
                }
            }
        }
        Ok(drained)
    }

    fn read_sample(sample: &IMFSample) -> Option<(Vec<u8>, i64, i64, bool)> {
        unsafe {
            let buf = sample.ConvertToContiguousBuffer().ok()?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut cur = 0u32;
            buf.Lock(&mut ptr, None, Some(&mut cur)).ok()?;
            let data = std::slice::from_raw_parts(ptr, cur as usize).to_vec();
            let _ = buf.Unlock();
            let time = sample.GetSampleTime().unwrap_or(0);
            let dur = sample.GetSampleDuration().unwrap_or(166_667);
            let key = sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) == 1;
            Some((data, time, dur, key))
        }
    }

    fn mux_replay(
        path: &str,
        packets: &[(Vec<u8>, i64, i64, bool)],
        seq_header: &[u8],
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<()> {
        ensure_mf();
        let url = HSTRING::from(path);

        // Byte stream propio (seekable) para pedir faststart: el sink de MPEG-4 escribe
        // el `moov` (índice) ANTES del `mdat`, así el reproductor empieza al instante en
        // vez de escanear el archivo entero al abrir (lo que daba ~10 s en negro). El
        // atributo MF_MPEG4SINK_MOOV_BEFORE_MDAT se lee del byte stream.
        let byte_stream = unsafe {
            MFCreateFile(
                MF_ACCESSMODE_READWRITE,
                MF_OPENMODE_DELETE_IF_EXIST,
                MF_FILEFLAGS_NONE,
                &url,
            )?
        };
        if let Ok(bs_attr) = byte_stream.cast::<IMFAttributes>() {
            unsafe {
                let _ = bs_attr.SetUINT32(&MF_MPEG4SINK_MOOV_BEFORE_MDAT, 1);
            }
        }

        let attrs = unsafe {
            let mut a: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut a, 2)?;
            let a = a.unwrap();
            a.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
            // Sin URL no se infiere el contenedor: hay que decirlo explícitamente.
            a.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?;
            a
        };
        let writer =
            unsafe { MFCreateSinkWriterFromURL(PCWSTR::null(), &byte_stream, &attrs)? };

        let h264 = unsafe { MFCreateMediaType()? };
        unsafe {
            h264.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            h264.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            h264.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            h264.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            h264.SetUINT64(&MF_MT_FRAME_SIZE, pack2(width, height))?;
            h264.SetUINT64(&MF_MT_FRAME_RATE, pack2(fps, 1))?;
            h264.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack2(1, 1))?;
            if !seq_header.is_empty() {
                h264.SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, seq_header)?;
            }
        }
        let stream = unsafe { writer.AddStream(&h264)? };
        // Passthrough: el input del stream es el mismo H.264 ya codificado.
        unsafe { writer.SetInputMediaType(stream, &h264, None)? };
        unsafe { writer.BeginWriting()? };

        let base = packets[0].1;
        let n = packets.len();
        for i in 0..n {
            let data = &packets[i].0;
            let time = packets[i].1;
            let key = packets[i].3;
            // Duración = salto al siguiente frame (captura VFR: WGC no manda duplicados,
            // así que el ritmo real lo marcan los timestamps). El último usa un valor
            // nominal de 1/60 s. De aquí sale la duración correcta del MP4.
            let dur = if i + 1 < n {
                (packets[i + 1].1 - time).max(1)
            } else {
                166_667
            };
            let mf_buf = unsafe { MFCreateMemoryBuffer(data.len() as u32)? };
            unsafe {
                let mut ptr: *mut u8 = std::ptr::null_mut();
                mf_buf.Lock(&mut ptr, None, None)?;
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
                mf_buf.Unlock()?;
                mf_buf.SetCurrentLength(data.len() as u32)?;
            }
            let sample = unsafe { MFCreateSample()? };
            unsafe {
                sample.AddBuffer(&mf_buf)?;
                sample.SetSampleTime(time - base)?;
                sample.SetSampleDuration(dur)?;
                if key {
                    sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
                }
                writer.WriteSample(stream, &sample)?;
            }
        }
        unsafe { writer.Finalize()? };
        Ok(())
    }

    fn blob(mt: &IMFMediaType, key: &GUID) -> Option<Vec<u8>> {
        unsafe {
            let size = mt.GetBlobSize(key).ok()?;
            if size == 0 {
                return None;
            }
            let mut v = vec![0u8; size as usize];
            mt.GetBlob(key, &mut v, None).ok()?;
            Some(v)
        }
    }

    fn create_bgra_textures(
        device: &ID3D11Device,
        width: u32,
        height: u32,
        count: usize,
    ) -> Result<Vec<ID3D11Texture2D>> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let mut t: Option<ID3D11Texture2D> = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut t))? };
            out.push(t.unwrap());
        }
        Ok(out)
    }

    fn create_nv12_samples(
        device: &ID3D11Device,
        width: u32,
        height: u32,
        count: usize,
    ) -> Result<Vec<IMFSample>> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let mut t: Option<ID3D11Texture2D> = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut t))? };
            let tex = t.unwrap();
            let buf = unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &tex, 0, false)? };
            let sample = unsafe { MFCreateSample()? };
            unsafe { sample.AddBuffer(&buf)? };
            out.push(sample);
        }
        Ok(out)
    }

    // Salidas NV12 en memoria de sistema para el conversor por software (NV12 contiguo:
    // plano Y de w*h + plano UV intercalado de w*h/2).
    fn create_nv12_cpu_samples(width: u32, height: u32, count: usize) -> Result<Vec<IMFSample>> {
        let size = width * height + (width * height) / 2;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let buf = unsafe { MFCreateMemoryBuffer(size)? };
            let sample = unsafe { MFCreateSample()? };
            unsafe { sample.AddBuffer(&buf)? };
            out.push(sample);
        }
        Ok(out)
    }

    // Textura de staging BGRA legible por CPU para el readback del camino software.
    fn create_staging_bgra(
        device: &ID3D11Device,
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut t: Option<ID3D11Texture2D> = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut t))? };
        Ok(t.unwrap())
    }

    fn pack2(high: u32, low: u32) -> u64 {
        ((high as u64) << 32) | low as u64
    }

    // Bits por píxel y frame según calidad: a más factor, más bitrate (y tamaño).
    fn bitrate_factor(quality: &str) -> f64 {
        match quality {
            "low" => 0.04,
            "normal" => 0.06,
            "ultra" => 0.12,
            _ => 0.08, // "high" (por defecto)
        }
    }

    fn clamp_fps(fps: u32) -> u32 {
        fps.clamp(10, 240)
    }

    fn target_bitrate(width: u32, height: u32, fps: u32, factor: f64) -> u32 {
        (((width as u64 * height as u64 * fps as u64) as f64 * factor) as u32).max(2_000_000)
    }

    // Dimensiones de salida dadas las de captura y un alto objetivo (0 = nativo). Se
    // mantiene el aspecto, se redondea a par (NV12/H.264) y NUNCA se hace upscaling:
    // si el objetivo es >= al nativo se graba a nativo (subir resolución solo infla el
    // archivo sin añadir detalle). El escalado real lo hace el encoder/conversor.
    fn output_dims(cap_w: u32, cap_h: u32, target_h: u32) -> (u32, u32) {
        if target_h == 0 || target_h >= cap_h || cap_h == 0 {
            return (cap_w, cap_h);
        }
        let out_h = (target_h & !1).max(2);
        let out_w = ((((cap_w as u64 * out_h as u64) / cap_h as u64) as u32) & !1).max(2);
        (out_w, out_h)
    }

    // Intervalo mínimo (en unidades de 100 ns) entre frames codificados para no superar
    // los FPS objetivo. 0 = sin límite. El límite real se aplica descartando frames en el
    // handler de captura, antes de tocar la GPU/encoder: menos FPS = menos trabajo.
    fn fps_interval(fps: u32) -> i64 {
        if fps == 0 {
            0
        } else {
            10_000_000 / fps as i64
        }
    }

    // Decide si conservar un frame con timestamp `t` dado el último conservado en
    // `last` (i64::MIN = ninguno aún). Mantiene la cadencia sin acumular deriva.
    fn keep_frame(last: &AtomicI64, t: i64, interval: i64) -> bool {
        if interval <= 0 {
            return true;
        }
        let prev = last.load(Ordering::Relaxed);
        if prev == i64::MIN || t - prev >= interval - interval / 10 {
            last.store(t, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    // VARIANT de tipo VT_UI4 para las propiedades de ICodecAPI (GOP, etc.).
    fn variant_u32(val: u32) -> VARIANT {
        let mut v = VARIANT::default();
        unsafe {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt = VT_UI4;
            inner.Anonymous.ulVal = val;
        }
        v
    }

    fn clip_filename() -> String {
        let st: SYSTEMTIME = unsafe { GetLocalTime() };
        format!(
            "Flashback_{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.mp4",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
        )
    }

    fn capture_item_for_monitor(monitor: HMONITOR) -> Result<GraphicsCaptureItem> {
        let interop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        unsafe { interop.CreateForMonitor(monitor) }
    }

    fn capture_item_for_window(hwnd: HWND) -> Result<GraphicsCaptureItem> {
        let interop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        unsafe { interop.CreateForWindow(hwnd) }
    }

    // Resuelve el objetivo de captura a un GraphicsCaptureItem. `"window"` = la ventana
    // del juego detectado (modo Aplicación); cualquier otro valor = nombre de dispositivo
    // de monitor (`\\.\DISPLAYn`). Se ejecuta dentro del hilo de captura (apartamento MTA).
    fn resolve_target_item(target: &str) -> std::result::Result<GraphicsCaptureItem, String> {
        if target == "window" {
            let hwnd = resolve_game_window()
                .ok_or_else(|| "No hay ventana de juego para capturar".to_string())?;
            capture_item_for_window(hwnd).map_err(|e| format!("{e:?}"))
        } else {
            let hmon =
                resolve_monitor(target).ok_or_else(|| "Monitor no encontrado".to_string())?;
            capture_item_for_monitor(hmon).map_err(|e| format!("{e:?}"))
        }
    }

    // Ventana principal del juego rastreado: la top-level visible y no minimizada de
    // mayor área que pertenece a su PID. Sirve para CreateForWindow en modo Aplicación.
    fn resolve_game_window() -> Option<HWND> {
        let pid = crate::detect::current_game_pid()?;
        find_main_window(pid)
    }

    struct WinSearch {
        pid: u32,
        best: Option<HWND>,
        best_area: i32,
    }

    fn find_main_window(pid: u32) -> Option<HWND> {
        let mut search = WinSearch {
            pid,
            best: None,
            best_area: 0,
        };
        unsafe {
            let _ = EnumWindows(
                Some(window_proc),
                LPARAM(&mut search as *mut WinSearch as isize),
            );
        }
        search.best
    }

    unsafe extern "system" fn window_proc(hwnd: HWND, data: LPARAM) -> BOOL {
        let search = &mut *(data.0 as *mut WinSearch);
        let mut wpid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut wpid));
        if wpid != search.pid
            || !IsWindowVisible(hwnd).as_bool()
            || IsIconic(hwnd).as_bool()
            || GetWindow(hwnd, GW_OWNER).map(|h| !h.0.is_null()).unwrap_or(false)
        {
            return BOOL(1);
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return BOOL(1);
        }
        let area = (rect.right - rect.left) * (rect.bottom - rect.top);
        if area > search.best_area {
            search.best_area = area;
            search.best = Some(hwnd);
        }
        BOOL(1)
    }

    fn enum_monitors() -> Vec<HMONITOR> {
        let mut list: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(enum_proc),
                LPARAM(&mut list as *mut Vec<HMONITOR> as isize),
            );
        }
        list
    }

    unsafe extern "system" fn enum_proc(
        hmon: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<HMONITOR>);
        list.push(hmon);
        BOOL(1)
    }

    fn monitor_info(hmon: HMONITOR, index: usize, screen_dc: HDC) -> Option<MonitorInfo> {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let ok = unsafe { GetMonitorInfoW(hmon, &mut info as *mut _ as *mut MONITORINFO) };
        if !ok.as_bool() {
            return None;
        }
        let rc = info.monitorInfo.rcMonitor;
        Some(MonitorInfo {
            id: device_name(&info),
            label: format!("Pantalla {}", index + 1),
            width: (rc.right - rc.left).max(0) as u32,
            height: (rc.bottom - rc.top).max(0) as u32,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            thumb: snapshot(screen_dc, rc),
        })
    }

    // Foto fija de una pantalla por GDI: BitBlt reescalado a un BMP pequeño que se
    // devuelve como data URL. Es para la miniatura del selector, no para capturar.
    fn snapshot(screen_dc: HDC, rc: RECT) -> Option<String> {
        if screen_dc.is_invalid() {
            return None;
        }
        let sw = rc.right - rc.left;
        let sh = rc.bottom - rc.top;
        if sw <= 0 || sh <= 0 {
            return None;
        }
        let tw: i32 = 320;
        let th: i32 = ((tw as i64 * sh as i64) / sw as i64).max(1) as i32;

        unsafe {
            let mem = CreateCompatibleDC(Some(screen_dc));
            if mem.is_invalid() {
                return None;
            }
            let bmp = CreateCompatibleBitmap(screen_dc, tw, th);
            if bmp.is_invalid() {
                let _ = DeleteDC(mem);
                return None;
            }
            let mem_dc = HDC(mem.0);
            let old = SelectObject(mem_dc, bmp.into());
            SetStretchBltMode(mem_dc, HALFTONE);
            let blit = StretchBlt(
                mem_dc, 0, 0, tw, th, Some(screen_dc), rc.left, rc.top, sw, sh, SRCCOPY,
            );

            let result = if blit.as_bool() {
                let mut bmi = BITMAPINFO::default();
                bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = tw;
                bmi.bmiHeader.biHeight = th; // positivo => bottom-up, como espera el BMP
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = 0; // BI_RGB
                let mut pixels = vec![0u8; (tw * th * 4) as usize];
                let lines = GetDIBits(
                    mem_dc,
                    bmp,
                    0,
                    th as u32,
                    Some(pixels.as_mut_ptr() as *mut _),
                    &mut bmi,
                    DIB_RGB_COLORS,
                );
                (lines != 0).then(|| bmp_data_url(tw, th, &pixels))
            } else {
                None
            };

            SelectObject(mem_dc, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            result
        }
    }

    fn bmp_data_url(w: i32, h: i32, pixels: &[u8]) -> String {
        use base64::Engine;
        let pix_size = pixels.len() as u32;
        let file_size = 14 + 40 + pix_size;
        let mut buf = Vec::with_capacity(file_size as usize);
        buf.extend_from_slice(b"BM");
        buf.extend_from_slice(&file_size.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&54u32.to_le_bytes());
        buf.extend_from_slice(&40u32.to_le_bytes());
        buf.extend_from_slice(&w.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&32u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&pix_size.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(pixels);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        format!("data:image/bmp;base64,{b64}")
    }

    // El nombre de dispositivo (\\.\DISPLAY1) es estable en la sesión: lo usamos
    // como id para volver a resolver el HMONITOR al arrancar la captura.
    fn resolve_monitor(id: &str) -> Option<HMONITOR> {
        enum_monitors().into_iter().find(|&hmon| {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            let ok = unsafe { GetMonitorInfoW(hmon, &mut info as *mut _ as *mut MONITORINFO) };
            ok.as_bool() && device_name(&info) == id
        })
    }

    fn device_name(info: &MONITORINFOEXW) -> String {
        let len = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        String::from_utf16_lossy(&info.szDevice[..len])
    }
}
