mod shell;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Precisa ser o primeiro plugin: uma segunda execução só mostra o painel da primeira.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::show_panel(app);
        }))
        .setup(|app| {
            shell::setup(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Sonar");
}
