use super::{Desktop, NativeDesktop, check_copy_size, clipboard::Content};
use image::{ImageFormat, RgbaImage};
use std::{ffi::OsStr, io::Cursor, os::windows::ffi::OsStrExt, path::Path, time::Duration};
use tokio::sync::watch;
use windows_sys::Win32::{
    System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    },
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

#[async_trait::async_trait]
impl Desktop for NativeDesktop {
    async fn copy(&self, text: &str) -> Result<(), String> {
        check_copy_size(text)?;
        let text = text.to_owned();
        tokio::time::timeout(
            Duration::from_secs(3),
            tokio::task::spawn_blocking(move || {
                arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.set_text(text))
                    .map_err(|_| "Clipboard unavailable; try copying again".to_owned())
            }),
        )
        .await
        .map_err(|_| "Clipboard request timed out")?
        .map_err(|_| "Clipboard worker stopped")?
    }

    async fn open(&self, url: &str) -> Result<(), String> {
        if !crate::message_actions::valid_web_link(url) {
            return Err("Only HTTP/HTTPS links can be opened".into());
        }
        open(OsStr::new(url)).await
    }

    async fn open_file(&self, path: &Path) -> Result<(), String> {
        if !path.is_absolute() || !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
            return Err("Downloaded file is unavailable".into());
        }
        open(path.as_os_str()).await
    }
}

async fn open(target: &OsStr) -> Result<(), String> {
    let target: Vec<u16> = target.encode_wide().chain(Some(0)).collect();
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || {
            if target[..target.len() - 1].contains(&0) {
                return Err("Cannot open a path or link containing NUL".to_owned());
            }
            // Shell verb handlers may use COM and require an STA apartment.
            // Balance every successful initialization, including S_FALSE.
            if unsafe {
                CoInitializeEx(
                    std::ptr::null(),
                    (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
                )
            } < 0
            {
                return Err("Cannot initialize the default application launcher".to_owned());
            }
            struct Apartment;
            impl Drop for Apartment {
                fn drop(&mut self) {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
            let _apartment = Apartment;
            let operation: Vec<u16> = "open\0".encode_utf16().collect();
            // SAFETY: Strings are NUL terminated; ShellExecute receives the literal
            // path/URL, with no command interpreter or argument interpolation.
            let result = unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    operation.as_ptr(),
                    target.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
            if result as isize <= 32 {
                Err("Cannot open this link or file; configure a default application".into())
            } else {
                Ok(())
            }
        }),
    )
    .await
    .map_err(|_| "Desktop request timed out")?
    .map_err(|_| "Desktop worker stopped")?
}

pub async fn read_clipboard(mut cancel: watch::Receiver<bool>) -> Result<Content, String> {
    if *cancel.borrow() {
        return Err("Clipboard read cancelled".into());
    }
    tokio::select! {
        _ = cancel.changed() => Err("Clipboard read cancelled".into()),
        result = tokio::time::timeout(Duration::from_secs(3), tokio::task::spawn_blocking(read)) => {
            result.map_err(|_| "Clipboard helper timed out")?
                .map_err(|_| "Clipboard worker stopped")?
        },
    }
}

fn read() -> Result<Content, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|_| "Clipboard unavailable; try pasting again")?;
    if let Ok(image) = clipboard.get_image() {
        if image.bytes.len() > 16 * 1024 * 1024 {
            return Err("Clipboard content exceeds the size limit".into());
        }
        let width = u32::try_from(image.width).map_err(|_| "Clipboard image is too large")?;
        let height = u32::try_from(image.height).map_err(|_| "Clipboard image is too large")?;
        let image = RgbaImage::from_raw(width, height, image.bytes.into_owned())
            .ok_or("Invalid clipboard image")?;
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .map_err(|_| "Cannot prepare clipboard image")?;
        return Ok(Content::Image {
            bytes: bytes.into_inner(),
            filename: "clipboard.png".into(),
        });
    }
    if let Ok(files) =
        clipboard_win::get_clipboard::<Vec<String>, _>(clipboard_win::formats::FileList)
    {
        if files.len() != 1 {
            return Err("Copy one image at a time".into());
        }
        let path = std::path::PathBuf::from(&files[0]);
        if !path.is_absolute() {
            return Err("Clipboard file must be a local image".into());
        }
        return Ok(Content::File(path));
    }
    let text = clipboard
        .get_text()
        .map_err(|_| "Clipboard has no supported image, copied file, or text")?;
    check_copy_size(&text)?;
    Ok(Content::Text(text))
}
