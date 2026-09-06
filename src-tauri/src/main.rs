// Hides the console window on a release build. Debug builds keep it, because
// watching the log stream while a preflight runs is how this gets developed.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    team_principal_lib::run();
}
