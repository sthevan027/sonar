mod shell;
mod shell_geometry;
mod dispatcher;
mod actions;
mod action_runtime;
mod bindings;
mod hotkeys;
mod audio;
mod rules;
mod store;
mod profiles;
mod state;
mod engine;
mod commands;
mod apps;
mod updates;

use std::path::PathBuf;
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
use dispatcher::Dispatcher;
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
    install_crash_log();
    tauri::Builder::default()
        // Precisa ser o primeiro plugin: uma segunda execução só mostra o painel da primeira.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::show_panel(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        // Atualização: checa o endpoint de `plugins.updater` (tauri.conf.json) e verifica a assinatura.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // O atalho global vira uma ação e entra na fila: o handler não executa nada, ele
        // só enfileira. Quem executa é o worker do `Dispatcher`.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let Some(dispatcher) = app.try_state::<Dispatcher>() else {
                            return;
                        };
                        let bindings = hotkeys::registrados();
                        let avisar = action_runtime::notificador(app);
                        action_runtime::enfileirar_atalho(
                            &dispatcher,
                            &bindings,
                            shortcut,
                            &avisar,
                        );
                    }
                })
                .build(),
        )
        .setup(|app| {
            shell::setup(app)?;
            start_engine(app)?;
            app.manage(updates::UpdateState::default());
            updates::start_background_checks(app.handle().clone());
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
            commands::save_bindings,
            commands::get_binding_status,
            commands::get_default_bindings,
            commands::list_running_apps,
            commands::app_icon,
            commands::set_blur_hide_suspended,
            commands::dispatch_action,
            updates::check_for_update,
            updates::install_update,
            updates::pending_update,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Sonar");
}

/// Cria o `Engine` com as implementações do Windows e liga as fontes de atualização.
/// Todas as threads são destacadas: não seguram o processo depois que o app fecha.
fn start_engine(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app_data_dir();
    // Sem pasta, o app segue: a gravação falha e aparece em `save_error`.
    if let Err(err) = std::fs::create_dir_all(&data_dir) {
        eprintln!("sonar: não foi possível criar {}: {err}", data_dir.display());
    }

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

    // A fila de ações: é por ela que atalho, bandeja e IPC passam. O executor sabe mexer
    // no painel; o `report` publica no painel o que não deu certo.
    let executor = Arc::new(action_runtime::AppExecutor::new(
        engine.clone(),
        app_handle.clone(),
    ));
    let avisar = action_runtime::notificador(&app_handle);
    let report: Arc<dyn Fn(&actions::Action, &actions::ActionResult) + Send + Sync> =
        Arc::new(move |acao: &actions::Action, resultado: &actions::ActionResult| {
            // O evento sai só quando a ação não deu certo (Ruling 17). Um sucesso parcial
            // (`ok == false`, `changed == true`) também sai: o usuário precisa saber que
            // o pedido foi cumprido só pela metade.
            if action_runtime::reporta_se_falhou(resultado) {
                avisar(acao, resultado);
            }
        });
    let dispatcher = Dispatcher::spawn_default(executor, report);
    app.manage(dispatcher);
    // Atalho e "Iniciar com o Windows" guardados valem desde a abertura.
    shell::apply_at_startup(&app_handle, &engine.settings());

    // Leitura inicial fora da thread principal (COM e o enumerador podem demorar).
    {
        let engine = engine.clone();
        std::thread::spawn(move || {
            guarded("leitura inicial", || {
                engine.refresh();
                dump_state_once(&engine);
            });
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
                    guarded("releitura por evento", || engine.refresh());
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
                    guarded("releitura periódica", || engine.refresh());
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
            let mut pause = MIC_IDLE;
            guarded("medidor do microfone", || {
                if !panel_visible(&app_handle) {
                    return;
                }
                if let Some(id) = engine.default_capture() {
                    let peak = engine.mic_peak(&id);
                    let level = MicLevel { id: id.0, peak };
                    let _ = app_handle.emit(commands::MIC_LEVEL, level);
                    pause = MIC_INTERVAL;
                }
            });
            std::thread::sleep(pause);
        });
    }

    Ok(())
}

/// Pasta de dados do app: a do sistema quando existe, senão a de reserva.
fn app_data_dir() -> PathBuf {
    let project = ProjectDirs::from("com", "sthevandev", "sonar")
        .map(|dirs| dirs.data_dir().to_path_buf());
    resolve_data_dir(project, std::env::temp_dir().join("sonar"))
}

fn resolve_data_dir(project: Option<PathBuf>, fallback: PathBuf) -> PathBuf {
    project.unwrap_or(fallback)
}

/// Hook de pânico que grava a mensagem, a localização e o backtrace em `sonar-crash.log`
/// na pasta de dados, e depois chama o hook anterior (o padrão imprime no stderr).
fn install_crash_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Falha de escrita é ignorada: o hook não pode causar outro pânico.
        let _ = write_crash_log(info);
        previous(info);
    }));
}

fn write_crash_log(info: &std::panic::PanicHookInfo<'_>) -> std::io::Result<()> {
    use std::io::Write;

    let dir = app_data_dir();
    std::fs::create_dir_all(&dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("sonar-crash.log"))?;

    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(sem mensagem)".into());
    let location = info
        .location()
        .map_or_else(|| "(sem localização)".into(), |loc| loc.to_string());
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let backtrace = std::backtrace::Backtrace::force_capture();
    // Uma única escrita por pânico, para pânicos simultâneos não intercalarem linhas.
    let entry = format!("[{secs}] pânico em {location}: {payload}\n{backtrace}\n\n");
    file.write_all(entry.as_bytes())
}

/// Roda `f` segurando pânico: loga e devolve `false` se ele ocorreu, para a thread
/// seguir no laço. Vale em release também: `panic = "unwind"` (Cargo.toml) mantém o
/// desenrolar do pânico, que é o que o `catch_unwind` precisa.
fn guarded(label: &str, f: impl FnOnce()) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(()) => true,
        Err(_) => {
            eprintln!("sonar: {label} falhou com pânico; seguindo");
            false
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_uses_project_path_when_available() {
        let project = PathBuf::from("dados").join("sonar");
        let fallback = PathBuf::from("temp").join("sonar");
        assert_eq!(resolve_data_dir(Some(project.clone()), fallback), project);
    }

    #[test]
    fn data_dir_uses_fallback_when_project_dirs_missing() {
        let fallback = PathBuf::from("temp").join("sonar");
        assert_eq!(resolve_data_dir(None, fallback.clone()), fallback);
    }

    #[test]
    fn guarded_returns_true_when_closure_completes() {
        let mut ran = false;
        assert!(guarded("teste", || ran = true));
        assert!(ran);
    }

    #[test]
    fn guarded_contains_panic_and_returns_false() {
        assert!(!guarded("teste", || panic!("falha de propósito")));
    }
}
