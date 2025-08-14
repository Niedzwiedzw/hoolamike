pub mod ipc;

#[cfg(not(all(target_os = "windows", not(debug_assertions))))]
pub mod wine_context;
