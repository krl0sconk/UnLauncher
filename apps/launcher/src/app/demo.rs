// SPDX-License-Identifier: GPL-3.0-or-later

//! The hidden `--demo` flag: a made-up Fab library, some tasks and a
//! signed-in name, to take screenshots and to measure the interface with 100
//! assets loaded (the real library needs an Epic account). Nothing here is
//! used in normal runs.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "made-up pixels and counts"
)]

use std::io::Cursor;
use std::path::Path;

use fab::{AssetKind, AssetVersion, LibraryAsset};
use jobs::{Reporter, Task, TaskError, TaskFuture, TaskOutcome};

#[allow(clippy::wildcard_imports, reason = "a part of the app module")]
use super::*;
use crate::images::{ImageKey, cache_file};

/// How many assets the demo library has.
pub const ASSET_COUNT: usize = 100;
/// Size of the made-up thumbnails.
const THUMB: (u32, u32) = (320, 180);

const TITLES: [&str; 12] = [
    "City Sample Buildings",
    "Advanced Locomotion",
    "Stylized Water",
    "Modular Sci-Fi Corridor",
    "Weather System Pro",
    "Medieval Props",
    "Forest Biome Kit",
    "Desert Ruins",
    "Cyber Alley",
    "Snow Peaks",
    "Fantasy Village",
    "Space Station Interiors",
];

/// The made-up library: `count` assets of every kind, with thumbnails that
/// [`seed_thumbnails`] puts in the disk cache.
pub fn assets(count: usize) -> Vec<LibraryAsset> {
    (0..count)
        .map(|n| {
            let kind = match n % 4 {
                1 => AssetKind::Plugin,
                3 => AssetKind::Project,
                _ => AssetKind::Content,
            };
            let first = (n % 6) as u32;
            LibraryAsset {
                id: format!("demo-{n}"),
                namespace: "demo".to_owned(),
                title: format!("{} {}", TITLES[n % TITLES.len()], n / TITLES.len() + 1),
                kind,
                versions: vec![AssetVersion {
                    id: format!("demo-{n}-v"),
                    engine_versions: (first..=7).map(|minor| (5, minor)).collect(),
                    title: None,
                }],
                thumbnail_url: Some(thumbnail_url(n)),
            }
        })
        .collect()
}

fn thumbnail_url(n: usize) -> String {
    format!("https://demo.invalid/thumbnails/{n}.png")
}

/// Writes a made-up picture into the cache for every asset, where the image
/// cache finds it without going to the network.
pub fn seed_thumbnails(cache_dir: &Path, count: usize) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    for n in 0..count {
        let file = cache_file(cache_dir, &thumbnail_url(n));
        if !file.exists() {
            std::fs::write(file, scene(n))?;
        }
    }
    Ok(())
}

/// A PNG of sky, sun and hills in colors that depend on `seed`.
fn scene(seed: usize) -> Vec<u8> {
    let (width, height) = THUMB;
    let mut state = (seed as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut next = move |limit: u32| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % u64::from(limit)) as u32
    };
    let palettes: [([u32; 3], [u32; 3]); 5] = [
        ([70, 130, 200], [40, 110, 50]),
        ([230, 150, 90], [150, 90, 50]),
        ([170, 190, 215], [225, 230, 240]),
        ([25, 30, 60], [30, 45, 70]),
        ([110, 60, 140], [40, 90, 100]),
    ];
    let (sky, hill) = palettes[seed % palettes.len()];
    let (sun_x, sun_y) = (40 + next(width - 80), 18 + next(40));
    let phase = f64::from(next(628)) / 100.0;
    let picture = image::RgbaImage::from_fn(width, height, |x, y| {
        let (fx, fy) = (f64::from(x), f64::from(y));
        let ground =
            120.0 + 22.0 * (fx / 40.0 + phase).sin() + 10.0 * (fx / 13.0 + 2.0 * phase).sin();
        let pixel = if (fx - f64::from(sun_x)).powi(2) + (fy - f64::from(sun_y)).powi(2)
            < 18.0_f64.powi(2)
        {
            [250, 225, 150]
        } else if fy > ground {
            let shade = 1.0 - 0.35 * ((fy - ground) / 60.0).min(1.0);
            hill.map(|c| (f64::from(c) * shade) as u8)
        } else {
            let light = 0.75 + 0.35 * fy / ground;
            sky.map(|c| (f64::from(c) * light).min(255.0) as u8)
        };
        image::Rgba([pixel[0], pixel[1], pixel[2], 255])
    });
    let mut bytes = Vec::new();
    // Writing to memory cannot fail.
    let _ = picture.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png);
    bytes
}

/// What a made-up task does.
#[derive(Debug, Clone, Copy)]
enum Demo {
    /// Runs forever at this fraction.
    Running(f32),
    /// Finishes at once.
    Done,
    /// Fails at once.
    Fails,
}

struct DemoTask {
    kind: &'static str,
    title: &'static str,
    step: &'static str,
    demo: Demo,
}

impl Task for DemoTask {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn title(&self) -> String {
        self.title.to_owned()
    }

    fn spec(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn pausable(&self) -> bool {
        true
    }

    fn run(self: Box<Self>, reporter: Reporter) -> TaskFuture {
        Box::pin(async move {
            reporter.stage("Working");
            reporter.message(self.step);
            match self.demo {
                Demo::Running(fraction) => {
                    reporter.fraction(Some(fraction));
                    loop {
                        reporter
                            .checkpoint()
                            .await
                            .map_err(|_| TaskError::Cancelled)?;
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
                Demo::Done => {
                    reporter.fraction(Some(1.0));
                    Ok(TaskOutcome::new(self.step))
                }
                Demo::Fails => {
                    reporter.fraction(Some(0.21));
                    Err(TaskError::failed(
                        self.step,
                        Some("Free some disk space and try again.".to_owned()),
                    ))
                }
            }
        })
    }
}

impl LauncherApp {
    /// Fills the app with made-up data (see the module docs).
    pub fn enable_demo(&mut self) {
        if let Err(error) = seed_thumbnails(&self.env.thumbnails_dir, ASSET_COUNT) {
            tracing::warn!(%error, "cannot write the demo thumbnails");
        }
        let assets = assets(ASSET_COUNT);
        // As if the user had scrolled through the whole library.
        for asset in &assets {
            if let Some(url) = &asset.thumbnail_url {
                let _ = self.images.texture(&ImageKey::Remote(url.clone()));
            }
        }
        self.library.epic = EpicState::Demo("krlos".to_owned());
        self.library.assets = Some(Ok(assets));
        self.github.saved = Some(true);

        let _guard = self.runtime.enter();
        for (kind, title, step, demo) in [
            (
                "fab-install",
                "Add City Sample Buildings to KartRacer",
                "2.3 GB installed",
                Demo::Done,
            ),
            (
                "build-engine",
                "Build Unreal Engine 5.6",
                "not enough space: the build needs 300 GB and the disk has 112 GB free",
                Demo::Fails,
            ),
            (
                "download-engine",
                "Download Unreal Engine 5.7.4",
                "Linux_Unreal_Engine_5.7.4.zip",
                Demo::Running(0.25),
            ),
            (
                "build-engine",
                "Build Unreal Engine 5.7 from source",
                "Compiling Module.Renderer.12.cpp",
                Demo::Running(0.31),
            ),
        ] {
            let id = self.tasks.submit(Box::new(DemoTask {
                kind,
                title,
                step,
                demo,
            }));
            if title.starts_with("Download") {
                // Give it a moment to start, then pause it.
                let tasks = self.tasks.clone();
                self.runtime.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    let _ = tasks.pause(id);
                });
            }
        }
    }
}

impl LauncherApp {
    /// Opens one of the dialogs, for screenshots (`--demo-dialog`).
    pub fn open_demo_dialog(&mut self, name: &str) {
        match name {
            "install" => {
                self.handle(Intent::InstallEngine);
                self.handle(Intent::SetInstallMode(InstallMode::Source));
            }
            "install-prebuilt" => self.handle(Intent::InstallEngine),
            "new-project" => self.handle(Intent::NewProject),
            "asset" => {
                if let Some(Ok(assets)) = &self.library.assets
                    && let Some(asset) = assets.first().cloned()
                {
                    self.handle(Intent::InstallAsset(asset));
                }
            }
            "uninstall" => {
                if let Ok(id) = EngineId::new("UE_5.6") {
                    self.handle(Intent::AskTyped {
                        title: "Uninstall Unreal Engine 5.6?".to_owned(),
                        body: "Its folder at ~/UnrealEngine/UE_5.6 is deleted and the engine leaves the list. Projects that use it stay where they are.".to_owned(),
                        expected: "UE_5.6".to_owned(),
                        action: Action::Uninstall(id),
                    });
                }
            }
            other => tracing::warn!(name = other, "unknown demo dialog"),
        }
    }
}
