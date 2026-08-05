mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::dsl_parse,
            commands::dsl_parse_checked,
            commands::dsl_serialize,
            commands::expr_eval,
            commands::generate_data,
            commands::save_text_file,
            commands::read_text_file,
            commands::open_dir,
            commands::compile_program,
            commands::run_program_ipc,
            commands::duipai_start,
            commands::duipai_cancel,
            commands::duipai_running,
            commands::nl_to_dsl_ipc,
            commands::model_status,
            commands::model_set_path,
            commands::model_load,
            commands::model_download
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
