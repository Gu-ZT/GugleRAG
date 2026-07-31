#[cfg(windows)]
use image::{ImageFormat, imageops::FilterType, load_from_memory_with_format};
#[cfg(windows)]
use std::{
    io::{self, IsTerminal},
    sync::mpsc::{self, SyncSender},
    thread,
};
#[cfg(windows)]
use tao::{
    event::Event,
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
};
use tokio::sync::watch;
#[cfg(windows)]
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};

#[cfg(windows)]
pub struct DesktopTray {
    event_loop_proxy: EventLoopProxy<TrayCommand>,
}

#[cfg(not(windows))]
pub struct DesktopTray;

#[cfg(windows)]
enum TrayCommand {
    UpdateTooltip(String),
    Exit,
}

pub fn is_desktop_launch() -> bool {
    #[cfg(windows)]
    {
        !io::stdin().is_terminal() && !io::stdout().is_terminal() && !io::stderr().is_terminal()
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
impl DesktopTray {
    pub fn start(listener_url: &str, shutdown_tx: watch::Sender<bool>) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let listener_url = listener_url.to_string();
        thread::Builder::new()
            .name("guglerag-tray".to_string())
            .spawn(move || run_tray(listener_url, shutdown_tx, ready_tx))
            .map_err(|error| format!("failed to start system tray thread: {error}"))?;

        let event_loop_proxy = ready_rx
            .recv()
            .map_err(|error| format!("system tray did not start: {error}"))??;
        Ok(Self { event_loop_proxy })
    }

    pub fn update_listener_url(&self, listener_url: &str) {
        let _ = self
            .event_loop_proxy
            .send_event(TrayCommand::UpdateTooltip(tooltip(listener_url)));
    }
}

#[cfg(windows)]
impl Drop for DesktopTray {
    fn drop(&mut self) {
        let _ = self.event_loop_proxy.send_event(TrayCommand::Exit);
    }
}

#[cfg(not(windows))]
impl DesktopTray {
    pub fn start(_listener_url: &str, _shutdown_tx: watch::Sender<bool>) -> Result<Self, String> {
        Err("system tray support is only available on Windows".to_string())
    }

    pub fn update_listener_url(&self, _listener_url: &str) {}
}

#[cfg(windows)]
fn run_tray(
    listener_url: String,
    shutdown_tx: watch::Sender<bool>,
    ready_tx: SyncSender<Result<EventLoopProxy<TrayCommand>, String>>,
) {
    let event_loop = EventLoopBuilder::<TrayCommand>::with_user_event().build();
    let event_loop_proxy = event_loop.create_proxy();
    let menu = Menu::new();
    let quit_item = MenuItem::new("退出 GugleRAG", true, None);
    if let Err(error) = menu.append(&quit_item) {
        let _ = ready_tx.send(Err(format!("failed to build system tray menu: {error}")));
        return;
    }

    let quit_id = quit_item.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            shutdown_tx.send_replace(true);
        }
    }));

    let icon = match app_icon() {
        Ok(icon) => icon,
        Err(error) => {
            let _ = ready_tx.send(Err(error));
            return;
        }
    };
    let tray_icon = match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip(&listener_url))
        .with_icon(icon)
        .build()
    {
        Ok(tray_icon) => tray_icon,
        Err(error) => {
            let _ = ready_tx.send(Err(format!("failed to create system tray icon: {error}")));
            return;
        }
    };

    if ready_tx.send(Ok(event_loop_proxy)).is_err() {
        return;
    }

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::UserEvent(command) = event {
            match command {
                TrayCommand::UpdateTooltip(tooltip) => {
                    if let Err(error) = tray_icon.set_tooltip(Some(tooltip)) {
                        tracing::warn!("failed to update system tray tooltip: {error}");
                    }
                }
                TrayCommand::Exit => *control_flow = ControlFlow::Exit,
            }
        }
    });
}

#[cfg(windows)]
fn app_icon() -> Result<Icon, String> {
    let image = load_from_memory_with_format(
        include_bytes!("../frontend/public/icon.png"),
        ImageFormat::Png,
    )
    .map_err(|error| format!("failed to decode system tray icon: {error}"))?
    .resize_exact(32, 32, FilterType::Lanczos3)
    .to_rgba8();
    Icon::from_rgba(image.into_raw(), 32, 32)
        .map_err(|error| format!("failed to create system tray icon: {error}"))
}

#[cfg(windows)]
fn tooltip(listener_url: &str) -> String {
    format!("GugleRAG - {listener_url}")
}
