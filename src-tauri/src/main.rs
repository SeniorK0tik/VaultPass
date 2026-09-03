// Точка входа. Вся сборка приложения — в `seif_lib::run`, чтобы тот же код
// можно было поднять из тестов и с мобильных целей.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    seif_lib::run()
}
