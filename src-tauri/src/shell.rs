//! Bandeja do sistema e janela do painel.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, WebviewWindow, WindowEvent};

const PANEL: &str = "panel";
const TRAY_ID: &str = "main-tray";
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");

/// Depois de o painel se esconder por perder o foco, um clique na bandeja dentro
/// deste intervalo não o reabre: o próprio clique é o que tirou o foco (mousedown
/// esconde, mouseup chega como clique).
const BLUR_GRACE: Duration = Duration::from_millis(250);

/// Quando o painel foi escondido por perda de foco pela última vez.
type HiddenAt = Arc<Mutex<Option<Instant>>>;

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

fn blur_deadline() -> std::sync::MutexGuard<'static, Option<Instant>> {
    BLUR_SUSPENDED_UNTIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    let panel = app
        .get_webview_window(PANEL)
        .ok_or(tauri::Error::WindowNotFound)?;
    let hidden_at: HiddenAt = Arc::new(Mutex::new(None));

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
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle(), &hidden_at);
            }
        })
        .build(app)?;

    Ok(())
}

/// Mostra e foca o painel. Usado pelo menu "Abrir" e pela segunda execução do app.
pub fn show_panel(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        show(&panel);
    }
}

fn toggle_panel(app: &AppHandle, hidden_at: &HiddenAt) {
    let Some(panel) = app.get_webview_window(PANEL) else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return;
    }
    let just_hidden_by_blur = lock(hidden_at).is_some_and(|at| at.elapsed() < BLUR_GRACE);
    if !just_hidden_by_blur {
        show(&panel);
    }
}

fn show(panel: &WebviewWindow) {
    // Mostrar o painel é ação do usuário: nenhum diálogo segue suspendendo o esconder.
    *blur_deadline() = None;
    let _ = panel.show();
    let _ = panel.set_focus();
}

fn lock(hidden_at: &HiddenAt) -> std::sync::MutexGuard<'_, Option<Instant>> {
    hidden_at.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
