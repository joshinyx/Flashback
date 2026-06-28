use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Default)]
pub struct ClipAudio {
    pub system: Option<String>,
    pub mic: Option<String>,
    // Forma de onda ya reducida a cubos: se calcula en el backend para evitar volcar el WAV
    // completo al WebView y decodificarlo allí (cientos de MB). Solo viaja el envolvente.
    pub sys_peaks: Option<Vec<f32>>,
    pub mic_peaks: Option<Vec<f32>>,
    pub mix_peaks: Option<Vec<f32>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MixerState {
    pub sys_vol: f32,
    pub sys_muted: bool,
    pub mic_vol: f32,
    pub mic_muted: bool,
}

impl Default for MixerState {
    fn default() -> Self {
        Self {
            sys_vol: 1.0,
            sys_muted: false,
            mic_vol: 1.0,
            mic_muted: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Segment {
    pub start_ms: f64,
    pub end_ms: f64,
    // Estado del editor (posición en la línea de tiempo y tamaño máximo de la sección). La
    // exportación no los usa —une los tramos sin huecos— pero se persisten para restaurar el
    // montaje. Opcionales por compatibilidad con ediciones antiguas.
    #[serde(default)]
    pub pos_ms: Option<f64>,
    #[serde(default)]
    pub bound_start_ms: Option<f64>,
    #[serde(default)]
    pub bound_end_ms: Option<f64>,
    // Solo se persiste para restaurar el montaje; la exportación recibe ya filtrados los activos.
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClipEdit {
    pub segments: Vec<Segment>,
    pub mixer: MixerState,
}

#[cfg(target_os = "windows")]
pub use win::{
    clip_fps, export_clip, frame_times, keyframe_times, prepare_clip_audio,
};

// Edición no destructiva: cortes y mezcla viven en el índice único de app-data (no en un sidecar
// por clip), indexados por la ruta del MP4. El original nunca se toca. No usa Media Foundation,
// así que es común a todas las plataformas.
pub fn save_edit(index: String, path: String, edit: ClipEdit) -> Result<(), String> {
    let val = serde_json::to_value(&edit).map_err(|e| e.to_string())?;
    crate::edits::save(std::path::Path::new(&index), &path, val);
    Ok(())
}

pub fn load_edit(index: String, path: String) -> Result<ClipEdit, String> {
    let idx = std::path::Path::new(&index);
    if let Some(val) = crate::edits::load(idx, &path) {
        return serde_json::from_value(val).map_err(|e| e.to_string());
    }
    // Migración: importar el viejo sidecar `<clip>.edit.json` al índice y borrarlo.
    let legacy = std::path::Path::new(&path).with_extension("edit.json");
    if let Ok(s) = std::fs::read_to_string(&legacy) {
        if let Ok(edit) = serde_json::from_str::<ClipEdit>(&s) {
            if let Ok(val) = serde_json::to_value(&edit) {
                crate::edits::save(idx, &path, val);
            }
            let _ = std::fs::remove_file(&legacy);
            return Ok(edit);
        }
    }
    Ok(ClipEdit {
        segments: Vec::new(),
        mixer: MixerState::default(),
    })
}

#[cfg(not(target_os = "windows"))]
pub fn prepare_clip_audio(_path: String) -> Result<ClipAudio, String> {
    Err("El editor solo está disponible en Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn keyframe_times(_path: String) -> Result<Vec<f64>, String> {
    Err("El editor solo está disponible en Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn frame_times(_path: String) -> Result<Vec<f64>, String> {
    Err("El editor solo está disponible en Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn clip_fps(_path: String) -> Result<u32, String> {
    Err("El editor solo está disponible en Windows".into())
}

#[cfg(not(target_os = "windows"))]
pub fn export_clip<F: Fn(f32)>(_src: String, _dst: String, _edit: ClipEdit, _progress: F) -> Result<(), String> {
    Err("El editor solo está disponible en Windows".into())
}

#[cfg(target_os = "windows")]
mod win {
    use std::sync::Once;

    use windows::core::{Result, GUID, HSTRING};
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    use super::{ClipAudio, ClipEdit};

    const ALL_STREAMS: u32 = 0xFFFF_FFFE;
    const ENDOFSTREAM: u32 = 0x0000_0002;

    static MF_INIT: Once = Once::new();
    fn ensure_mf() {
        MF_INIT.call_once(|| unsafe {
            let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
        });
    }

    // Ejecuta una operación de Media Foundation en su propio hilo con COM (MTA) y MF
    // inicializados. Los SourceReader/SinkWriter exigen ese contexto; sin él, fallan en
    // silencio en el hilo de comandos de Tauri.
    fn with_mf<T, F>(f: F) -> std::result::Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> std::result::Result<T, String> + Send + 'static,
    {
        std::thread::spawn(move || {
            unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
            ensure_mf();
            let r = f();
            unsafe { CoUninitialize(); }
            r
        })
        .join()
        .map_err(|_| "El hilo de Media Foundation terminó inesperadamente".to_string())?
    }

    pub fn prepare_clip_audio(path: String, audio_dir: String) -> std::result::Result<ClipAudio, String> {
        std::thread::spawn(move || {
            unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
            ensure_mf();
            let r = extract(&path, &audio_dir);
            unsafe { CoUninitialize(); }
            r
        })
        .join()
        .map_err(|_| "El hilo de extracción de audio terminó inesperadamente".to_string())?
    }

    fn extract(path: &str, audio_dir: &str) -> std::result::Result<ClipAudio, String> {
        let mf = |e| format!("{e:?}");
        let io = |e: std::io::Error| e.to_string();

        let audio_streams = count_audio_streams(path).map_err(mf)?;

        // Sin micro no hay pistas que separar: la pista única va embebida y la reproduce el propio
        // vídeo. Aun así se calcula su forma de onda (mezcla) para dibujarla en el editor.
        if audio_streams < 2 {
            if audio_streams == 1 {
                let (pcm, _sr, ch) = read_pcm(path, 0).map_err(mf)?;
                return Ok(ClipAudio {
                    mix_peaks: Some(peaks_from_pcm(&pcm, ch)),
                    ..Default::default()
                });
            }
            return Ok(ClipAudio::default());
        }

        let key = temp_key(path);
        let dir = std::path::Path::new(audio_dir);
        // `a2` versiona el formato de extracción: los WAV anteriores se generaban sin alinear el
        // hueco inicial de cada pista, así que se descartan (nombre nuevo) y se rehacen ya alineados.
        let sys = dir
            .join(format!("flashback_edit_{key}_a2_sys.wav"))
            .to_string_lossy()
            .into_owned();
        let mic = dir
            .join(format!("flashback_edit_{key}_a2_mic.wav"))
            .to_string_lossy()
            .into_owned();

        // Los clips son inmutables (edición no destructiva): si ya se separaron las pistas en
        // una apertura anterior, se reutilizan en vez de volver a volcar cientos de MB de WAV.
        // En ese caso los picos se sacan del WAV local (lectura barata) en vez de redecodificar.
        let ready = |p: &str| std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false);
        let (sys_peaks, mic_peaks) = if ready(&sys) && ready(&mic) {
            (peaks_from_wav(&sys), peaks_from_wav(&mic))
        } else {
            let (sys_pcm, sr, sc) = read_pcm(path, 1).map_err(mf)?;
            write_wav(&sys, &sys_pcm, sr, sc).map_err(io)?;
            let sp = peaks_from_pcm(&sys_pcm, sc);
            let (mic_pcm, mr, mc) = read_pcm(path, 0).map_err(mf)?;
            write_wav(&mic, &mic_pcm, mr, mc).map_err(io)?;
            let mp = peaks_from_pcm(&mic_pcm, mc);
            (Some(sp), Some(mp))
        };

        Ok(ClipAudio {
            system: Some(sys),
            mic: Some(mic),
            sys_peaks,
            mic_peaks,
            mix_peaks: None,
        })
    }

    // Clave estable por ruta completa: evita colisiones entre clips con el mismo nombre de
    // archivo en carpetas distintas (p. ej. el original y su `_edit.mp4`).
    fn temp_key(path: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        format!("{:016x}", h.finish())
    }

    fn open_reader(path: &str) -> Result<IMFSourceReader> {
        let url = HSTRING::from(path);
        unsafe { MFCreateSourceReaderFromURL(&url, None) }
    }

    fn count_audio_streams(path: &str) -> Result<usize> {
        let reader = open_reader(path)?;
        let mut count = 0usize;
        let mut i = 0u32;
        while let Ok(mt) = unsafe { reader.GetNativeMediaType(i, 0) } {
            if unsafe { mt.GetGUID(&MF_MT_MAJOR_TYPE) }
                .map(|g| g == MFMediaType_Audio)
                .unwrap_or(false)
            {
                count += 1;
            }
            i += 1;
        }
        Ok(count)
    }

    fn read_pcm(path: &str, ordinal: usize) -> Result<(Vec<u8>, u32, u16)> {
        let reader = open_reader(path)?;
        unsafe { reader.SetStreamSelection(ALL_STREAMS, false)? };

        let mut target: Option<u32> = None;
        let mut seen = 0usize;
        let mut i = 0u32;
        while let Ok(mt) = unsafe { reader.GetNativeMediaType(i, 0) } {
            let major = unsafe { mt.GetGUID(&MF_MT_MAJOR_TYPE) }
                .unwrap_or(GUID::zeroed());
            if major == MFMediaType_Audio {
                if seen == ordinal {
                    target = Some(i);
                    break;
                }
                seen += 1;
            }
            i += 1;
        }

        let idx = target.ok_or_else(|| {
            windows::core::Error::from(windows::core::HRESULT(0x80070002u32 as i32))
        })?;

        unsafe { reader.SetStreamSelection(idx, true)? };

        let pcm_type = unsafe { MFCreateMediaType()? };
        unsafe { pcm_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)? };
        unsafe { pcm_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)? };
        unsafe { reader.SetCurrentMediaType(idx, None, &pcm_type)? };

        let actual = unsafe { reader.GetCurrentMediaType(idx)? };
        let sr = unsafe { actual.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.unwrap_or(48000);
        let ch = unsafe { actual.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.unwrap_or(2) as u16;
        let frame_bytes = (ch.max(1) as usize) * 2;

        let mut pcm = Vec::new();
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            unsafe { reader.ReadSample(idx, 0, None, Some(&mut flags), None, Some(&mut sample))? };
            if flags & ENDOFSTREAM != 0 { break; }
            let Some(sample) = sample else { continue };
            // Alinear al origen de tiempo común (t=0). Si la pista arrancó tarde —el loopback de
            // sistema de WASAPI no entrega paquetes mientras no hay sonido—, su primer sample llega
            // con timestamp > 0 y el muxer dejó ese hueco en el MP4. Rellenamos con silencio hasta
            // su posición real para que sistema y micro queden sincronizados entre sí y con el
            // vídeo; concatenar sin más comprimía el hueco y desfasaba la pista varios segundos.
            let t = unsafe { sample.GetSampleTime() }.unwrap_or(0).max(0);
            let expected = (t as f64 / 10_000_000.0 * sr as f64).round() as usize * frame_bytes;
            if pcm.len() < expected {
                pcm.resize(expected, 0);
            }
            let buf = unsafe { sample.ConvertToContiguousBuffer()? };
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut cur = 0u32;
            unsafe { buf.Lock(&mut ptr, None, Some(&mut cur))? };
            if cur > 0 {
                let slice = unsafe { std::slice::from_raw_parts(ptr, cur as usize) };
                pcm.extend_from_slice(slice);
            }
            unsafe { buf.Unlock()? };
        }

        Ok((pcm, sr, ch))
    }

    fn write_wav(path: &str, pcm: &[u8], sample_rate: u32, channels: u16) -> std::io::Result<()> {
        use std::io::Write;
        let bits = 16u16;
        let byte_rate = sample_rate * channels as u32 * (bits as u32 / 8);
        let block_align = channels * (bits / 8);
        let data_len = pcm.len() as u32;
        let file_len = 36 + data_len;

        let mut f = std::fs::File::create(path)?;
        f.write_all(b"RIFF")?;
        f.write_all(&file_len.to_le_bytes())?;
        f.write_all(b"WAVE")?;
        f.write_all(b"fmt ")?;
        f.write_all(&16u32.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?;
        f.write_all(&channels.to_le_bytes())?;
        f.write_all(&sample_rate.to_le_bytes())?;
        f.write_all(&byte_rate.to_le_bytes())?;
        f.write_all(&block_align.to_le_bytes())?;
        f.write_all(&bits.to_le_bytes())?;
        f.write_all(b"data")?;
        f.write_all(&data_len.to_le_bytes())?;
        f.write_all(pcm)?;
        Ok(())
    }

    // Nº de cubos del envolvente: coincide con el ancho lógico que dibuja el editor. No hace falta
    // leer todas las muestras (millones en clips largos): se sondea a saltos dentro de cada cubo,
    // con coste fijo (~WAVE_BUCKETS × PEAK_PROBES) sea cual sea la duración.
    const WAVE_BUCKETS: usize = 1600;
    const PEAK_PROBES: usize = 96;

    fn peaks_from_pcm(pcm: &[u8], channels: u16) -> Vec<f32> {
        let ch = channels.max(1) as usize;
        let frames = pcm.len() / (ch * 2);
        let mut out = vec![0f32; WAVE_BUCKETS];
        if frames == 0 {
            return out;
        }
        let size = (frames / WAVE_BUCKETS).max(1);
        for (b, slot) in out.iter_mut().enumerate() {
            let start = b * size;
            if start >= frames {
                break;
            }
            let end = (start + size).min(frames);
            let span = end - start;
            let stride = if span > PEAK_PROBES { span / PEAK_PROBES } else { 1 };
            let mut peak = 0f32;
            let mut f = start;
            while f < end {
                let base = (f * ch) * 2;
                for c in 0..ch {
                    let idx = base + c * 2;
                    let v = i16::from_le_bytes([pcm[idx], pcm[idx + 1]]) as f32;
                    let a = if v < 0.0 { -v } else { v };
                    if a > peak {
                        peak = a;
                    }
                }
                f += stride;
            }
            *slot = peak / 32768.0;
        }
        out
    }

    // Picos desde un WAV PCM16 ya escrito por nosotros (cabecera fija de 44 bytes). Lee el archivo
    // local en vez de redecodificar el MP4 vía Media Foundation cuando las pistas ya están en caché.
    fn peaks_from_wav(path: &str) -> Option<Vec<f32>> {
        let bytes = std::fs::read(path).ok()?;
        if bytes.len() < 44 {
            return None;
        }
        let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
        Some(peaks_from_pcm(&bytes[44..], channels))
    }

    // Tiempos de presentación (ms) de TODOS los fotogramas de vídeo, ordenados. La captura WGC es de
    // framerate variable (frames solo cuando la pantalla cambia), así que para avanzar exactamente un
    // fotograma hay que conocer sus timestamps reales en vez de asumir un paso fijo. Mismo coste que
    // keyframe_times (una pasada de demux, sin decodificar).
    fn frame_times_inner(path: &str) -> std::result::Result<Vec<f64>, String> {
        let mf = |e: windows::core::Error| format!("{e:?}");
        let reader = open_reader(path).map_err(mf)?;
        let idx = find_stream(&reader, MFMediaType_Video).map_err(mf)?;
        unsafe { reader.SetStreamSelection(ALL_STREAMS, false) }.map_err(mf)?;
        unsafe { reader.SetStreamSelection(idx, true) }.map_err(mf)?;

        let mut times = Vec::new();
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            unsafe { reader.ReadSample(idx, 0, None, Some(&mut flags), None, Some(&mut sample)) }
                .map_err(mf)?;
            if flags & ENDOFSTREAM != 0 { break; }
            let Some(sample) = sample else { continue };
            let t = unsafe { sample.GetSampleTime() }.unwrap_or(0);
            times.push(t as f64 / 10_000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Ok(times)
    }

    pub fn frame_times(path: String) -> std::result::Result<Vec<f64>, String> {
        with_mf(move || frame_times_inner(&path))
    }

    pub fn keyframe_times(path: String) -> std::result::Result<Vec<f64>, String> {
        with_mf(move || keyframe_times_inner(&path))
    }

    pub fn clip_fps(path: String) -> std::result::Result<u32, String> {
        with_mf(move || read_video_meta(&path).map(|m| m.fps).map_err(|e| format!("{e:?}")))
    }

    fn keyframe_times_inner(path: &str) -> std::result::Result<Vec<f64>, String> {
        let mf = |e: windows::core::Error| format!("{e:?}");
        let reader = open_reader(path).map_err(mf)?;
        let idx = find_stream(&reader, MFMediaType_Video).map_err(mf)?;
        unsafe { reader.SetStreamSelection(ALL_STREAMS, false) }.map_err(mf)?;
        unsafe { reader.SetStreamSelection(idx, true) }.map_err(mf)?;

        let mut times = Vec::new();
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            unsafe { reader.ReadSample(idx, 0, None, Some(&mut flags), None, Some(&mut sample)) }
                .map_err(mf)?;
            if flags & ENDOFSTREAM != 0 { break; }
            let Some(sample) = sample else { continue };
            let is_sync = unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0;
            if is_sync {
                let t = unsafe { sample.GetSampleTime() }.unwrap_or(0);
                times.push(t as f64 / 10_000.0);
            }
        }
        Ok(times)
    }

    pub fn export_clip<F: Fn(f32) + Send + 'static>(
        src: String,
        dst: String,
        edit: ClipEdit,
        progress: F,
    ) -> std::result::Result<(), String> {
        std::thread::spawn(move || {
            unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
            ensure_mf();
            let r = do_export(&src, &dst, &edit, &progress);
            unsafe { CoUninitialize(); }
            // El clip editado hereda el origen del original (juego/monitor) embebiéndolo igual que
            // en la captura, para que conserve su etiqueta en la biblioteca.
            if r.is_ok() {
                let source = crate::library::clip_source(std::path::Path::new(&src))
                    .unwrap_or_default();
                if !source.is_empty() {
                    let _ = crate::library::write_embedded_source(
                        std::path::Path::new(&dst),
                        &source,
                    );
                }
            }
            r
        })
        .join()
        .map_err(|_| "El hilo de exportación terminó inesperadamente".to_string())?
    }

    #[derive(Clone)]
    struct VideoMeta {
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    }

    fn read_video_meta(path: &str) -> Result<VideoMeta> {
        let reader = open_reader(path)?;
        let v_idx = find_stream(&reader, MFMediaType_Video)?;
        let mt = unsafe { reader.GetCurrentMediaType(v_idx)? };
        let size = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) }.unwrap_or(pack2(0, 0));
        let w = (size >> 32) as u32;
        let h = (size & 0xFFFFFFFF) as u32;
        let fps_packed = unsafe { mt.GetUINT64(&MF_MT_FRAME_RATE) }.unwrap_or(pack2(30, 1));
        let fps_n = (fps_packed >> 32) as u32;
        let fps_d = (fps_packed & 0xFFFFFFFF) as u32;
        let fps = if fps_d == 0 { 30 } else { fps_n / fps_d };
        // Bitrate objetivo para recodificar en el export. MF no siempre expone MF_MT_AVG_BITRATE en
        // el MP4, así que si falta (o es ínfimo) se usa un suelo por resolución·fps para no exportar
        // con mala calidad. Si el media type sí lo da, gana ese.
        let mt_bitrate = unsafe { mt.GetUINT32(&MF_MT_AVG_BITRATE) }.unwrap_or(0);
        let floor = ((w as u64 * h as u64 * fps.max(1) as u64) / 8).clamp(4_000_000, 80_000_000) as u32;

        Ok(VideoMeta {
            width: w,
            height: h,
            fps: fps.max(1),
            bitrate: mt_bitrate.max(floor),
        })
    }

    fn find_stream(reader: &IMFSourceReader, kind: GUID) -> Result<u32> {
        let mut i = 0u32;
        while let Ok(mt) = unsafe { reader.GetNativeMediaType(i, 0) } {
            let major = unsafe { mt.GetGUID(&MF_MT_MAJOR_TYPE) }
                .unwrap_or(GUID::zeroed());
            if major == kind {
                return Ok(i);
            }
            i += 1;
        }
        Err(windows::core::Error::from(windows::core::HRESULT(0x80070002u32 as i32)))
    }

    // Exporta el montaje recodificando con un ÚNICO encoder por hardware: así el clip de salida
    // tiene un solo SPS/PPS coherente con su avcC. La versión anterior copiaba la mayor parte y
    // recodificaba solo el GOP del borde con el encoder por software, mezclando dos SPS distintos
    // bajo un único avcC: el decodificador estricto del editor (WebView2) no podía decodificar el
    // primer frame y lo mostraba en negro. Recodificar todo con un encoder es además más rápido
    // que el camino anterior (HW + seek, sin pasadas desde el inicio del archivo). El recorte
    // sigue siendo exacto al frame: el primer fotograma conservado de cada tramo se vuelve IDR.
    fn do_export(
        src: &str,
        dst: &str,
        edit: &ClipEdit,
        progress: &dyn Fn(f32),
    ) -> std::result::Result<(), String> {
        let mf = |e: windows::core::Error| format!("{e:?}");

        if edit.segments.is_empty() {
            return Err("No hay segmentos para exportar".into());
        }
        let meta = read_video_meta(src).map_err(mf)?;
        let keyframes = keyframe_times_inner(src)?;
        let frame_dur = (10_000_000 / meta.fps.max(1) as i64).max(1);

        // Total de fotogramas a escribir, para el porcentaje del progreso.
        let total_frames: i64 = edit
            .segments
            .iter()
            .map(|s| (((s.end_ms - s.start_ms).max(0.0) / 1000.0) * meta.fps as f64).round() as i64)
            .sum::<i64>()
            .max(1);

        // Decodificador de vídeo a NV12. Se configura ANTES de declarar la entrada del encoder para
        // usar su tipo de salida REAL (stride/apertura incluidos): construir el NV12 a mano solo con
        // el tamaño dejaba una franja verde arriba por desajuste de geometría entre el buffer del
        // decodificador y lo que el encoder asumía.
        // Se decodifica a RGB32 (no NV12) y se deja que el SinkWriter haga RGB→NV12→H.264 con su
        // propio conversor (el mismo camino que la captura). Alimentar NV12 del decodificador
        // directamente dejaba una franja verde arriba: 1080 no es múltiplo de 16, el plano luma sale
        // alineado a 1088 y el encoder leía el plano de croma desfasado. RGB32 no tiene planos de
        // croma, así que el desajuste desaparece. Requiere "advanced video processing" en el lector.
        let v_reader = {
            let attrs = unsafe {
                let mut a: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut a, 1).map_err(mf)?;
                let a = a.unwrap();
                a.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1).map_err(mf)?;
                a
            };
            let url = HSTRING::from(src);
            unsafe { MFCreateSourceReaderFromURL(&url, &attrs).map_err(mf)? }
        };
        let v_idx = find_stream(&v_reader, MFMediaType_Video).map_err(mf)?;
        unsafe { v_reader.SetStreamSelection(ALL_STREAMS, false).map_err(mf)? };
        unsafe { v_reader.SetStreamSelection(v_idx, true).map_err(mf)? };
        let rgb = unsafe { MFCreateMediaType().map_err(mf)? };
        unsafe {
            rgb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(mf)?;
            rgb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).map_err(mf)?;
            rgb.SetUINT64(&MF_MT_FRAME_SIZE, pack2(meta.width, meta.height)).map_err(mf)?;
            v_reader.SetCurrentMediaType(v_idx, None, &rgb).map_err(mf)?;
        }
        let v_in = unsafe { v_reader.GetCurrentMediaType(v_idx).map_err(mf)? };
        unsafe { v_in.SetUINT64(&MF_MT_FRAME_RATE, pack2(meta.fps, 1)).map_err(mf)? };

        // SinkWriter con transformaciones por hardware habilitadas: usa el encoder H.264 por
        // hardware si está disponible (el mismo tipo que la captura), con fallback a software.
        let sink_attrs = unsafe {
            let mut a: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut a, 2).map_err(mf)?;
            let a = a.unwrap();
            a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1).map_err(mf)?;
            a.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1).map_err(mf)?;
            a
        };
        let dst_url = HSTRING::from(dst);
        let sink: IMFSinkWriter =
            unsafe { MFCreateSinkWriterFromURL(&dst_url, None, &sink_attrs).map_err(mf)? };

        // Vídeo: entrada NV12 (lo que entrega el decodificador) → salida H.264.
        let v_out = unsafe { MFCreateMediaType().map_err(mf)? };
        unsafe {
            v_out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(mf)?;
            v_out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).map_err(mf)?;
            v_out.SetUINT64(&MF_MT_FRAME_SIZE, pack2(meta.width, meta.height)).map_err(mf)?;
            v_out.SetUINT64(&MF_MT_FRAME_RATE, pack2(meta.fps, 1)).map_err(mf)?;
            v_out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(mf)?;
            v_out.SetUINT32(&MF_MT_AVG_BITRATE, meta.bitrate).map_err(mf)?;
        }
        let v_stream = unsafe { sink.AddStream(&v_out).map_err(mf)? };
        // Entrada del encoder = tipo de salida REAL del decodificador (ver arriba): evita la franja verde.
        unsafe { sink.SetInputMediaType(v_stream, &v_in, None).map_err(mf)? };

        // Audio: con micro (2 pistas) se rehornea la mezcla sistema+micro; si solo hay una pista
        // embebida, se decodifica a PCM y el SinkWriter la recodifica (con el volumen del fader).
        let src_reader = open_reader(src).map_err(mf)?;
        let has_mic = count_audio_streams(src).map(|n| n >= 2).unwrap_or(false);
        let remixed = if has_mic { Some(build_remixed_pcm(src, edit).map_err(mf)?) } else { None };
        let a_idx = if has_mic { None } else { find_stream(&src_reader, MFMediaType_Audio).ok() };

        let mut remix_stream = None;
        let mut pass_stream = None;
        if let Some((_, rate)) = &remixed {
            remix_stream = Some(add_remix_audio_stream(&sink, *rate).map_err(mf)?);
        } else if let Some(a_idx) = a_idx {
            let native_ch = unsafe {
                src_reader.GetNativeMediaType(a_idx, 0)
                    .ok()
                    .and_then(|mt| mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).ok())
                    .unwrap_or(2)
            };
            let pcm_type = unsafe { MFCreateMediaType().map_err(mf)? };
            unsafe {
                pcm_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(mf)?;
                pcm_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(mf)?;
                pcm_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, native_ch.min(2)).map_err(mf)?;
                src_reader.SetCurrentMediaType(a_idx, None, &pcm_type).map_err(mf)?;
            }
            let a_src = unsafe { src_reader.GetCurrentMediaType(a_idx).map_err(mf)? };
            let sr = unsafe { a_src.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.unwrap_or(48000);
            let ch = unsafe { a_src.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.unwrap_or(2);
            let a_stream = add_pcm_audio_stream(&sink, sr, ch).map_err(mf)?;
            pass_stream = Some((a_idx, a_stream));
        }

        unsafe { sink.BeginWriting().map_err(mf)? };

        // --- Vídeo: recodificar solo los tramos conservados (el decodificador ya está en NV12) ---
        let mut kept_before: i64 = 0;
        let mut written: i64 = 0;
        for seg in &edit.segments {
            let start_hns = (seg.start_ms * 10_000.0) as i64;
            let end_hns = (seg.end_ms * 10_000.0) as i64;
            if end_hns <= start_hns {
                continue;
            }
            // Posicionar el lector en el keyframe anterior al inicio del tramo: decodifica desde ahí
            // (necesario para las referencias) pero solo se escriben los frames a partir del corte,
            // así el recorte es exacto al frame sin exportar nada que el usuario no quería.
            let kf = keyframes.iter().rev().find(|k| **k <= seg.start_ms).copied().unwrap_or(0.0);
            let pos = PROPVARIANT::from((kf * 10_000.0) as i64);
            unsafe { v_reader.SetCurrentPosition(&GUID::zeroed(), &pos).map_err(mf)? };

            loop {
                let mut flags = 0u32;
                let mut sample: Option<IMFSample> = None;
                unsafe {
                    v_reader.ReadSample(v_idx, 0, None, Some(&mut flags), None, Some(&mut sample)).map_err(mf)?;
                }
                if flags & ENDOFSTREAM != 0 { break; }
                let Some(sample) = sample else { continue };
                let t = unsafe { sample.GetSampleTime().map_err(mf)? };
                if t < start_hns { continue; }
                if t >= end_hns { break; }
                // Re-tiempo al hueco eliminado, idéntico al del audio: los tramos se concatenan sin
                // huecos en la salida (mismo mapeo origen→salida que el audio para mantener el sync).
                let out_t = (t - start_hns) + kept_before;
                unsafe {
                    sample.SetSampleTime(out_t).map_err(mf)?;
                    sample.SetSampleDuration(frame_dur).map_err(mf)?;
                    sink.WriteSample(v_stream, &sample).map_err(mf)?;
                }
                written += 1;
                if written % 10 == 0 {
                    progress((written as f32 / total_frames as f32).min(0.97));
                }
            }
            kept_before += end_hns - start_hns;
        }

        // --- Audio ---
        if let Some(a_stream) = remix_stream {
            let (mixed, rate) = remixed.as_ref().unwrap();
            write_remixed_audio(&sink, a_stream, mixed, *rate, edit).map_err(mf)?;
        } else if let Some((a_idx, a_stream)) = pass_stream {
            unsafe { src_reader.SetStreamSelection(ALL_STREAMS, false).map_err(mf)? };
            unsafe { src_reader.SetStreamSelection(a_idx, true).map_err(mf)? };
            let sys_gain = if edit.mixer.sys_muted { 0.0f32 } else { edit.mixer.sys_vol };
            let seg_ranges: Vec<(i64, i64, i64)> = {
                let mut kept = 0i64;
                edit.segments.iter().map(|seg| {
                    let s = (seg.start_ms * 10_000.0) as i64;
                    let e = (seg.end_ms * 10_000.0) as i64;
                    let offset = s - kept;
                    kept += e - s;
                    (s, e, offset)
                }).collect()
            };
            let mut seg_idx = 0usize;
            loop {
                let mut flags = 0u32;
                let mut sample: Option<IMFSample> = None;
                unsafe { src_reader.ReadSample(a_idx, 0, None, Some(&mut flags), None, Some(&mut sample)).map_err(mf)? };
                if flags & ENDOFSTREAM != 0 { break; }
                let Some(sample) = sample else { continue };
                let t = unsafe { sample.GetSampleTime().map_err(mf)? };
                while seg_idx < seg_ranges.len() && t >= seg_ranges[seg_idx].1 {
                    seg_idx += 1;
                }
                if seg_idx >= seg_ranges.len() { break; }
                let (start_hns, _end_hns, offset) = seg_ranges[seg_idx];
                if t >= start_hns {
                    unsafe { sample.SetSampleTime(t - offset).map_err(mf)? };
                    if (sys_gain - 1.0).abs() > 1e-3 {
                        apply_gain_pcm16(&sample, sys_gain).map_err(mf)?;
                    }
                    unsafe { sink.WriteSample(a_stream, &sample).map_err(mf)? };
                }
            }
        }

        progress(0.99);
        unsafe { sink.Finalize().map_err(mf)? };
        progress(1.0);
        Ok(())
    }

    fn pack2(hi: u32, lo: u32) -> u64 {
        (hi as u64) << 32 | lo as u64
    }

    // Genera la pista de mezcla final aplicando los volúmenes/silencios del editor sobre las
    // pistas de sistema (1) y micro (2). Devuelve PCM16 estéreo entrelazado al rate elegido.
    // El AAC de Media Foundation solo admite 44100/48000 Hz; si el origen es otro, se remuestrea.
    fn build_remixed_pcm(src: &str, edit: &ClipEdit) -> Result<(Vec<i16>, u32)> {
        let (sys_raw, sr, sc) = read_pcm(src, 1)?;
        let (mic_raw, mr, mc) = read_pcm(src, 0)?;

        let out_rate = if sr == 44100 || sr == 48000 {
            sr
        } else if mr == 44100 || mr == 48000 {
            mr
        } else {
            48000
        };

        let sys = to_stereo_f32(&sys_raw, sr, sc, out_rate);
        let mic = to_stereo_f32(&mic_raw, mr, mc, out_rate);

        let sys_gain = if edit.mixer.sys_muted { 0.0 } else { edit.mixer.sys_vol.max(0.0) };
        let mic_gain = if edit.mixer.mic_muted { 0.0 } else { edit.mixer.mic_vol.max(0.0) };

        let n = sys.len().max(mic.len());
        let mut mixed = vec![0i16; n];
        for i in 0..n {
            let s = sys.get(i).copied().unwrap_or(0.0) * sys_gain;
            let m = mic.get(i).copied().unwrap_or(0.0) * mic_gain;
            mixed[i] = soft_clip_sample(s + m);
        }
        Ok((mixed, out_rate))
    }

    // PCM16 entrelazado de `src_ch` canales a estéreo f32 al `out_rate`. Downmix multicanal
    // (frontales íntegros, central/surround a 0.707) y remuestreo lineal cuando los rates
    // difieren. Mismo criterio que el mezclador en vivo (audio.rs), pero offline.
    fn to_stereo_f32(pcm: &[u8], src_rate: u32, src_ch: u16, out_rate: u32) -> Vec<f32> {
        let src_ch = src_ch.max(1) as usize;
        let in_frames = pcm.len() / (src_ch * 2);
        if in_frames == 0 {
            return Vec::new();
        }
        let rd = |frame: usize, ch: usize| -> f32 {
            let idx = (frame * src_ch + ch) * 2;
            i16::from_le_bytes([pcm[idx], pcm[idx + 1]]) as f32
        };
        let lr = |frame: usize| -> (f32, f32) {
            if src_ch == 1 {
                let m = rd(frame, 0);
                (m, m)
            } else {
                let mut l = rd(frame, 0);
                let mut r = rd(frame, 1);
                if src_ch >= 3 {
                    let c = 0.707 * rd(frame, 2);
                    l += c;
                    r += c;
                }
                let mut i = 4;
                while i < src_ch {
                    let s = 0.707 * rd(frame, i);
                    if (i - 4) % 2 == 0 {
                        l += s;
                    } else {
                        r += s;
                    }
                    i += 1;
                }
                (l, r)
            }
        };

        let src_rate_i = src_rate.max(1) as i64;
        let out_rate_i = out_rate.max(1) as i64;
        let same = src_rate_i == out_rate_i;
        let out_frames = if same {
            in_frames
        } else {
            ((in_frames as i64 * out_rate_i) / src_rate_i).max(0) as usize
        };

        let mut out = Vec::with_capacity(out_frames * 2);
        for k in 0..out_frames {
            let (l, r) = if same {
                lr(k.min(in_frames - 1))
            } else {
                let pos = k as f64 * src_rate_i as f64 / out_rate_i as f64;
                let i0 = (pos.floor() as usize).min(in_frames - 1);
                let i1 = (i0 + 1).min(in_frames - 1);
                let frac = (pos - pos.floor()) as f32;
                let (l0, r0) = lr(i0);
                let (l1, r1) = lr(i1);
                (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
            };
            out.push(l);
            out.push(r);
        }
        out
    }

    // Soft clip: lineal por debajo del umbral, compresión suave por encima. Evita la
    // distorsión áspera del recorte duro al sumar dos fuentes a tope.
    fn soft_clip_sample(x: f32) -> i16 {
        const T: f32 = 0.75;
        let n = x / 32768.0;
        let a = n.abs();
        let y = if a <= T {
            n
        } else {
            n.signum() * (T + (1.0 - T) * (1.0 - (-(a - T) / (1.0 - T)).exp()))
        };
        (y * 32767.0).clamp(-32768.0, 32767.0) as i16
    }

    // Escala en sitio un IMFSample de PCM16 por una ganancia (atenuación del fader de pista única).
    // Solo se invoca cuando la ganancia != 1.0; los samples del SourceReader traen un buffer único.
    fn apply_gain_pcm16(sample: &IMFSample, gain: f32) -> Result<()> {
        let buf = unsafe { sample.ConvertToContiguousBuffer()? };
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut cur = 0u32;
        unsafe { buf.Lock(&mut ptr, None, Some(&mut cur))? };
        let n = cur as usize / 2;
        if n > 0 {
            let s = unsafe { std::slice::from_raw_parts_mut(ptr as *mut i16, n) };
            for x in s.iter_mut() {
                *x = ((*x as f32) * gain).clamp(-32768.0, 32767.0) as i16;
            }
        }
        unsafe { buf.Unlock()? };
        Ok(())
    }

    fn add_remix_audio_stream(sink: &IMFSinkWriter, rate: u32) -> Result<u32> {
        let ch = 2u32;
        let out_type = unsafe { MFCreateMediaType()? };
        unsafe {
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            out_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
            out_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch)?;
            out_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            out_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 128_000 / 8)?;
            out_type.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
        }
        let stream = unsafe { sink.AddStream(&out_type)? };

        let in_type = unsafe { MFCreateMediaType()? };
        unsafe {
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            in_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            in_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
            in_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch)?;
            in_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            in_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, ch * 2)?;
            in_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * ch * 2)?;
            sink.SetInputMediaType(stream, &in_type, None)?;
        }
        Ok(stream)
    }

    fn add_pcm_audio_stream(sink: &IMFSinkWriter, rate: u32, ch: u32) -> Result<u32> {
        let out_type = unsafe { MFCreateMediaType()? };
        unsafe {
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            out_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
            out_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch)?;
            out_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            out_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 128_000 / 8)?;
            out_type.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
        }
        let stream = unsafe { sink.AddStream(&out_type)? };
        let in_type = unsafe { MFCreateMediaType()? };
        let block_align = ch * 2;
        unsafe {
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            in_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            in_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
            in_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch)?;
            in_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            in_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_align)?;
            in_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * block_align)?;
            sink.SetInputMediaType(stream, &in_type, None)?;
        }
        Ok(stream)
    }

    // Escribe la mezcla recortada por segmentos: solo las porciones conservadas, concatenadas,
    // con timestamps secuenciales (el SinkWriter recodifica PCM→AAC). `mixed` es estéreo
    // entrelazado al `rate` dado.
    fn write_remixed_audio(
        sink: &IMFSinkWriter,
        a_stream: u32,
        mixed: &[i16],
        rate: u32,
        edit: &ClipEdit,
    ) -> Result<()> {
        let frames_total = mixed.len() / 2;
        let rate_i = rate.max(1) as i64;
        let block_frames = (rate / 50).max(1) as usize;
        let mut out_t: i64 = 0;

        for seg in &edit.segments {
            let start_f = (((seg.start_ms / 1000.0) * rate as f64).round() as i64).max(0) as usize;
            let end_f = (((seg.end_ms / 1000.0) * rate as f64).round() as i64).max(0) as usize;
            let start_f = start_f.min(frames_total);
            let end_f = end_f.min(frames_total);

            let mut f = start_f;
            while f < end_f {
                let chunk = (end_f - f).min(block_frames);
                let byte_len = chunk * 4;

                let sample = unsafe { MFCreateSample()? };
                let buf = unsafe { MFCreateMemoryBuffer(byte_len as u32)? };
                let mut ptr: *mut u8 = std::ptr::null_mut();
                unsafe { buf.Lock(&mut ptr, None, None)? };
                unsafe {
                    let dst = std::slice::from_raw_parts_mut(ptr as *mut i16, chunk * 2);
                    dst.copy_from_slice(&mixed[f * 2..f * 2 + chunk * 2]);
                }
                unsafe { buf.Unlock()? };
                unsafe { buf.SetCurrentLength(byte_len as u32)? };
                unsafe { sample.AddBuffer(&buf)? };
                unsafe { sample.SetSampleTime(out_t)? };
                let dur = chunk as i64 * 10_000_000 / rate_i;
                unsafe { sample.SetSampleDuration(dur)? };
                unsafe { sink.WriteSample(a_stream, &sample)? };

                out_t += dur;
                f += chunk;
            }
        }
        Ok(())
    }
}
