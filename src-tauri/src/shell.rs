//! Bandeja do sistema, janela do painel, atalho global e "Iniciar com o Windows".

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::actions::Action;
use crate::action_runtime;
use crate::dispatcher::Dispatcher;
use crate::hotkeys;
use crate::shell_geometry::{corner_fallback, place_near_tray, Rect, Size};
use crate::state::Settings;

const PANEL: &str = "panel";
const TRAY_ID: &str = "main-tray";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
/// Tamanho do painel em pixels lógicos (igual ao `tauri.conf.json`); em pixels físicos vem
/// multiplicado pela escala do monitor onde ele abre.
const PANEL_WIDTH: f64 = 420.0;
const PANEL_HEIGHT: f64 = 700.0;
const AUTOSTART_VALUE: &str = "Sonar";
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Depois de o painel se esconder por perda de foco, um clique na bandeja dentro
/// deste intervalo não o reabre: o próprio clique é o que tirou o foco (mousedown
/// esconde, mouseup chega como clique).
const BLUR_GRACE: Duration = Duration::from_millis(250);

/// Quando o painel foi escondido por perda de foco pela última vez.
type HiddenAt = Arc<Mutex<Option<Instant>>>;

/// Último retângulo conhecido do ícone da bandeja, em pixels físicos. `None` antes do primeiro
/// evento com posição válida: o painel então abre no canto da área de trabalho.
static LAST_TRAY: Mutex<Option<Rect>> = Mutex::new(None);

/// Enquanto o diálogo "Procurar .exe" está aberto, o painel perde o foco para ele e não deve
/// se esconder. A interface liga e desliga pelo comando `set_blur_hide_suspended`. O prazo é
/// a rede de segurança: se o "desligar" nunca chegar, o painel volta ao normal sozinho.
const BLUR_SUSPEND_MAX: Duration = Duration::from_secs(60);

/// Até quando a suspensão vale (`None` = sem suspensão).
static BLUR_SUSPENDED_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Prazo para `suspended` a partir de `now`; `None` cancela a suspensão.
fn suspension_deadline(suspended: bool, now: Instant) -> Option<Instant> {
    suspended.then(|| now + BLUR_SUSPEND_MAX)
}

/// Suspenso enquanto há prazo e ele ainda não passou.
fn is_suspended(deadline: Option<Instant>, now: Instant) -> bool {
    deadline.is_some_and(|until| now < until)
}

fn blur_deadline() -> MutexGuard<'static, Option<Instant>> {
    locked(&BLUR_SUSPENDED_UNTIL)
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Liga/desliga a suspensão. Ao desligar, se o painel continua visível e sem foco (o diálogo
/// tomou o foco e não o devolveu), ele se esconde como faria pela perda de foco normal.
pub fn set_blur_hide_suspended(app: &AppHandle, suspended: bool) {
    *blur_deadline() = suspension_deadline(suspended, Instant::now());
    if suspended {
        return;
    }
    if let Some(panel) = app.get_webview_window(PANEL) {
        if panel.is_visible().unwrap_or(false) && !panel.is_focused().unwrap_or(false) {
            let _ = panel.hide();
        }
    }
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    // Sonda de conflito de atalho (S4): só em depuração e só com a variável `SONAR_PROBE_SHORTCUTS`
    // definida. Roda antes de o app registrar o atalho guardado.
    #[cfg(debug_assertions)]
    probe_shortcuts_from_env(app.handle());

    let panel = app
        .get_webview_window(PANEL)
        .ok_or(tauri::Error::WindowNotFound)?;
    let hidden_at: HiddenAt = Arc::new(Mutex::new(None));
    // O atalho global usa o mesmo `hidden_at` da bandeja: a graça de 250 ms vale para os dois.
    app.manage(hidden_at.clone());

    // Perder o foco esconde o painel. Fechar (Alt+F4) também só esconde: o app vive na bandeja.
    {
        let window = panel.clone();
        let hidden_at = hidden_at.clone();
        panel.on_window_event(move |event| match event {
            WindowEvent::Focused(false) => {
                if !is_suspended(*blur_deadline(), Instant::now())
                    && window.is_visible().unwrap_or(false)
                {
                    let _ = window.hide();
                    *lock(&hidden_at) = Some(Instant::now());
                }
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        });
    }

    let menu = MenuBuilder::new(app)
        .text("open", "Abrir")
        .separator()
        .text("quit", "Sair")
        .build()?;

    let icon = Image::from_bytes(TRAY_ICON)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Sonar")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_panel(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            let app = tray.app_handle();
            remember_tray_rect(app, &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                alterna_pelo_dispatcher(app);
            }
        })
        .build(app)?;

    Ok(())
}

/// Guarda onde está o ícone (pixels físicos). Posição ou tamanho zerados não contam.
fn remember_tray_rect(app: &AppHandle, event: &TrayIconEvent) {
    let rect = match event {
        TrayIconEvent::Click { rect, .. }
        | TrayIconEvent::DoubleClick { rect, .. }
        | TrayIconEvent::Enter { rect, .. }
        | TrayIconEvent::Move { rect, .. }
        | TrayIconEvent::Leave { rect, .. } => rect,
        _ => return,
    };
    let scale = app
        .get_webview_window(PANEL)
        .and_then(|panel| panel.scale_factor().ok())
        .unwrap_or(1.0);
    let position = rect.position.to_physical::<i32>(scale);
    let size = rect.size.to_physical::<u32>(scale);
    let known = size.width > 0 && size.height > 0 && (position.x, position.y) != (0, 0);
    *locked(&LAST_TRAY) = known.then(|| Rect {
        x: position.x,
        y: position.y,
        width: size.width as i32,
        height: size.height as i32,
    });
}

/// Mostra e foca o painel. Usado pelo menu "Abrir" e pela segunda execução do app.
pub fn show_panel(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        show(&panel);
    }
}

/// Alterna o painel. Devolve se o estado mudou de fato (`false` quando o painel não
/// existe ou quando o clique foi o que tirou o foco dele).
fn toggle_panel(app: &AppHandle, hidden_at: &HiddenAt) -> bool {
    let Some(panel) = app.get_webview_window(PANEL) else {
        return false;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return true;
    }
    let just_hidden_by_blur = lock(hidden_at).is_some_and(|at| at.elapsed() < BLUR_GRACE);
    if just_hidden_by_blur {
        return false;
    }
    show(&panel);
    true
}

/// Atalho global: alterna o painel pelo mesmo caminho da bandeja (graça e suspensão valem).
///
/// O painel é do app, não do motor de áudio: é por aqui que a ação `TogglePanel` chega
/// quando o `AppExecutor` a executa.
pub fn toggle_from_shortcut(app: &AppHandle) -> bool {
    let Some(hidden_at) = app.try_state::<HiddenAt>() else {
        return false;
    };
    toggle_panel(app, &hidden_at)
}

/// O clique na bandeja vira uma ação: o painel alterna pelo mesmo caminho do atalho.
///
/// A ação entra na fila; quem executa é o worker do `Dispatcher`. Sem fila (o app ainda
/// está abrindo), o clique vai pelo caminho direto — perder o clique seria pior.
fn alterna_pelo_dispatcher(app: &AppHandle) {
    let Some(dispatcher) = app.try_state::<Dispatcher>() else {
        if let Some(hidden_at) = app.try_state::<HiddenAt>() {
            toggle_panel(app, &hidden_at);
        }
        return;
    };
    let avisar = action_runtime::notificador(app);
    action_runtime::submit(&dispatcher, Action::TogglePanel, &avisar);
}

fn show(panel: &WebviewWindow) {
    // Mostrar o painel é ação do usuário: nenhum diálogo segue suspendendo o esconder.
    *blur_deadline() = None;
    // Posiciona antes de mostrar: o painel nunca aparece no centro da tela.
    position_panel(panel);
    let _ = panel.show();
    let _ = panel.set_focus();
}

/// Põe o painel perto do ícone da bandeja (último conhecido), dentro da área de trabalho do
/// monitor onde o ícone está. Sem ícone conhecido: canto inferior direito do monitor principal.
fn position_panel(panel: &WebviewWindow) {
    let tray = *locked(&LAST_TRAY);
    let monitor = match tray {
        Some(rect) => panel
            .monitor_from_point(
                f64::from(rect.x) + f64::from(rect.width) / 2.0,
                f64::from(rect.y) + f64::from(rect.height) / 2.0,
            )
            .ok()
            .flatten(),
        None => None,
    }
    .or_else(|| panel.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let window = Size {
        width: (PANEL_WIDTH * scale).round() as i32,
        height: (PANEL_HEIGHT * scale).round() as i32,
    };
    let work = monitor.work_area();
    let work_area = Rect {
        x: work.position.x,
        y: work.position.y,
        width: work.size.width as i32,
        height: work.size.height as i32,
    };
    let point = match tray {
        Some(rect) => place_near_tray(rect, work_area, window),
        None => corner_fallback(work_area, window),
    };
    let _ = panel.set_position(PhysicalPosition::new(point.x, point.y));
}

fn lock(hidden_at: &HiddenAt) -> MutexGuard<'_, Option<Instant>> {
    hidden_at.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Quem troca o atalho registrado. Separado do plugin para os testes rodarem sem o sistema.
pub trait ShortcutRegistrar {
    /// `Err` com frase em português quando o texto não é um atalho ou o sistema recusa.
    fn register(&self, accelerator: &str) -> Result<(), String>;
    fn unregister(&self, accelerator: &str);
}

/// Troca o atalho. Registra o novo antes de soltar o antigo: se o novo falhar, o antigo segue
/// valendo e nada muda. Atalho igual ao atual não faz nada.
pub fn switch_shortcut(
    registrar: &dyn ShortcutRegistrar,
    current: Option<&str>,
    next: &str,
) -> Result<(), String> {
    if current == Some(next) {
        return Ok(());
    }
    registrar.register(next)?;
    if let Some(old) = current {
        registrar.unregister(old);
    }
    Ok(())
}

/// Registro real pelo plugin `tauri-plugin-global-shortcut`.
struct PluginShortcuts<'a> {
    app: &'a AppHandle,
}

impl ShortcutRegistrar for PluginShortcuts<'_> {
    fn register(&self, accelerator: &str) -> Result<(), String> {
        let shortcut: Shortcut = accelerator
            .parse()
            .map_err(|_| format!("Atalho inválido: {accelerator}."))?;
        self.app.global_shortcut().register(shortcut).map_err(|_| {
            format!(
                "Não foi possível registrar o atalho {accelerator}. \
                 Outro programa pode estar usando essa combinação."
            )
        })
    }

    fn unregister(&self, accelerator: &str) {
        if let Ok(shortcut) = accelerator.parse::<Shortcut>() {
            let _ = self.app.global_shortcut().unregister(shortcut);
        }
    }
}

/// O registrador real: o plugin de atalho global do Windows.
///
/// É a ponte entre a lógica de binding (`hotkeys`) e o plugin: os testes usam um
/// registrador falso, a produção usa este.
pub fn registrador(app: &AppHandle) -> impl ShortcutRegistrar + '_ {
    PluginShortcuts { app }
}

/// Sonda de conflito de atalho (spike S4): registra cada acelerador por um instante e solta,
/// para saber se o Windows aceita registrá-lo. Ela registra e solta, uma tecla por vez, na
/// ordem: um atalho que falhou não interrompe os outros e nada fica preso no sistema.
/// Não é o mecanismo de registro do app (esse é o `apply_bindings` da fase F2).
#[cfg(any(debug_assertions, test))]
fn probe_shortcuts(
    registrar: &dyn ShortcutRegistrar,
    accelerators: &[&str],
) -> Vec<(String, Result<(), String>)> {
    accelerators
        .iter()
        .map(|accelerator| {
            let result = registrar.register(accelerator);
            // Solta sempre, mesmo depois de falhar: a sonda devolve o sistema como achou.
            registrar.unregister(accelerator);
            ((*accelerator).to_string(), result)
        })
        .collect()
}

/// Roda a sonda na abertura do app quando a variável `SONAR_PROBE_SHORTCUTS` lista os
/// aceleradores (separados por vírgula). Só existe em builds de depuração: em release a
/// variável nem é lida.
#[cfg(debug_assertions)]
fn probe_shortcuts_from_env(app: &AppHandle) {
    let Ok(raw) = std::env::var("SONAR_PROBE_SHORTCUTS") else {
        return;
    };
    let accelerators: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if accelerators.is_empty() {
        return;
    }
    eprintln!("sonar: sonda de atalhos ({} combinações)", accelerators.len());
    let registrar = PluginShortcuts { app };
    for (accelerator, result) in probe_shortcuts(&registrar, &accelerators) {
        match result {
            Ok(()) => eprintln!("sonar: sonda OK {accelerator}"),
            Err(err) => eprintln!("sonar: sonda FALHOU {accelerator}: {err}"),
        }
    }
}

/// Liga ou desliga "Iniciar com o Windows": valor `Sonar` em `HKCU\...\Run` com o caminho do
/// executável entre aspas. Desligar remove o valor (sem erro se ele não existir).
#[cfg(windows)]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    use std::io::ErrorKind;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    // `create_subkey_with_flags` devolve (chave, se foi criada ou aberta); só a chave importa.
    let (run, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
        .map_err(|err| format!("Não foi possível abrir o registro do Windows ({err})."))?;
    if enabled {
        let exe = std::env::current_exe()
            .map_err(|err| format!("Não foi possível achar o programa ({err})."))?;
        run.set_value(AUTOSTART_VALUE, &format!("\"{}\"", exe.display()))
            .map_err(|err| format!("Não foi possível ligar Iniciar com o Windows ({err})."))
    } else {
        match run.delete_value(AUTOSTART_VALUE) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!(
                "Não foi possível desligar Iniciar com o Windows ({err})."
            )),
        }
    }
}

#[cfg(not(windows))]
pub fn set_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}

/// Aplica o que foi salvo ao abrir o app: os atalhos da lista de bindings e "Iniciar com
/// o Windows" (que re-grava o caminho do executável, caso ele tenha mudado).
///
/// O atalho legado não é registrado por aqui: ele virou o binding `toggle_panel` e quem
/// registra os atalhos é o `hotkeys`. Falha vira aviso no log: o app segue.
pub fn apply_at_startup(app: &AppHandle, settings: &Settings) {
    if let Err(err) = set_autostart(settings.autostart) {
        eprintln!("sonar: {err}");
    }
    hotkeys::apply_at_startup(app, settings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn suspension_deadline_is_set_only_when_suspending() {
        let now = Instant::now();
        assert_eq!(suspension_deadline(false, now), None);
        assert_eq!(suspension_deadline(true, now), Some(now + BLUR_SUSPEND_MAX));
    }

    #[test]
    fn suspension_holds_until_deadline_then_expires() {
        let now = Instant::now();
        let deadline = suspension_deadline(true, now);
        assert!(is_suspended(deadline, now));
        assert!(is_suspended(deadline, now + BLUR_SUSPEND_MAX - Duration::from_millis(1)));
        assert!(!is_suspended(deadline, now + BLUR_SUSPEND_MAX));
        assert!(!is_suspended(None, now));
    }

    /// Registro falso: atalhos que começam com "bad" são recusados; o resto é aceito.
    #[derive(Default)]
    struct FakeRegistrar {
        calls: RefCell<Vec<String>>,
    }

    impl ShortcutRegistrar for FakeRegistrar {
        fn register(&self, accelerator: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("register {accelerator}"));
            if accelerator.starts_with("bad") {
                return Err(format!("Atalho inválido: {accelerator}."));
            }
            Ok(())
        }

        fn unregister(&self, accelerator: &str) {
            self.calls.borrow_mut().push(format!("unregister {accelerator}"));
        }
    }

    #[test]
    fn invalid_shortcut_keeps_previous() {
        let registrar = FakeRegistrar::default();

        let result = switch_shortcut(&registrar, Some("Super+Alt+V"), "bad+Key");

        assert_eq!(result, Err("Atalho inválido: bad+Key.".to_string()));
        // O antigo não foi soltado: continua valendo.
        assert_eq!(*registrar.calls.borrow(), vec!["register bad+Key".to_string()]);
    }

    #[test]
    fn switch_registers_new_before_releasing_old() {
        let registrar = FakeRegistrar::default();

        switch_shortcut(&registrar, Some("Ctrl+Alt+K"), "Super+Alt+V").unwrap();

        assert_eq!(
            *registrar.calls.borrow(),
            vec![
                "register Super+Alt+V".to_string(),
                "unregister Ctrl+Alt+K".to_string(),
            ]
        );
    }

    #[test]
    fn same_shortcut_is_a_no_op() {
        let registrar = FakeRegistrar::default();

        switch_shortcut(&registrar, Some("Super+Alt+V"), "Super+Alt+V").unwrap();

        assert!(registrar.calls.borrow().is_empty());
    }

    #[test]
    fn probe_reports_each_key_independently() {
        let registrar = FakeRegistrar::default();

        let results = probe_shortcuts(&registrar, &["Ctrl+Alt+M", "bad+Key", "Ctrl+Alt+Up"]);

        assert_eq!(results.len(), 3);
        // A ordem é a da lista e cada tecla tem o próprio resultado: a falha de uma não
        // interrompe as outras.
        assert_eq!(results[0].0, "Ctrl+Alt+M");
        assert_eq!(results[0].1, Ok(()));
        assert_eq!(results[1].0, "bad+Key");
        assert_eq!(results[1].1, Err("Atalho inválido: bad+Key.".to_string()));
        assert_eq!(results[2].0, "Ctrl+Alt+Up");
        assert_eq!(results[2].1, Ok(()));
        // Cada acelerador é registrado e solto antes do próximo: a sonda não deixa atalho
        // preso no sistema.
        assert_eq!(
            *registrar.calls.borrow(),
            vec![
                "register Ctrl+Alt+M".to_string(),
                "unregister Ctrl+Alt+M".to_string(),
                "register bad+Key".to_string(),
                "unregister bad+Key".to_string(),
                "register Ctrl+Alt+Up".to_string(),
                "unregister Ctrl+Alt+Up".to_string(),
            ]
        );
    }

    #[test]
    fn every_shortcut_the_settings_screen_can_produce_parses() {
        // Mesmo conjunto de `shortcutFromKeys` (src/screens/settingsModel.ts): Ctrl, Alt,
        // Shift e Super, com pelo menos Ctrl, Alt ou Super; tecla letra, dígito, F1 a F12
        // ou uma das setas (Up, Down).
        let modifiers = ["Ctrl", "Alt", "Shift", "Super"];
        let mut mains: Vec<String> = ('A'..='Z').map(|c| c.to_string()).collect();
        mains.extend((0..=9).map(|d| d.to_string()));
        mains.extend((1..=12).map(|f| format!("F{f}")));
        mains.push("Up".to_string());
        mains.push("Down".to_string());
        let mut checked = 0;
        for mask in 0u32..16 {
            let chosen: Vec<&str> = modifiers
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, m)| *m)
                .collect();
            let strong = chosen.iter().any(|m| *m != "Shift");
            if !strong {
                continue;
            }
            for main in &mains {
                let mut parts = chosen.clone();
                parts.push(main);
                let accelerator = parts.join("+");
                assert!(
                    accelerator.parse::<Shortcut>().is_ok(),
                    "atalho da tela não é aceito pelo plugin: {accelerator}"
                );
                checked += 1;
            }
        }
        // 14 combinações de modificadores (16 máscaras menos a vazia e a de só Shift) x 50 teclas
        // principais (26 letras + 10 dígitos + 12 teclas F + Up + Down).
        assert_eq!(mains.len(), 26 + 10 + 12 + 2);
        assert_eq!(checked, 14 * 50);
        assert!("Super+Alt+V".parse::<Shortcut>().is_ok());
    }
}
