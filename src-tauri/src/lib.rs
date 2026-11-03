mod shell;
#[allow(dead_code)] // Tasks seguintes consomem o roteador; até lá só os testes o usam.
mod audio;
#[allow(dead_code)] // Task 9 consome as regras; até lá só os testes o usam.
mod rules;
#[allow(dead_code)] // Task 9 consome o Store; até lá só os testes o usam.
mod store;
#[allow(dead_code)] // Task 9 consome o perfil; até lá só os testes o usam.
mod profiles;
mod state;
mod engine;
mod commands;

use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use directories::ProjectDirs;
use tauri::{AppHandle, Emitter, Manager};

use audio::default_device::WindowsDefaultDevice;
use audio::devices::{watch, WindowsAudioSource};
use audio::endpoint::WindowsEndpoint;
use audio::policy::WindowsRouteBackend;
use audio::types::AudioEvent;
use commands::MicLevel;
use engine::Engine;
use state::AppState;
use store::Store;

const PANEL: &str = "panel";
/// Releitura de segurança com o painel aberto e escondido (o evento do Windows pode faltar).
const SAFETY_VISIBLE: Duration = Duration::from_secs(3);
const SAFETY_HIDDEN: Duration = Duration::from_secs(30);
/// Cerca de 15 leituras por segundo para o medidor, e espera curta enquanto não há o que medir.
const MIC_INTERVAL: Duration = Duration::from_millis(66);
const MIC_IDLE: Duration = Duration::from_millis(250);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Precisa ser o primeiro plugin: uma segunda execução só mostra o painel da primeira.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::show_panel(app);
        }))
        .setup(|app| {
            shell::setup(app)?;
            start_engine(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_app_route,
            commands::set_session_volume,
            commands::set_session_muted,
            commands::set_device_volume,
            commands::set_device_muted,
            commands::set_default_device,
            commands::update_device_meta,
            commands::save_profile,
            commands::delete_profile,
            commands::activate_profile,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Sonar");
}

/// Cria o `Engine` com as implementações do Windows e liga as fontes de atualização.
/// Todas as threads são destacadas: não seguram o processo depois que o app fecha.
fn start_engine(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = ProjectDirs::from("com", "sthevandev", "sonar")
        .ok_or("não foi possível localizar a pasta de dados do usuário")?
        .data_dir()
        .to_path_buf();
    std::fs::create_dir_all(&data_dir)?;

    let app_handle = app.handle().clone();
    let notify_handle = app_handle.clone();
    let engine = Arc::new(Engine::new(
        Arc::new(WindowsAudioSource),
        Arc::new(WindowsRouteBackend::new()),
        Arc::new(WindowsEndpoint),
        Arc::new(WindowsDefaultDevice),
        Store::open(data_dir),
        Arc::new(move |state: &AppState| {
            // Falha ao emitir não deve derrubar a mudança de estado.
            let _ = notify_handle.emit(commands::STATE_CHANGED, state);
        }),
    ));
    app.manage(engine.clone());

    // Leitura inicial fora da thread principal (COM e o enumerador podem demorar).
    {
        let engine = engine.clone();
        std::thread::spawn(move || {
            engine.refresh();
            dump_state_once(&engine);
        });
    }

    // Eventos do Windows disparam releitura. O WatchHandle fica dentro da thread consumidora,
    // então vive enquanto o app viver.
    let (tx, rx) = mpsc::channel::<AudioEvent>();
    match watch(tx) {
        Ok(handle) => {
            let engine = engine.clone();
            std::thread::spawn(move || {
                let _watch = handle;
                while rx.recv().is_ok() {
                    // Junta os eventos que já chegaram: uma releitura cobre todos.
                    while rx.try_recv().is_ok() {}
                    engine.refresh();
                }
            });
        }
        Err(err) => {
            eprintln!("sonar: notificações de áudio indisponíveis, usando só a releitura periódica: {err:?}");
        }
    }

    // Rede de segurança: relê a cada 3 s com o painel aberto e a cada 30 s fechado.
    {
        let engine = engine.clone();
        let app_handle = app_handle.clone();
        std::thread::spawn(move || {
            let mut last = Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let every = if panel_visible(&app_handle) {
                    SAFETY_VISIBLE
                } else {
                    SAFETY_HIDDEN
                };
                if last.elapsed() >= every {
                    engine.refresh();
                    last = Instant::now();
                }
            }
        });
    }

    // Medidor do microfone padrão, só com o painel aberto.
    {
        let engine = engine.clone();
        let app_handle = app_handle.clone();
        std::thread::spawn(move || loop {
            if !panel_visible(&app_handle) {
                std::thread::sleep(MIC_IDLE);
                continue;
            }
            match engine.default_capture() {
                Some(id) => {
                    let peak = engine.mic_peak(&id);
                    let level = MicLevel { id: id.0, peak };
                    let _ = app_handle.emit(commands::MIC_LEVEL, level);
                    std::thread::sleep(MIC_INTERVAL);
                }
                None => std::thread::sleep(MIC_IDLE),
            }
        });
    }

    Ok(())
}

fn panel_visible(app: &AppHandle) -> bool {
    app.get_webview_window(PANEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// Ferramenta de desenvolvimento: com `SONAR_DEBUG_STATE` definida, imprime o primeiro
/// estado lido em JSON. Só existe em builds de depuração.
#[cfg(debug_assertions)]
fn dump_state_once(engine: &Engine) {
    if std::env::var_os("SONAR_DEBUG_STATE").is_none() {
        return;
    }
    match serde_json::to_string_pretty(&engine.state()) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("sonar: não foi possível serializar o estado: {err}"),
    }
}

#[cfg(not(debug_assertions))]
fn dump_state_once(_engine: &Engine) {}
