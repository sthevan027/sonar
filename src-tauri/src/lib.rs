mod shell;
mod shell_geometry;
#[allow(dead_code)] // `spawn` (janela manual) só é usada nos testes; o resto é consumido pelo app.
mod dispatcher;
#[allow(dead_code)] // As variantes de `ErrorCode` que ainda não têm ação ficam sem construção; o allow cobre o módulo.
mod actions;
mod action_runtime;
mod bindings;
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
                        let bindings = tabela_de_atalhos(app);
                        let avisar = action_runtime::notificador(app);
                        action_runtime::enfileirar_atalho(&dispatcher, &bindings, shortcut, &avisar);
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
    let project = ProjectDirs::from("com", "sthevandev", "sonar")
        .map(|dirs| dirs.data_dir().to_path_buf());
    let data_dir = resolve_data_dir(project, std::env::temp_dir().join("sonar"));
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
            if !resultado.ok {
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

/// Pasta de dados: a do sistema quando existe, senão a de reserva.
fn resolve_data_dir(project: Option<PathBuf>, fallback: PathBuf) -> PathBuf {
    project.unwrap_or(fallback)
}

/// Roda `f` segurando pânico: loga e devolve `false` se ele ocorreu, para a thread
/// seguir no laço. Em build de release `panic = "abort"` (Cargo.toml) não há o que
/// segurar; o guarda só vale em depuração e nos testes.
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

/// A tabela de atalhos valendo agora: os padrão do app mais o atalho legado das
/// configurações (que ainda alterna o painel).
///
/// O atalho legado entra na tabela enquanto a Task 14 não unificar as duas coisas: sem
/// ele, o atalho que o usuário salvou deixaria de funcionar.
fn tabela_de_atalhos(app: &AppHandle) -> Vec<bindings::Binding> {
    let mut tabela = bindings::default_bindings();
    let legado = app
        .try_state::<Arc<Engine>>()
        .map(|engine| engine.settings().shortcut);
    if let Some(legado) = legado {
        let ja_existe = tabela
            .iter()
            .any(|binding| binding.accelerator.eq_ignore_ascii_case(&legado));
        if !ja_existe {
            tabela.push(bindings::Binding {
                accelerator: legado,
                action: actions::Action::TogglePanel,
            });
        }
    }
    tabela
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
