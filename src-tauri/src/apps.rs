//! Apps abertos e ícones: a lista do seletor de app e o ícone de cada app (PNG em data URL).
//!
//! As funções puras (nome, dedupe, PNG, base64) ficam fora do `cfg(windows)` para ter teste
//! direto. As chamadas Win32 ficam nas funções `#[cfg(windows)]`. Nada aqui pode derrubar o
//! app: quem chama (commands.rs) segura pânico.

use serde::Serialize;

use crate::audio::sessions::{normalize_exe, UNKNOWN_EXE};

/// Lado máximo aceito para um ícone (os ícones de app têm 32 ou 48 px; o resto é suspeito).
const MAX_ICON_SIDE: i32 = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunningApp {
    /// Nome do executável em minúsculas (é a chave das regras).
    pub exe: String,
    /// Descrição do arquivo do Windows, ou o nome do executável sem `.exe`.
    pub name: String,
}

/// Nome amigável: a descrição do arquivo quando existe; senão o nome sem `.exe`.
pub fn friendly_name(description: Option<&str>, exe: &str) -> String {
    if let Some(text) = description.map(str::trim).filter(|text| !text.is_empty()) {
        return text.to_string();
    }
    let base = exe.trim();
    // `.exe` é ASCII, então o corte nunca cai no meio de um caractere.
    let stem = if base.to_ascii_lowercase().ends_with(".exe") {
        &base[..base.len() - 4]
    } else {
        base
    };
    stem.to_string()
}

/// Limpa e ordena: tira vazios e o `desconhecido.exe` (processo sem caminho legível), um app
/// por executável (fica o primeiro) e ordem por nome, sem diferenciar maiúsculas.
pub fn collect_apps(apps: Vec<RunningApp>) -> Vec<RunningApp> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<RunningApp> = apps
        .into_iter()
        .filter(|app| !app.exe.is_empty() && app.exe != UNKNOWN_EXE)
        .filter(|app| seen.insert(app.exe.clone()))
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.exe.cmp(&b.exe))
    });
    out
}

/// Pids dos processos cujo nome de executável é `exe` (comparação pelo nome normalizado).
pub fn matching_pids(exe: &str, processes: &[(u32, String)]) -> Vec<u32> {
    let wanted = normalize_exe(exe);
    processes
        .iter()
        .filter(|(_, name)| normalize_exe(name) == wanted)
        .map(|(pid, _)| *pid)
        .collect()
}

/// BGRA (ordem do GDI) para RGBA. Ícone sem canal alfa (todos os bytes de alfa zerados)
/// vira opaco; senão ele sairia totalmente invisível.
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let legacy = bgra.chunks_exact(4).all(|px| px[3] == 0);
    let mut out = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        let alpha = if legacy { 255 } else { px[3] };
        out.extend_from_slice(&[px[2], px[1], px[0], alpha]);
    }
    out
}

/// PNG de 8 bits RGBA. Erro se o buffer não tem `width * height * 4` bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "imagem grande demais".to_string())?;
    if rgba.len() != expected {
        return Err(format!(
            "buffer de {} bytes para imagem {width}x{height} (esperado {expected})",
            rgba.len()
        ));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
        writer
            .write_image_data(rgba)
            .map_err(|err| err.to_string())?;
    }
    Ok(out)
}

pub fn png_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64_encode(png))
}

/// Base64 padrão (com `=`), sem dependência extra.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Apps com janela visível (um por executável), para o seletor. Vazio fora do Windows.
pub fn list_running_apps() -> Vec<RunningApp> {
    #[cfg(windows)]
    {
        windows_impl::list_running_apps()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Ícone do app como data URL PNG. `exe` é o nome do executável (ou um caminho completo).
/// `None` quando o app não está aberto ou o ícone não pode ser lido.
pub fn app_icon(exe: &str) -> Option<String> {
    #[cfg(windows)]
    {
        windows_impl::app_icon(exe).map(|png| png_data_url(&png))
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        None
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::iter::once;
    use std::mem::size_of;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::Shell::ExtractIconExW;
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, EnumWindows, GetIconInfo, GetWindow, GetWindowLongPtrW,
        GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER,
        HICON, ICONINFO, WS_EX_TOOLWINDOW,
    };

    use super::{
        collect_apps, encode_png, friendly_name, matching_pids, bgra_to_rgba, RunningApp,
        MAX_ICON_SIDE,
    };
    use crate::audio::sessions::{normalize_exe, process_image_path};

    pub fn list_running_apps() -> Vec<RunningApp> {
        let mut pids: Vec<u32> = Vec::new();
        // SAFETY: EnumWindows chama `collect_window` de forma síncrona, e `pids` vive até o retorno.
        let _ = unsafe {
            EnumWindows(Some(collect_window), LPARAM(&mut pids as *mut Vec<u32> as isize))
        };
        pids.sort_unstable();
        pids.dedup();
        let own = std::process::id();
        let apps = pids
            .into_iter()
            .filter(|pid| *pid != own)
            .filter_map(|pid| {
                let path = process_image_path(pid)?;
                let exe = normalize_exe(&path);
                let name = friendly_name(file_description(&path).as_deref(), &exe);
                Some(RunningApp { exe, name })
            })
            .collect();
        collect_apps(apps)
    }

    /// Chamado pelo Windows para cada janela de topo. Nunca propaga pânico (sairia do app).
    unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: `lparam` é o `&mut Vec<u32>` de `list_running_apps`, vivo durante o EnumWindows.
            let pids = unsafe { &mut *(lparam.0 as *mut Vec<u32>) };
            if is_app_window(hwnd) {
                let mut pid = 0u32;
                // SAFETY: `hwnd` veio do Windows nesta chamada; `pid` é um local válido.
                unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
                if pid != 0 {
                    pids.push(pid);
                }
            }
        }));
        BOOL(1)
    }

    /// Janela de app: visível, com título, sem dono (diálogos têm dono), sem a marca de
    /// janela de ferramenta e não "cloaked" (frames fantasmas de apps da Store).
    fn is_app_window(hwnd: HWND) -> bool {
        // SAFETY: chamadas de consulta sobre um HWND que o Windows acabou de entregar.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) <= 0 {
                return false;
            }
            let has_owner = GetWindow(hwnd, GW_OWNER)
                .map(|owner| !owner.0.is_null())
                .unwrap_or(false);
            if has_owner {
                return false;
            }
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
                return false;
            }
            let mut cloaked: u32 = 0;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut c_void,
                size_of::<u32>() as u32,
            );
            cloaked == 0
        }
    }

    /// Todos os processos como (pid, nome do executável).
    fn running_processes() -> Vec<(u32, String)> {
        let mut out = Vec::new();
        // SAFETY: snapshot só de leitura, fechado no fim.
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY: `entry` tem o tamanho que o Windows espera em `dwSize`.
        if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
                out.push((entry.th32ProcessID, String::from_utf16_lossy(&entry.szExeFile[..len])));
                // SAFETY: mesmo `entry` de antes; o laço para no primeiro erro (fim da lista).
                if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                    break;
                }
            }
        }
        // SAFETY: handle obtido de CreateToolhelp32Snapshot acima.
        let _ = unsafe { CloseHandle(snapshot) };
        out
    }

    /// Caminho do executável para um nome (ex.: `spotify.exe`) ou o próprio caminho, se vier um.
    fn resolve_path(exe: &str) -> Option<String> {
        let wanted = exe.trim();
        if wanted.is_empty() {
            return None;
        }
        if wanted.contains(['\\', '/']) {
            return Path::new(wanted).is_file().then(|| wanted.to_string());
        }
        let processes = running_processes();
        matching_pids(wanted, &processes)
            .into_iter()
            .find_map(process_image_path)
    }

    pub fn app_icon(exe: &str) -> Option<Vec<u8>> {
        let path = resolve_path(exe)?;
        let wide = to_wide(&path);
        let mut large = HICON::default();
        // SAFETY: `wide` termina em nulo; `large` recebe o handle do ícone grande.
        let count = unsafe {
            ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large as *mut HICON), None, 1)
        };
        if count == 0 || large.0.is_null() {
            return None;
        }
        let mut guard = IconGuard {
            icon: large,
            color: HBITMAP::default(),
            mask: HBITMAP::default(),
            dc: HDC::default(),
        };
        let mut info = ICONINFO::default();
        // SAFETY: `large` é um HICON válido; `info` recebe dois bitmaps que pertencem a quem chama.
        unsafe { GetIconInfo(large, &mut info) }.ok()?;
        guard.color = info.hbmColor;
        guard.mask = info.hbmMask;
        if guard.color.0.is_null() {
            // Ícone monocromático (sem bitmap colorido): não tratado.
            return None;
        }

        let mut bitmap = BITMAP::default();
        // SAFETY: `bitmap` tem o tamanho de BITMAP que o GetObjectW recebe.
        let read = unsafe {
            GetObjectW(
                HGDIOBJ(guard.color.0),
                size_of::<BITMAP>() as i32,
                Some(&mut bitmap as *mut BITMAP as *mut c_void),
            )
        };
        let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
        if read == 0 || width <= 0 || height <= 0 || width > MAX_ICON_SIDE || height > MAX_ICON_SIDE {
            return None;
        }

        // SAFETY: DC compatível com a tela; liberado pelo guard.
        guard.dc = unsafe { CreateCompatibleDC(None) };
        if guard.dc.0.is_null() {
            return None;
        }
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Altura negativa: linhas de cima para baixo, como o PNG.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; width as usize * height as usize * 4];
        // SAFETY: `bgra` tem width*height*4 bytes, o tamanho que o GetDIBits escreve (32 bpp).
        let lines = unsafe {
            GetDIBits(
                guard.dc,
                guard.color,
                0,
                height as u32,
                Some(bgra.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        if lines != height {
            return None;
        }
        encode_png(width as u32, height as u32, &bgra_to_rgba(&bgra)).ok()
    }

    /// Versão do arquivo: a "descrição" que o Windows mostra (ex.: "Spotify").
    fn file_description(path: &str) -> Option<String> {
        let wide = to_wide(path);
        // SAFETY: caminho terminado em nulo; o tamanho devolvido dimensiona o buffer.
        let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None) };
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        // SAFETY: `block` tem `size` bytes, como a chamada pede.
        unsafe {
            GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, block.as_mut_ptr() as *mut c_void)
        }
        .ok()?;

        // Idioma e página de código do primeiro bloco de tradução (pares de u16, em bytes).
        let (translation, len) = query(&block, "\\VarFileInfo\\Translation")?;
        if len < 4 {
            return None;
        }
        // SAFETY: o ponteiro aponta para dentro de `block`, que ainda está vivo; `len` bytes.
        let bytes = unsafe { std::slice::from_raw_parts(translation as *const u8, 4) };
        let lang = u16::from_le_bytes([bytes[0], bytes[1]]);
        let codepage = u16::from_le_bytes([bytes[2], bytes[3]]);
        let key = format!("\\StringFileInfo\\{lang:04x}{codepage:04x}\\FileDescription");
        let (text, len) = query(&block, &key)?;
        // SAFETY: `len` é em caracteres UTF-16 (inclui o nulo final), dentro de `block`.
        let units = unsafe { std::slice::from_raw_parts(text as *const u16, len as usize) };
        let trimmed: Vec<u16> = units.iter().copied().take_while(|c| *c != 0).collect();
        Some(String::from_utf16_lossy(&trimmed))
    }

    /// Ponteiro e tamanho de um valor da versão (dentro de `block`).
    fn query(block: &[u8], key: &str) -> Option<(*const c_void, u32)> {
        let wide = to_wide(key);
        let mut value: *mut c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        // SAFETY: `block` vive durante a chamada; `value`/`len` são locais válidos.
        let found = unsafe {
            VerQueryValueW(
                block.as_ptr() as *const c_void,
                PCWSTR(wide.as_ptr()),
                &mut value,
                &mut len,
            )
        };
        (found.as_bool() && !value.is_null()).then_some((value as *const c_void, len))
    }

    fn to_wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(once(0)).collect()
    }

    /// Libera os handles do ícone ao sair de escopo, mesmo em retorno antecipado.
    struct IconGuard {
        icon: HICON,
        color: HBITMAP,
        mask: HBITMAP,
        dc: HDC,
    }

    impl Drop for IconGuard {
        fn drop(&mut self) {
            // SAFETY: cada handle foi obtido das chamadas acima e é liberado uma única vez aqui.
            unsafe {
                if !self.dc.0.is_null() {
                    let _ = DeleteDC(self.dc);
                }
                if !self.color.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(self.color.0));
                }
                if !self.mask.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(self.mask.0));
                }
                if !self.icon.0.is_null() {
                    let _ = DestroyIcon(self.icon);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, name: &str) -> RunningApp {
        RunningApp {
            exe: exe.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn friendly_name_prefers_description_then_stem() {
        assert_eq!(friendly_name(Some("  Spotify  "), "spotify.exe"), "Spotify");
        assert_eq!(friendly_name(Some("   "), "spotify.exe"), "spotify");
        assert_eq!(friendly_name(None, "game.exe"), "game");
        assert_eq!(friendly_name(None, "sem-extensao"), "sem-extensao");
        assert_eq!(friendly_name(None, "ÁUDIO.EXE"), "ÁUDIO");
    }

    #[test]
    fn collect_apps_drops_unknown_dedupes_and_sorts_by_name() {
        let apps = vec![
            app("zeta.exe", "zeta"),
            app(UNKNOWN_EXE, "desconhecido"),
            app("", "vazio"),
            app("Beta.exe", "Beta"),
            app("alfa.exe", "alfa"),
            app("alfa.exe", "alfa de novo"),
        ];

        let out = collect_apps(apps);

        assert_eq!(
            out,
            vec![app("alfa.exe", "alfa"), app("Beta.exe", "Beta"), app("zeta.exe", "zeta")]
        );
    }

    #[test]
    fn matching_pids_compares_basename_case_insensitively() {
        let processes = vec![
            (10, "Spotify.exe".to_string()),
            (11, "discord.exe".to_string()),
            (12, "spotify.exe".to_string()),
        ];
        assert_eq!(matching_pids("SPOTIFY.exe", &processes), vec![10, 12]);
        assert_eq!(matching_pids("C:\\Apps\\discord.exe", &processes), vec![11]);
        assert!(matching_pids("nada.exe", &processes).is_empty());
    }

    #[test]
    fn bgra_to_rgba_swaps_red_and_blue_and_keeps_alpha() {
        // Um pixel BGRA = (azul 1, verde 2, vermelho 3, alfa 128).
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 128]), vec![3, 2, 1, 128]);
    }

    #[test]
    fn bgra_to_rgba_forces_opaque_when_icon_has_no_alpha() {
        // Ícone sem canal alfa vem com todos os alfas zerados: sai opaco, não invisível.
        let rgba = bgra_to_rgba(&[1, 2, 3, 0, 4, 5, 6, 0]);
        assert_eq!(rgba, vec![3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn encode_png_rejects_buffer_of_wrong_length() {
        assert!(encode_png(2, 2, &[0; 15]).is_err());
        assert!(encode_png(2, 2, &[0; 17]).is_err());
    }

    #[test]
    fn encode_png_round_trips_a_two_by_two_image() {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, //
            0, 0, 255, 255, 10, 20, 30, 40,
        ];
        let bytes = encode_png(2, 2, &rgba).expect("png");

        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().expect("cabeçalho");
        let mut buf = vec![0; reader.output_buffer_size().expect("tamanho")];
        let info = reader.next_frame(&mut buf).expect("quadro");

        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&buf[..info.buffer_size()], rgba.as_slice());
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"M"), "TQ==");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Hello, world"), "SGVsbG8sIHdvcmxk");
    }

    #[test]
    fn png_data_url_has_png_prefix() {
        assert_eq!(png_data_url(b"Man"), "data:image/png;base64,TWFu");
    }
}
