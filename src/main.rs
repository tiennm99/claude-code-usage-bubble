// Release builds use the GUI subsystem so no console window opens with the
// app. Debug builds keep the console for log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    usage_monitor_lib::run();
}
