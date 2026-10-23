// Release builds are GUI-subsystem so no console window appears at login.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    productivity_tracker_lib::run();
}
