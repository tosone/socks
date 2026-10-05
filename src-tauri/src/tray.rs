use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager, Wry,
};

use crate::session::AppState;

const MENU_OPEN: &str = "tray-open";
const MENU_TOGGLE: &str = "tray-toggle";
const MENU_QUIT: &str = "tray-quit";

/// Handles kept around so the menu-bar icon and the connect/disconnect label
/// can follow the connection state.
struct Handles {
    tray: TrayIcon<Wry>,
    toggle_item: MenuItem<Wry>,
    connected_icon: Image<'static>,
    idle_icon: Image<'static>,
}

impl Handles {
    fn apply(&self, connected: bool) {
        let icon = if connected {
            self.connected_icon.clone()
        } else {
            self.idle_icon.clone()
        };
        let _ = self.tray.set_icon(Some(icon));
        let _ = self
            .toggle_item
            .set_text(if connected { "断开" } else { "连接" });
        let _ = self.tray.set_tooltip(Some(if connected {
            "socks · 已连接"
        } else {
            "socks · 未连接"
        }));
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, MENU_OPEN, "打开", true, None::<&str>)?;
    let toggle_item = MenuItem::with_id(app, MENU_TOGGLE, "连接", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &toggle_item, &quit_item])?;

    let connected_icon = Image::from_bytes(include_bytes!("../icons/icon.png"))?;
    let idle_icon = desaturate(&connected_icon);

    let tray = TrayIconBuilder::with_id("socks-tray")
        .icon(idle_icon.clone())
        .icon_as_template(false)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => show_main_window(app),
            MENU_TOGGLE => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { toggle_connection(app).await });
            }
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(Handles {
        tray,
        toggle_item,
        connected_icon,
        idle_icon,
    });

    Ok(())
}

/// Keep the menu-bar icon and the connect/disconnect label in sync with the
/// connection state. Called after every connect/disconnect.
pub fn apply(app: &AppHandle, connected: bool) {
    if let Some(handles) = app.try_state::<Handles>() {
        handles.apply(connected);
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

async fn toggle_connection(app: AppHandle) {
    let connected = {
        let state = app.state::<AppState>();
        if state.active_id().await.is_some() {
            let _ = state.disconnect().await;
            false
        } else {
            let mut target = state.last_active_id().await;
            if target.is_none() {
                target = state
                    .list_profiles()
                    .await
                    .first()
                    .map(|profile| profile.id.clone());
            }
            match target {
                Some(id) => state.connect(&id).await.is_ok(),
                None => false,
            }
        }
    };

    apply(&app, connected);
    let _ = app.emit("runtime-status", ());
}

/// Mute the colored app icon into a gray one for the disconnected state.
fn desaturate(source: &Image<'static>) -> Image<'static> {
    let mut rgba = source.rgba().to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        let luminance =
            (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32).round()
                as u8;
        pixel[0] = luminance;
        pixel[1] = luminance;
        pixel[2] = luminance;
        pixel[3] = (u16::from(pixel[3]) * 3 / 4) as u8;
    }
    Image::new_owned(rgba, source.width(), source.height())
}
