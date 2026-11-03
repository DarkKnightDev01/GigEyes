// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    eframe::run_ui_native("GigEyes", Default::default(), |ui, _frame| {
        ui.heading("GigEyes");
        ui.label("Nothing here yet. Run gp-probe first.");
    })
}
