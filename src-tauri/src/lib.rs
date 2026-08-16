use std::{
    borrow::Cow,
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use arboard::ImageData;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, ImageFormat};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSPasteboard;
use serde::{Deserialize, Serialize};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State,
};
#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const MAX_ITEMS: usize = 300;
const POLL_MS: u64 = 350;
const TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+V";
const IMAGE_DATA_URL_PREFIX: &str = "data:image/png;base64,";
const MODULE_CLIPTRAIL: &str = "cliptrail";
const MODULE_MARKLENS: &str = "marklens";
const MODULE_TEXTFORGE: &str = "textforge";
const EVENT_NAVIGATE: &str = "tongdock:navigate";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum ClipKind {
    #[default]
    Text,
    Image,
    Files,
}

#[derive(Clone, Serialize, Deserialize)]
struct ClipItem {
    id: u64,
    #[serde(default)]
    kind: ClipKind,
    content: String,
    #[serde(default)]
    image_data_url: Option<String>,
    #[serde(default)]
    image_width: Option<usize>,
    #[serde(default)]
    image_height: Option<usize>,
    #[serde(default)]
    image_format: Option<String>,
    #[serde(default)]
    byte_size: Option<u64>,
    #[serde(default)]
    file_paths: Vec<String>,
    pinned: bool,
    ts: u64,
}

#[derive(Clone, PartialEq, Eq)]
struct PendingClip {
    kind: ClipKind,
    content: String,
    image_data_url: Option<String>,
    image_width: Option<usize>,
    image_height: Option<usize>,
    image_format: Option<String>,
    byte_size: Option<u64>,
    file_paths: Vec<String>,
}

impl PendingClip {
    fn text(content: String) -> Self {
        Self {
            kind: ClipKind::Text,
            content,
            image_data_url: None,
            image_width: None,
            image_height: None,
            image_format: None,
            byte_size: None,
            file_paths: Vec::new(),
        }
    }

    fn image(
        content: String,
        image_data_url: String,
        width: usize,
        height: usize,
        image_format: String,
        byte_size: u64,
    ) -> Self {
        Self {
            kind: ClipKind::Image,
            content,
            image_data_url: Some(image_data_url),
            image_width: Some(width),
            image_height: Some(height),
            image_format: Some(image_format),
            byte_size: Some(byte_size),
            file_paths: Vec::new(),
        }
    }

    fn files(content: String, file_paths: Vec<String>, byte_size: u64) -> Self {
        Self {
            kind: ClipKind::Files,
            content,
            image_data_url: None,
            image_width: None,
            image_height: None,
            image_format: None,
            byte_size: Some(byte_size),
            file_paths,
        }
    }
}

impl From<&ClipItem> for PendingClip {
    fn from(value: &ClipItem) -> Self {
        Self {
            kind: value.kind.clone(),
            content: value.content.clone(),
            image_data_url: value.image_data_url.clone(),
            image_width: value.image_width,
            image_height: value.image_height,
            image_format: value.image_format.clone(),
            byte_size: value.byte_size,
            file_paths: value.file_paths.clone(),
        }
    }
}

#[derive(Default)]
struct History {
    items: VecDeque<ClipItem>,
    next_id: u64,
}

#[derive(Default)]
struct AppState {
    history: Mutex<History>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn store_file(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("history.json"))
}

fn load_history(app: &AppHandle) -> History {
    if let Some(path) = store_file(app) {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(items) = serde_json::from_str::<Vec<ClipItem>>(&data) {
                let next_id = items.iter().map(|item| item.id).max().map(|max| max + 1).unwrap_or(0);
                return History {
                    items: VecDeque::from(items),
                    next_id,
                };
            }
        }
    }

    History::default()
}

fn save_history(app: &AppHandle, history: &History) {
    if let Some(path) = store_file(app) {
        if let Ok(data) = serde_json::to_string(&history.items) {
            let _ = std::fs::write(path, data);
        }
    }
}

fn enforce_cap(history: &mut History) {
    while history.items.len() > MAX_ITEMS {
        match history.items.iter().rposition(|item| !item.pinned) {
            Some(index) => {
                history.items.remove(index);
            }
            None => break,
        }
    }
}

fn snapshot(history: &History) -> Vec<ClipItem> {
    history.items.iter().cloned().collect()
}

fn same_clip(item: &ClipItem, clip: &PendingClip) -> bool {
    item.kind == clip.kind
        && item.content == clip.content
        && item.image_data_url == clip.image_data_url
        && item.image_width == clip.image_width
        && item.image_height == clip.image_height
        && item.image_format == clip.image_format
        && item.byte_size == clip.byte_size
        && item.file_paths == clip.file_paths
}

    fn total_file_size(paths: &[PathBuf]) -> u64 {
        paths
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum()
    }

fn describe_file(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn describe_file_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| describe_file(path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn encode_image_data_url_and_size(image: &ImageData<'_>) -> Result<(String, u64), String> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            image.bytes.as_ref(),
            image.width as u32,
            image.height as u32,
            ColorType::Rgba8.into(),
        )
        .map_err(|error| error.to_string())?;

    let byte_size = png.len() as u64;
    Ok((format!("{IMAGE_DATA_URL_PREFIX}{}", BASE64.encode(png)), byte_size))
}

fn decode_image_data_url(data_url: &str) -> Result<ImageData<'static>, String> {
    let encoded = data_url
        .strip_prefix(IMAGE_DATA_URL_PREFIX)
        .ok_or_else(|| "image data is missing the PNG prefix".to_string())?;
    let png = BASE64.decode(encoded).map_err(|error| error.to_string())?;
    let rgba = image::load_from_memory_with_format(&png, ImageFormat::Png)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let (width, height) = rgba.dimensions();

    Ok(ImageData {
        width: width as usize,
        height: height as usize,
        bytes: Cow::Owned(rgba.into_raw()),
    })
}

fn capture_clipboard_item(clipboard: &mut arboard::Clipboard) -> Option<PendingClip> {
    if let Ok(paths) = clipboard.get().file_list() {
        if !paths.is_empty() {
            let total_size = total_file_size(&paths);
            return Some(PendingClip::files(
                describe_file_list(&paths),
                paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect(),
                total_size,
            ));
        }
    }

    if let Ok(image) = clipboard.get_image() {
        if let Ok((image_data_url, byte_size)) = encode_image_data_url_and_size(&image) {
            return Some(PendingClip::image(
                format!("Image {}x{}", image.width, image.height),
                image_data_url,
                image.width,
                image.height,
                "png".to_string(),
                byte_size,
            ));
        }
    }

    if let Ok(text) = clipboard.get_text() {
        if !text.is_empty() {
            return Some(PendingClip::text(text));
        }
    }

    None
}

fn record_clip(app: &AppHandle, clip: PendingClip) {
    if clip.content.is_empty() && clip.image_data_url.is_none() && clip.file_paths.is_empty() {
        return;
    }

    let state = app.state::<AppState>();
    let snapshot = {
        let mut history = state.history.lock().unwrap();

        let mut pinned = false;
        if let Some(index) = history.items.iter().position(|item| same_clip(item, &clip)) {
            if let Some(existing) = history.items.remove(index) {
                pinned = existing.pinned;
            }
        }

        let id = history.next_id;
        history.next_id += 1;
        history.items.push_front(ClipItem {
            id,
            kind: clip.kind,
            content: clip.content,
            image_data_url: clip.image_data_url,
            image_width: clip.image_width,
            image_height: clip.image_height,
            image_format: clip.image_format,
            byte_size: clip.byte_size,
            file_paths: clip.file_paths,
            pinned,
            ts: now_ms(),
        });
        enforce_cap(&mut history);
        save_history(app, &history);
        snapshot(&history)
    };

    let _ = app.emit("clipboard-updated", snapshot);
}

#[cfg(target_os = "macos")]
fn clipboard_change_count() -> Option<isize> {
    Some(NSPasteboard::generalPasteboard().changeCount() as isize)
}

#[cfg(not(target_os = "macos"))]
fn clipboard_change_count() -> Option<isize> {
    None
}

fn start_clipboard_watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(_) => return,
        };

        let mut last_seen = {
            let state = app.state::<AppState>();
            let history = state.history.lock().unwrap();
            history.items.front().map(PendingClip::from)
        };
        let mut last_change = clipboard_change_count();

        loop {
            if let Some(change_count) = clipboard_change_count() {
                if Some(change_count) == last_change {
                    std::thread::sleep(Duration::from_millis(POLL_MS));
                    continue;
                }
                last_change = Some(change_count);
            }

            match capture_clipboard_item(&mut clipboard) {
                Some(clip) if last_seen.as_ref() != Some(&clip) => {
                    last_seen = Some(clip.clone());
                    record_clip(&app, clip);
                }
                Some(clip) => {
                    last_seen = Some(clip);
                }
                None => {
                    last_seen = None;
                }
            }

            std::thread::sleep(Duration::from_millis(POLL_MS));
        }
    });
}

#[cfg(target_os = "macos")]
fn set_accessory_mode(app: &AppHandle) {
    let _ = app.set_activation_policy(ActivationPolicy::Accessory);
}

#[cfg(not(target_os = "macos"))]
fn set_accessory_mode(_app: &AppHandle) {}

#[cfg(target_os = "macos")]
fn set_regular_mode(app: &AppHandle) {
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
}

#[cfg(not(target_os = "macos"))]
fn set_regular_mode(_app: &AppHandle) {}

fn show_main_window(app: &AppHandle) {
    set_regular_mode(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    set_accessory_mode(app);
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            hide_main_window(app);
        } else {
            show_main_window(app);
        }
    }
}

fn show_module(app: &AppHandle, module_id: &str) {
    let _ = app.emit(EVENT_NAVIGATE, module_id.to_string());
    show_main_window(app);
}

#[tauri::command]
fn get_history(state: State<AppState>) -> Vec<ClipItem> {
    snapshot(&state.history.lock().unwrap())
}

#[tauri::command]
fn copy_item(app: AppHandle, id: u64) -> Result<(), String> {
    let item = {
        let state = app.state::<AppState>();
        let history = state.history.lock().unwrap();
        history.items.iter().find(|item| item.id == id).cloned()
    }
    .ok_or_else(|| "item not found".to_string())?;

    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    match item.kind {
        ClipKind::Text => clipboard
            .set_text(item.content)
            .map_err(|error| error.to_string())?,
        ClipKind::Image => {
            let data_url = item
                .image_data_url
                .as_deref()
                .ok_or_else(|| "image data missing".to_string())?;
            clipboard
                .set_image(decode_image_data_url(data_url)?)
                .map_err(|error| error.to_string())?;
        }
        ClipKind::Files => {
            let paths = item.file_paths.iter().map(PathBuf::from).collect::<Vec<_>>();
            if paths.is_empty() {
                return Err("file list missing".to_string());
            }
            clipboard
                .set()
                .file_list(&paths)
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

#[tauri::command]
fn delete_item(app: AppHandle, id: u64) -> Vec<ClipItem> {
    let state = app.state::<AppState>();
    let mut history = state.history.lock().unwrap();
    if let Some(index) = history.items.iter().position(|item| item.id == id) {
        history.items.remove(index);
    }
    save_history(&app, &history);
    snapshot(&history)
}

#[tauri::command]
fn toggle_pin(app: AppHandle, id: u64) -> Vec<ClipItem> {
    let state = app.state::<AppState>();
    let mut history = state.history.lock().unwrap();
    if let Some(item) = history.items.iter_mut().find(|item| item.id == id) {
        item.pinned = !item.pinned;
    }
    save_history(&app, &history);
    snapshot(&history)
}

#[tauri::command]
fn clear_history(app: AppHandle) -> Vec<ClipItem> {
    let state = app.state::<AppState>();
    let mut history = state.history.lock().unwrap();
    history.items.retain(|item| item.pinned);
    save_history(&app, &history);
    snapshot(&history)
}

#[tauri::command]
fn hide_window(window: tauri::Window) {
    hide_main_window(&window.app_handle());
}

fn set_px(buf: &mut [u8], size: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
        return;
    }

    let idx = ((y as u32 * size + x as u32) * 4) as usize;
    buf[idx] = color[0];
    buf[idx + 1] = color[1];
    buf[idx + 2] = color[2];
    buf[idx + 3] = color[3];
}

fn draw_line(
    buf: &mut [u8],
    size: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: [u8; 4],
) {
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        for oy in -thickness..=thickness {
            for ox in -thickness..=thickness {
                if ox * ox + oy * oy <= thickness * thickness {
                    set_px(buf, size, x + ox, y + oy, color);
                }
            }
        }

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn tray_icon_image() -> Image<'static> {
    let size = 22u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let white = [255, 255, 255, 255];

    // Keep menu bar icon as transparent background + white T.
    draw_line(&mut rgba, size, 5, 6, 17, 6, 1, white);
    draw_line(&mut rgba, size, 11, 6, 11, 18, 1, white);

    Image::new_owned(rgba, size, size)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let cliptrail = MenuItem::with_id(app, MODULE_CLIPTRAIL, "ClipTrail", true, None::<&str>)?;
    let marklens = MenuItem::with_id(app, MODULE_MARKLENS, "MarkLens", true, None::<&str>)?;
    let textforge = MenuItem::with_id(app, MODULE_TEXTFORGE, "TextForge", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit TongDock", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&cliptrail, &marklens, &textforge, &quit])?;

    TrayIconBuilder::with_id("tongdock-tray")
        .icon(tray_icon_image())
        .icon_as_template(false)
        .tooltip("TongDock")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MODULE_CLIPTRAIL => show_module(app, MODULE_CLIPTRAIL),
            MODULE_MARKLENS => show_module(app, MODULE_MARKLENS),
            MODULE_TEXTFORGE => show_module(app, MODULE_TEXTFORGE),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        toggle_main_window(app);
                    }
                })
                .build(),
        )
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_history,
            copy_item,
            delete_item,
            toggle_pin,
            clear_history,
            hide_window
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            {
                let loaded = load_history(&handle);
                *app.state::<AppState>().history.lock().unwrap() = loaded;
            }

            start_clipboard_watch(handle.clone());
            set_regular_mode(&handle);

            if let Err(error) = app.global_shortcut().register(TOGGLE_SHORTCUT) {
                eprintln!("TongDock: failed to register global shortcut: {error}");
            }

            build_tray(&handle)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                hide_main_window(&window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running TongDock application");
}
