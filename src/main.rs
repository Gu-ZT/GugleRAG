#![cfg_attr(windows, windows_subsystem = "windows")]

#[tokio::main]
async fn main() {
    gugle_rag::run().await;
}
