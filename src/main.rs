// SPIKE DESCARTAVEL. Uso:
//   audio-spike list
//   audio-spike get <pid>
//   audio-spike route <pid> <indice-do-dispositivo>
//   audio-spike clear <pid>      (volta o app pro padrao do Windows)
use std::ffi::c_void;
use windows::core::*;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::WinRT::*;

const IID_21H2: GUID = GUID::from_u128(0xab3d4648_e242_459f_b02f_541c70306324);
const IID_DOWNLEVEL: GUID = GUID::from_u128(0x2a59116d_6c4f_45e0_a74f_707e3fef9258);
const RENDER_SUFFIX: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";
const CAPTURE_SUFFIX: &str = "#{2eef81be-33fa-4800-9670-1cd474972c3f}";
fn is_mic() -> bool { std::env::var("MIC").is_ok() }
fn flow() -> EDataFlow { if is_mic() { eCapture } else { eRender } }
fn suffix() -> &'static str { if is_mic() { CAPTURE_SUFFIX } else { RENDER_SUFFIX } }
const MMDEVAPI: &str = r"\\?\SWD#MMDEVAPI#";

// vtable: 6 slots IUnknown+IInspectable, depois N metodos "incompletos", depois os 3 uteis.
type SetFn = unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut c_void) -> HRESULT;
type GetFn = unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut HSTRING) -> HRESULT;

struct Policy {
    ptr: *mut c_void,
    set_slot: usize,
    get_slot: usize,
    variant: &'static str,
}

fn windows_build() -> u32 {
    // build lido do registro via RtlGetVersion seria ideal; usa a env do cmd como fallback simples
    let out = std::process::Command::new("cmd").args(["/c", "ver"]).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    s.split('.').nth(2).and_then(|x| x.trim().parse().ok()).unwrap_or(0)
}

unsafe fn policy() -> Result<Policy> {
    let class = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
    let build = windows_build();
    // EarTrumpet: build >= 21390 usa a variante 21H2 (19 metodos antes), senao Downlevel (17).
    let (iid, fillers, name) = if build >= 21390 { (IID_21H2, 19usize, "21H2") } else { (IID_DOWNLEVEL, 17usize, "Downlevel") };
    let mut p: *mut c_void = std::ptr::null_mut();
    let f: IInspectable = RoGetActivationFactory(&class)?;
    f.query(&iid, &mut p).ok()?;
    std::mem::forget(f); // spike: nao liberar
    println!("[info] build={build} variante={name}");
    Ok(Policy { ptr: p, set_slot: 6 + fillers, get_slot: 6 + fillers + 1, variant: name })
}

unsafe fn vfn<T: Copy>(obj: *mut c_void, slot: usize) -> T {
    let vtbl = *(obj as *mut *mut usize);
    std::mem::transmute_copy(&*vtbl.add(slot))
}

fn mk(s: &str) -> HSTRING {
    // HSTRING real do combase (como o EarTrumpet), nao o formato proprio do crate
    let w: Vec<u16> = s.encode_utf16().collect();
    unsafe { WindowsCreateString(Some(&w)).unwrap() }
}
fn wrap(id: &str, variant: u32) -> HSTRING {
    match variant {
        1 => mk(id),
        2 => mk(&format!("{MMDEVAPI}{id}")),
        3 => mk(&format!("{id}{RENDER_SUFFIX}")),
        _ => mk(&format!("{MMDEVAPI}{id}{}", suffix())),
    }
}
fn unwrap_id(s: &str) -> String {
    s.trim_start_matches(MMDEVAPI).trim_end_matches(RENDER_SUFFIX).trim_end_matches(CAPTURE_SUFFIX).to_string()
}

unsafe fn devices(en: &IMMDeviceEnumerator) -> Result<Vec<(String, String)>> {
    let col = en.EnumAudioEndpoints(flow(), DEVICE_STATE_ACTIVE)?;
    let mut v = vec![];
    for i in 0..col.GetCount()? {
        let d = col.Item(i)?;
        let id = d.GetId()?.to_string()?;
        let store = d.OpenPropertyStore(STGM_READ)?;
        let name = store.GetValue(&PKEY_Device_FriendlyName)?.to_string();
        v.push((id, name));
    }
    Ok(v)
}

unsafe fn proc_name(pid: u32) -> String {
    if pid == 0 { return "(sistema)".into(); }
    if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok { return String::from_utf16_lossy(&buf[..len as usize]).rsplit('\\').next().unwrap_or("").to_string(); }
    }
    "?".into()
}

unsafe fn sessions(en: &IMMDeviceEnumerator) -> Result<()> {
    // Sessoes de todos os dispositivos de saida ativos
    let col = en.EnumAudioEndpoints(flow(), DEVICE_STATE_ACTIVE)?;
    for i in 0..col.GetCount()? {
        let d = col.Item(i)?;
        let mgr: IAudioSessionManager2 = d.Activate(CLSCTX_ALL, None)?;
        let se = mgr.GetSessionEnumerator()?;
        for j in 0..se.GetCount()? {
            let c2: IAudioSessionControl2 = se.GetSession(j)?.cast()?;
            let pid = c2.GetProcessId()?;
            if pid == 0 { continue; }
            println!("  [sessao] pid={pid} {} (no dispositivo #{i})", proc_name(pid));
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let args: Vec<String> = std::env::args().collect();
        match args.get(1).map(|s| s.as_str()) {
            Some("list") | None => {
                println!("Dispositivos de saida:");
                for (i, (id, name)) in devices(&en)?.iter().enumerate() { println!("  #{i} {name}  [{id}]"); }
                println!("Apps com audio:");
                sessions(&en)?;
            }
            Some("get") => {
                let pid: u32 = args[2].parse().unwrap();
                let p = policy()?;
                let get: GetFn = vfn(p.ptr, p.get_slot);
                let mut out = HSTRING::new();
                for role in [eMultimedia.0, eConsole.0] {
                    let hr = get(p.ptr, pid, flow().0, role, &mut out);
                    println!("role={role} hr={:#x} destino=\"{}\"", hr.0, unwrap_id(&out.to_string()));
                }
            }
            Some("roundtrip") => {
                let pid: u32 = args[2].parse().unwrap();
                let p = policy()?;
                let get: GetFn = vfn(p.ptr, p.get_slot);
                let set: SetFn = vfn(p.ptr, p.set_slot);
                let mut out = HSTRING::new();
                let hr = get(p.ptr, pid, flow().0, eMultimedia.0, &mut out);
                let raw_s = out.to_string();
                println!("get hr={:#x} len={} bruto=\"{}\"", hr.0, raw_s.len(), raw_s);
                println!("utf16: {:?}", raw_s.encode_utf16().take(12).collect::<Vec<_>>());
                // devolve exatamente o mesmo HSTRING
                let raw: *mut c_void = std::mem::transmute_copy(&out);
                let hr2 = set(p.ptr, pid, flow().0, eMultimedia.0, raw);
                println!("set(mesmo hstring) hr={:#x}", hr2.0);
                // e uma copia nova com o mesmo texto
                let h2 = mk(&raw_s);
                let raw2: *mut c_void = std::mem::transmute_copy(&h2);
                let hr3 = set(p.ptr, pid, flow().0, eMultimedia.0, raw2);
                println!("set(copia do texto) hr={:#x}", hr3.0);
            }
            Some(cmd @ ("route" | "clear")) => {
                let pid: u32 = args[2].parse().unwrap();
                let p = policy()?;
                let set: SetFn = vfn(p.ptr, p.set_slot);
                let h: HSTRING = if cmd == "route" {
                    let idx: usize = args[3].parse().unwrap();
                    let devs = devices(&en)?;
                    println!("roteando pid {pid} -> {}", devs[idx].1);
                    let dbg = wrap(&devs[idx].0, args.get(4).and_then(|x| x.parse().ok()).unwrap_or(0)).to_string();
                    println!("meu texto  len={} \"{}\"", dbg.len(), dbg);
                    println!("id puro    len={} {:?}", devs[idx].0.len(), devs[idx].0);
                    wrap(&devs[idx].0, args.get(4).and_then(|x| x.parse().ok()).unwrap_or(0))
                } else { HSTRING::new() };
                let raw: *mut c_void = std::mem::transmute_copy(&h);
                for role in [eMultimedia.0, eConsole.0] {
                    let hr = set(p.ptr, pid, flow().0, role, raw);
                    println!("Set role={role} hr={:#x} ({})", hr.0, p.variant);
                }
            }
            _ => println!("uso: list | get <pid> | route <pid> <idx> | clear <pid>"),
        }
    }
    Ok(())
}
