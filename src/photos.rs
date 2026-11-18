//! People's photos: fetched at the size they are drawn, kept on disk between launches, and
//! decoded off the thread that draws.
//!
//! Photos are fetched without the session's cookies, as Google's web client fetches them,
//! and only from Google's image hosts.

use std::{
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Duration,
};

use gpui::{App, Asset, RenderImage};
use image::{Frame, imageops::FilterType};
use tokio::sync::Semaphore;

use crate::network;

/// Hosts Google serves photos from.
const HOSTS: [&str; 2] = [".googleusercontent.com", ".ggpht.com"];
/// Photos fetched at once.
const AT_ONCE: usize = 6;
/// The largest reply taken for a photo. One at the sizes asked for is a few kilobytes.
const LARGEST: u64 = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
/// How long a photo kept on disk is used before it is fetched again.
const KEPT_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A person's photo at a size, for `Window::use_asset`. `None` when it cannot be had.
pub struct Photo;

impl Asset for Photo {
    /// The photo's address as Google gives it, and how many pixels across to draw it.
    type Source = (gpui::SharedString, u32);
    type Output = Option<Arc<RenderImage>>;

    fn load(
        (photo, pixels): Self::Source,
        _cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        load(photo.to_string(), pixels)
    }
}

/// Loads the photo at `photo`, `pixels` across, from disk or from Google.
pub async fn load(photo: String, pixels: u32) -> Option<Arc<RenderImage>> {
    let address = sized(&photo, pixels)?;
    let loader = loader()?;
    let task = network::runtime().ok()?.spawn(async move {
        let kept = loader
            .folder
            .as_ref()
            .map(|folder| folder.join(file_name(&address)));
        let from_disk = {
            let kept = kept.clone();
            tokio::task::spawn_blocking(move || read_kept(&kept?))
                .await
                .ok()
                .flatten()
        };
        let bytes = match from_disk {
            Some(bytes) => bytes,
            None => {
                let bytes = loader.fetch(&address).await?;
                if let Some(kept) = kept {
                    let bytes = bytes.clone();
                    tokio::task::spawn_blocking(move || keep(&kept, &bytes));
                }
                bytes
            }
        };
        tokio::task::spawn_blocking(move || decode(&bytes, pixels))
            .await
            .ok()
            .flatten()
    });
    task.await.ok().flatten()
}

/// Deletes the photos kept on disk, when the person signs out.
pub fn forget() {
    if let Some(folder) = folder() {
        // Nothing kept is nothing to delete.
        fs::remove_dir_all(folder).ok();
    }
}

/// Marks the picture Google gives everyone who has not chosen a photo, in its older form.
const NO_PHOTO: &str = "/AAAAAAAAAAI/AAAAAAAAAAA/";
/// Where the newer form of that picture lives: a letter on a colour, made by Google.
const NO_PHOTO_FOLDER: &str = "a/";

/// The address to fetch `photo` from at `pixels` across, cropped square. `None` unless it is
/// on one of Google's image hosts over https, and for the picture Google gives people who
/// have not chosen one: Peep draws their initials instead.
fn sized(photo: &str, pixels: u32) -> Option<String> {
    let address = match photo.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => photo.to_owned(),
    };
    let rest = address.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    let host_ok = !host.contains(['@', ':'])
        && HOSTS
            .iter()
            .any(|suffix| host.to_ascii_lowercase().ends_with(suffix));
    if !host_ok || path.is_empty() || path.contains(['?', '#']) {
        return None;
    }
    if path.starts_with(NO_PHOTO_FOLDER) || rest.contains(NO_PHOTO) {
        return None;
    }
    let (folders, last) = match path.rsplit_once('/') {
        Some((folders, last)) => (Some(folders), last),
        None => (None, path),
    };
    let size = format!("s{pixels}-c");
    let path = match (last.split_once('='), folders) {
        // The newer form carries its options after an equals sign.
        (Some((name, _)), Some(folders)) => format!("{folders}/{name}={size}"),
        (Some((name, _)), None) => format!("{name}={size}"),
        // The older form ends in a file name and takes them as a folder before it.
        (None, Some(folders)) if last.contains('.') => format!("{folders}/{size}/{last}"),
        (None, _) => format!("{path}={size}"),
    };
    Some(format!("https://{host}/{path}"))
}

struct Loader {
    http: reqwest::Client,
    limit: Semaphore,
    folder: Option<PathBuf>,
}

fn loader() -> Option<&'static Loader> {
    static LOADER: OnceLock<Option<Loader>> = OnceLock::new();
    LOADER
        .get_or_init(|| {
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(TIMEOUT)
                .build()
                .ok()?;
            Some(Loader {
                http,
                limit: Semaphore::new(AT_ONCE),
                folder: folder(),
            })
        })
        .as_ref()
}

impl Loader {
    async fn fetch(&self, address: &str) -> Option<Vec<u8>> {
        let _turn = self.limit.acquire().await.ok()?;
        let response = self.http.get(address).send().await.ok()?;
        if response.status().as_u16() != 200
            || response
                .content_length()
                .is_some_and(|length| length > LARGEST)
        {
            return None;
        }
        let bytes = response.bytes().await.ok()?;
        (bytes.len() as u64 <= LARGEST).then(|| bytes.to_vec())
    }
}

/// Where photos are kept: with the Mac's other caches, which it may clear.
fn folder() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Caches/Peep/Photos"))
}

/// The name a photo is kept under: its address is not fit to be one.
fn file_name(address: &str) -> String {
    sha1_smol::Sha1::from(address).digest().to_string()
}

/// A photo kept on disk, unless it has been there too long.
fn read_kept(path: &Path) -> Option<Vec<u8>> {
    let age = fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()?;
    (age < KEPT_FOR).then(|| fs::read(path).ok())?
}

fn keep(path: &Path, bytes: &[u8]) {
    let Some(folder) = path.parent() else { return };
    // A photo that cannot be kept is fetched again next time.
    if fs::create_dir_all(folder).is_ok() {
        fs::write(path, bytes).ok();
    }
}

/// Decodes a photo and fits it to a square `pixels` across, in the pixel order GPUI draws.
fn decode(bytes: &[u8], pixels: u32) -> Option<Arc<RenderImage>> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if (image.width(), image.height()) == (pixels, pixels) {
        image
    } else {
        image.resize_to_fill(pixels, pixels, FilterType::Lanczos3)
    };
    let mut pixels = image.into_rgba8();
    // GPUI wants blue first.
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([Frame::new(pixels)])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_photo_is_asked_for_at_the_size_it_is_drawn() {
        let host = "https://lh3.googleusercontent.com";
        assert_eq!(
            sized("//lh3.googleusercontent.com/a-/abc=s96-c", 64).unwrap(),
            format!("{host}/a-/abc=s64-c"),
            "options after an equals sign are replaced"
        );
        assert_eq!(
            sized(&format!("{host}/-x/y/photo.jpg"), 64).unwrap(),
            format!("{host}/-x/y/s64-c/photo.jpg"),
            "the older form takes its options as a folder"
        );
        assert_eq!(
            sized("https://yt3.ggpht.com/abc", 40).unwrap(),
            "https://yt3.ggpht.com/abc=s40-c"
        );
    }

    #[test]
    fn the_picture_google_gives_everyone_without_a_photo_is_not_fetched() {
        let host = "https://lh3.googleusercontent.com";
        assert_eq!(sized(&format!("{host}/a/abc=s96-c"), 64), None);
        assert_eq!(
            sized(
                &format!("{host}/-x/AAAAAAAAAAI/AAAAAAAAAAA/y/photo.jpg"),
                64
            ),
            None
        );
    }

    #[test]
    fn only_googles_image_hosts_are_asked() {
        for photo in [
            "",
            "http://lh3.googleusercontent.com/a-/abc",
            "https://example.com/a-/abc",
            "https://googleusercontent.com.example.com/a-/abc",
            "https://lh3.googleusercontent.com@example.com/a-/abc",
            "https://lh3.googleusercontent.com/",
            "https://lh3.googleusercontent.com/a-/abc?x=1",
        ] {
            assert_eq!(sized(photo, 64), None, "{photo}");
        }
    }

    #[test]
    fn a_photo_is_fitted_to_a_square_with_blue_first() {
        let mut source = image::RgbaImage::new(4, 2);
        for pixel in source.pixels_mut() {
            *pixel = image::Rgba([255, 128, 0, 255]);
        }
        let mut png = Vec::new();
        source
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let photo = decode(&png, 2).unwrap();

        let bytes = photo.as_bytes(0).unwrap();
        assert_eq!(bytes.len(), 2 * 2 * 4, "two by two");
        assert_eq!(bytes[..4], [0, 128, 255, 255]);
        assert!(decode(b"not an image", 2).is_none());
    }

    #[test]
    fn a_kept_photo_is_named_by_its_address_and_used_until_it_is_old() {
        assert_eq!(file_name("a"), "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8");

        let folder = std::env::temp_dir().join(format!("peep-photos-{}", std::process::id()));
        let path = folder.join(file_name("a"));
        assert_eq!(read_kept(&path), None, "nothing kept yet");
        keep(&path, b"bytes");
        assert_eq!(read_kept(&path).as_deref(), Some(&b"bytes"[..]));

        let old = std::time::SystemTime::now() - KEPT_FOR - Duration::from_secs(1);
        fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(old))
            .unwrap();
        assert_eq!(read_kept(&path), None, "too old to use");
        fs::remove_dir_all(&folder).unwrap();
    }
}
