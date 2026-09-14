// Espanso Studio - thin Tauri shell over the espanso config store.
// All real logic lives in store.rs (pure, unit-tested).

mod store;

#[tauri::command]
fn get_config_dir() -> Result<String, String> {
    store::config_dir_display()
}

#[tauri::command]
fn list_sets() -> Result<Vec<store::SetInfo>, String> {
    store::list_sets()
}

#[tauri::command]
fn save_set(path: String, yaml: String) -> Result<(), String> {
    store::save_set(&path, &yaml)
}

#[tauri::command]
fn create_set(name: String) -> Result<store::SetInfo, String> {
    store::create_set(&name)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_config_dir, list_sets, save_set, create_set])
        .run(tauri::generate_context!())
        .expect("error while running Espanso Studio");
}
