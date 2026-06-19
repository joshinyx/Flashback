// Genera una miniatura JPEG (un fotograma escalado) de un clip, para mostrarla como
// carátula ligera en la biblioteca sin cargar un elemento <video> por tarjeta. El trabajo
// pesado vive en el backend (CLAUDE.md §4): Media Foundation decodifica y escala el frame,
// y WIC lo codifica a JPEG. Sin dependencias externas.

#[cfg(target_os = "windows")]
pub use win::generate;

#[cfg(not(target_os = "windows"))]
pub fn generate(_src: String, _dst: String, _max_w: u32) -> Result<(), String> {
    Err("Las miniaturas solo están disponibles en Windows".into())
}

#[cfg(target_os = "windows")]
mod win {
    use std::sync::Once;

    use windows::core::{GUID, HSTRING};
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
        GUID_WICPixelFormat32bppBGRA, IWICBitmapFrameEncode, IWICImagingFactory,
        WICBitmapEncoderNoCache,
    };
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::StructuredStorage::IPropertyBag2;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };

    // Selector "primer flujo de vídeo" del SourceReader y flag de fin de stream.
    const FIRST_VIDEO: u32 = 0xFFFF_FFFC;
    const ENDOFSTREAM: u32 = 0x0000_0002;
    // GENERIC_WRITE, para IWICStream::InitializeFromFilename.
    const GENERIC_WRITE: u32 = 0x4000_0000;

    static MF_INIT: Once = Once::new();
    fn ensure_mf() {
        MF_INIT.call_once(|| unsafe {
            let _ = MFStartup(MF_VERSION, MFSTARTUP_LITE);
        });
    }

    pub fn generate(src: String, dst: String, max_w: u32) -> std::result::Result<(), String> {
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            ensure_mf();
            let r = run(&src, &dst, max_w).map_err(|e| format!("{e:?}"));
            unsafe { CoUninitialize() };
            r
        })
        .join()
        .map_err(|_| "El hilo de miniatura terminó inesperadamente".to_string())?
    }

    fn run(src: &str, dst: &str, max_w: u32) -> windows::core::Result<()> {
        // El SourceReader con procesamiento de vídeo avanzado puede entregar el frame ya
        // convertido a RGB32 y escalado al tamaño pedido, sin pasos manuales de color/escala.
        let mut attrs: Option<IMFAttributes> = None;
        unsafe { MFCreateAttributes(&mut attrs, 1)? };
        let attrs = attrs.unwrap();
        unsafe { attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)? };

        let url = HSTRING::from(src);
        let reader = unsafe { MFCreateSourceReaderFromURL(&url, &attrs)? };

        let native = unsafe { reader.GetNativeMediaType(FIRST_VIDEO, 0)? };
        let size = unsafe { native.GetUINT64(&MF_MT_FRAME_SIZE) }.unwrap_or(pack(1280, 720));
        let sw = (size >> 32) as u32;
        let sh = (size & 0xFFFF_FFFF) as u32;
        let (tw, th) = target_size(sw.max(1), sh.max(1), max_w);

        let out = unsafe { MFCreateMediaType()? };
        unsafe {
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack(tw, th))?;
            reader.SetCurrentMediaType(FIRST_VIDEO, None, &out)?;
        }

        // El procesador de vídeo puede no escalar exactamente al tamaño pedido; tomamos las
        // dimensiones y el stride realmente negociados para no cizallar la imagen.
        let actual = unsafe { reader.GetCurrentMediaType(FIRST_VIDEO)? };
        let asize = unsafe { actual.GetUINT64(&MF_MT_FRAME_SIZE) }.unwrap_or(pack(tw, th));
        let aw = ((asize >> 32) as u32).max(1);
        let ah = ((asize & 0xFFFF_FFFF) as u32).max(1);
        let ds = unsafe { actual.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.unwrap_or(0) as i32;

        // Primer frame con datos (algunos contenedores entregan muestras vacías al inicio).
        let mut frame = None;
        for _ in 0..120 {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            unsafe {
                reader.ReadSample(
                    FIRST_VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )?
            };
            if flags & ENDOFSTREAM != 0 {
                break;
            }
            if let Some(s) = sample {
                frame = Some(s);
                break;
            }
        }
        let sample = frame.ok_or_else(|| {
            windows::core::Error::from(windows::core::HRESULT(0x8000_4005u32 as i32))
        })?;

        let buf = unsafe { sample.ConvertToContiguousBuffer()? };
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        unsafe { buf.Lock(&mut ptr, None, Some(&mut len))? };
        let src = unsafe { std::slice::from_raw_parts(ptr, len as usize) };

        // Empaquetamos a un buffer top-down y sin relleno (stride = aw*4). El stride origen
        // sale del MF_MT_DEFAULT_STRIDE (su signo indica orientación) o, en su defecto, de
        // len/altura. Copiar fila a fila evita cizallado por relleno y arregla bottom-up.
        let dst_stride = (aw * 4) as usize;
        let src_stride = if ds != 0 {
            ds.unsigned_abs() as usize
        } else {
            (len as usize / ah as usize).max(dst_stride)
        };
        let bottom_up = ds < 0;
        let mut packed = vec![0u8; dst_stride * ah as usize];
        for row in 0..ah as usize {
            let src_row = if bottom_up { ah as usize - 1 - row } else { row };
            let s = src_row * src_stride;
            let d = row * dst_stride;
            if s + dst_stride <= src.len() {
                packed[d..d + dst_stride].copy_from_slice(&src[s..s + dst_stride]);
            }
        }
        unsafe { buf.Unlock()? };

        encode_jpeg(dst, aw, ah, dst_stride as u32, &packed)
    }

    fn encode_jpeg(
        dst: &str,
        w: u32,
        h: u32,
        stride: u32,
        pixels: &[u8],
    ) -> windows::core::Result<()> {
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };

        let stream = unsafe { factory.CreateStream()? };
        unsafe { stream.InitializeFromFilename(&HSTRING::from(dst), GENERIC_WRITE)? };

        let encoder = unsafe { factory.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())? };
        unsafe { encoder.Initialize(&stream, WICBitmapEncoderNoCache)? };

        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut props: Option<IPropertyBag2> = None;
        unsafe { encoder.CreateNewFrame(&mut frame, &mut props)? };
        let frame = frame.unwrap();
        unsafe { frame.Initialize(props.as_ref())? };
        unsafe { frame.SetSize(w, h)? };

        // JPEG no admite alfa, así que el encoder fija un formato sin alfa (24bpp). En vez de
        // escribir bytes crudos (que se desalinearían 4→3), entregamos un IWICBitmap declarado
        // como 32bppBGRA y dejamos que WIC convierta al formato del frame en WriteSource.
        let mut fmt: GUID = GUID_WICPixelFormat24bppBGR;
        unsafe { frame.SetPixelFormat(&mut fmt)? };

        let bitmap = unsafe {
            factory.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppBGRA, stride, pixels)?
        };
        unsafe { frame.WriteSource(&bitmap, std::ptr::null())? };

        unsafe { frame.Commit()? };
        unsafe { encoder.Commit()? };
        Ok(())
    }

    // Escala manteniendo la relación de aspecto, sin superar `max_w`, con lados pares.
    fn target_size(sw: u32, sh: u32, max_w: u32) -> (u32, u32) {
        let w = sw.min(max_w);
        let h = ((w as u64 * sh as u64) / sw as u64) as u32;
        (w & !1, h.max(2) & !1)
    }

    fn pack(hi: u32, lo: u32) -> u64 {
        (hi as u64) << 32 | lo as u64
    }
}
