mod commands;
mod render;
mod seed;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_traits,
            commands::list_palette_colors,
            commands::decode_seed,
            commands::generate_candidates,
            commands::cancel_search,
            commands::render_seed,
            commands::save_seed_png,
            commands::delete_seed_png,
            commands::export_seed_png,
            commands::layout_summary,
            commands::random_seed_for_address,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
