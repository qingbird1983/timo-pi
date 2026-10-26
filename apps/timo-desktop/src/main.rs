//! Timo 桌面壳入口：只做进程级装配，窗口内容在 `timo_desktop::app`。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    timo_desktop::run();
}
