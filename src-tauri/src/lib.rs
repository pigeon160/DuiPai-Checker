mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::dsl_parse,
            commands::dsl_parse_checked,
            commands::dsl_serialize,
            commands::expr_eval
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
