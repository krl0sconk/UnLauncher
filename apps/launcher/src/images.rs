// SPDX-License-Identifier: GPL-3.0-or-later

//! Pictures for the interface: project thumbnails from disk and Fab
//! thumbnails from the network.
//!
//! The interface asks [`Images::texture`] for a picture and draws a
//! placeholder until it returns one. Reading, downloading, decoding and
//! shrinking (to about 512 px) happen on worker threads; the interface thread
//! only uploads finished pixels, a few per frame. Fab pictures are cached on
//! disk (bounded in size) and textures are dropped by age past a limit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};

use egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions, Vec2};
use tokio::runtime::Handle;
use tokio::sync::Semaphore;

mod disk;
pub mod placeholder;

pub use disk::cache_file;

/// Pictures are shrunk to fit this many pixels on their longest side.
const MAX_SIDE: u32 = 512;
/// Textures kept in memory; the least recently drawn go first.
const MAX_TEXTURES: usize = 120;
/// Textures uploaded per frame, so a burst of finished pictures does not
/// stall one frame.
const UPLOADS_PER_FRAME: usize = 4;
/// Pictures read or downloaded and decoded at the same time.
const CONCURRENT_JOBS: usize = 4;
/// The disk cache of downloaded pictures stays under this many bytes.
const DISK_LIMIT: u64 = 200 * 1024 * 1024;
/// A download bigger than this is not a thumbnail.
const MAX_DOWNLOAD: usize = 8 * 1024 * 1024;

/// Which picture.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageKey {
    /// An image file on this computer.
    File(PathBuf),
    /// An `https` URL, cached on disk.
    Remote(String),
}

/// A texture ready to draw.
#[derive(Debug, Clone, Copy)]
pub struct Picture {
    /// The texture.
    pub id: TextureId,
    /// Its size in pixels.
    pub size: Vec2,
}

enum Slot {
    Loading,
    /// It could not be read or decoded; the placeholder stays.
    Failed,
    Ready {
        texture: TextureHandle,
        used: u64,
    },
}

#[derive(Default)]
struct State {
    slots: HashMap<ImageKey, Slot>,
    /// Counts frames, to find the least recently drawn texture.
    frame: u64,
}

struct Loaded {
    key: ImageKey,
    image: Option<ColorImage>,
}

/// The UnLauncher mark, shown in the sidebar (decoded once at startup from
/// bytes in the binary).
const BRAND_MARK: &[u8] = include_bytes!("../assets/icon-256.png");
/// Pixels of the decoded mark: it is shown at 28 points.
const BRAND_MARK_SIDE: u32 = 128;

struct Inner {
    brand: Option<TextureHandle>,
    state: Mutex<State>,
    results: Mutex<Receiver<Loaded>>,
    tx: Sender<Loaded>,
    runtime: Handle,
    http: reqwest::Client,
    cache_dir: PathBuf,
    jobs: Arc<Semaphore>,
    ctx: Context,
}

/// A cheap handle to the picture cache; clones share it.
#[derive(Clone)]
pub struct Images(Arc<Inner>);

impl Images {
    /// A cache that downloads with `http` into `cache_dir` and uses
    /// `runtime` for its jobs.
    pub fn new(ctx: &Context, runtime: Handle, http: reqwest::Client, cache_dir: PathBuf) -> Self {
        let (tx, rx) = channel();
        let brand = disk::decode(BRAND_MARK, BRAND_MARK_SIDE)
            .map_err(|error| tracing::warn!(%error, "cannot decode the UnLauncher mark"))
            .ok()
            .map(|image| ctx.load_texture("unlauncher-mark", image, TextureOptions::LINEAR));
        let images = Self(Arc::new(Inner {
            brand,
            state: Mutex::new(State::default()),
            results: Mutex::new(rx),
            tx,
            runtime,
            http,
            cache_dir,
            jobs: Arc::new(Semaphore::new(CONCURRENT_JOBS)),
            ctx: ctx.clone(),
        }));
        images.prune_disk();
        images
    }

    /// The UnLauncher mark.
    pub fn brand_mark(&self) -> Option<Picture> {
        self.0.brand.as_ref().map(|texture| Picture {
            id: texture.id(),
            size: texture.size_vec2(),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // A poisoned lock only means a drawing thread panicked; the map is
        // still usable.
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Call once per frame, before drawing: uploads finished pictures and
    /// drops the oldest textures past the limit.
    pub fn begin_frame(&self, ctx: &Context) {
        let mut state = self.state();
        state.frame += 1;
        let mut uploaded = 0;
        let mut more = false;
        {
            let results = self
                .0
                .results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while uploaded < UPLOADS_PER_FRAME {
                let Ok(loaded) = results.try_recv() else {
                    break;
                };
                let slot = match loaded.image {
                    Some(image) => {
                        uploaded += 1;
                        let name = match &loaded.key {
                            ImageKey::File(path) => path.display().to_string(),
                            ImageKey::Remote(url) => url.clone(),
                        };
                        Slot::Ready {
                            texture: ctx.load_texture(name, image, TextureOptions::LINEAR),
                            used: state.frame,
                        }
                    }
                    None => Slot::Failed,
                };
                state.slots.insert(loaded.key, slot);
            }
            // `try_recv` has nothing more once the queue is empty.
            if uploaded == UPLOADS_PER_FRAME {
                more = true;
            }
        }
        if more {
            ctx.request_repaint();
        }
        evict_oldest(&mut state.slots);
    }

    /// The texture for `key`, if it is ready. The first call for a key
    /// starts loading it in the background.
    pub fn texture(&self, key: &ImageKey) -> Option<Picture> {
        let mut state = self.state();
        let frame = state.frame;
        match state.slots.get_mut(key) {
            Some(Slot::Ready { texture, used }) => {
                *used = frame;
                Some(Picture {
                    id: texture.id(),
                    size: texture.size_vec2(),
                })
            }
            Some(Slot::Loading | Slot::Failed) => None,
            None => {
                state.slots.insert(key.clone(), Slot::Loading);
                drop(state);
                self.load(key.clone());
                None
            }
        }
    }

    /// Forgets pictures read from files, so a new project thumbnail or
    /// screenshot is picked up the next time it is drawn.
    pub fn forget_files(&self) {
        self.state()
            .slots
            .retain(|key, _| !matches!(key, ImageKey::File(_)));
    }

    fn load(&self, key: ImageKey) {
        let inner = Arc::clone(&self.0);
        self.0.runtime.spawn(async move {
            let image = match inner.jobs.clone().acquire_owned().await {
                Ok(_permit) => read(&inner, &key).await,
                Err(_) => None,
            };
            // The receiver only goes away when the app is closing.
            let _ = inner.tx.send(Loaded { key, image });
            inner.ctx.request_repaint();
        });
    }

    /// Removes the oldest cached downloads past the size limit.
    fn prune_disk(&self) {
        let dir = self.0.cache_dir.clone();
        self.0.runtime.spawn_blocking(move || {
            if let Err(error) = disk::prune(&dir, DISK_LIMIT) {
                tracing::warn!(dir = %dir.display(), %error, "cannot trim the thumbnail cache");
            }
        });
    }
}

/// Drops the least recently drawn textures beyond [`MAX_TEXTURES`].
fn evict_oldest(slots: &mut HashMap<ImageKey, Slot>) {
    let mut ready: Vec<(u64, ImageKey)> = slots
        .iter()
        .filter_map(|(key, slot)| match slot {
            Slot::Ready { used, .. } => Some((*used, key.clone())),
            _ => None,
        })
        .collect();
    if ready.len() <= MAX_TEXTURES {
        return;
    }
    ready.sort_by_key(|(used, _)| *used);
    let excess = ready.len() - MAX_TEXTURES;
    for (_, key) in ready.into_iter().take(excess) {
        slots.remove(&key);
    }
}

/// Gets the bytes of `key` (reading the file or downloading to the cache),
/// then decodes them on a blocking thread. `None` if anything fails.
async fn read(inner: &Inner, key: &ImageKey) -> Option<ColorImage> {
    let bytes = match key {
        ImageKey::File(path) => match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "cannot read a thumbnail");
                return None;
            }
        },
        ImageKey::Remote(url) => {
            let file = disk::cache_file(&inner.cache_dir, url);
            match cached_download(&inner.http, url, &file).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::debug!(%url, %error, "cannot download a thumbnail");
                    return None;
                }
            }
        }
    };
    let decoded = tokio::task::spawn_blocking(move || disk::decode(&bytes, MAX_SIDE)).await;
    match decoded {
        Ok(Ok(image)) => Some(image),
        Ok(Err(error)) => {
            tracing::debug!(?key, %error, "cannot decode a thumbnail");
            None
        }
        Err(_) => None,
    }
}

/// The picture at `url`: from `file` if cached, else downloaded and stored
/// there (temporary file, then rename).
async fn cached_download(
    http: &reqwest::Client,
    url: &str,
    file: &Path,
) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = tokio::fs::read(file).await {
        return Ok(bytes);
    }
    if !url.starts_with("https://") {
        return Err("not an https URL".to_owned());
    }
    let response = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.without_url().to_string())?;
    if response
        .content_length()
        .is_some_and(|len| len > MAX_DOWNLOAD as u64)
    {
        return Err("the picture is too big".to_owned());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| error.without_url().to_string())?;
    if bytes.len() > MAX_DOWNLOAD {
        return Err("the picture is too big".to_owned());
    }
    // Keep it only if it decodes: a captcha page must not fill the cache.
    let probe = bytes.clone();
    let decodes = tokio::task::spawn_blocking(move || image::guess_format(&probe).is_ok())
        .await
        .unwrap_or(false);
    if !decodes {
        return Err("not a picture".to_owned());
    }
    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|error| error.to_string())?;
    }
    let tmp = file.with_extension("part");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::rename(&tmp, file)
        .await
        .map_err(|error| error.to_string())?;
    Ok(bytes.to_vec())
}
