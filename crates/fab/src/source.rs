// SPDX-License-Identifier: GPL-3.0-or-later

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use egs_api::api::types::download_manifest::DownloadManifest;
use epic_auth::EpicSession;
use jobs::Reporter;

use crate::asset::{AssetVersion, LibraryAsset, from_fab};
use crate::download::plan::plan_from_manifest;
use crate::download::{self, DownloadedAsset, Fetch, HttpFetch, VersionDirs};
use crate::error::{Error, Result, from_egs};

/// A boxed, `Send` future, so [`AssetSource`] can be used as `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where Fab assets come from. [`EpicAssetSource`] in the app; fakes in tests.
///
/// Everything that touches Epic's unofficial API sits behind this trait, so
/// the backend can be replaced if Epic breaks it or publishes an official one.
pub trait AssetSource: Send + Sync {
    /// Lists every asset in the user's library (all pages).
    ///
    /// # Errors
    ///
    /// [`Error::Auth`] if the session expired, [`Error::Network`],
    /// [`Error::Http`], [`Error::ApiChanged`].
    fn list(&self) -> BoxFuture<'_, Result<Vec<LibraryAsset>>>;

    /// Downloads `version` of `asset` into
    /// `cache_dir/<asset>/<version>/files/`, resuming a previous attempt.
    ///
    /// Reports stage `"Downloading"` (fraction by bytes) and `"Assembling"`;
    /// honours pause and cancel between chunks and files.
    ///
    /// # Errors
    ///
    /// [`Error::Cancelled`], [`Error::FabBusy`], [`Error::NoDownload`],
    /// [`Error::ManifestCorrupt`], [`Error::ChunkCorrupt`],
    /// [`Error::UnsafePath`], [`Error::Io`], and the errors of
    /// [`AssetSource::list`].
    fn download<'a>(
        &'a self,
        asset: &'a LibraryAsset,
        version: &'a AssetVersion,
        cache_dir: &'a Path,
        reporter: &'a Reporter,
    ) -> BoxFuture<'a, Result<DownloadedAsset>>;
}

/// [`AssetSource`] backed by Epic's Fab API through `egs-api`.
#[derive(Debug, Clone)]
pub struct EpicAssetSource {
    session: EpicSession,
}

impl EpicAssetSource {
    /// Uses a signed-in session.
    #[must_use]
    pub fn new(session: EpicSession) -> Self {
        Self { session }
    }

    /// Fetches the asset manifest, downloads and parses the download manifest
    /// from the first distribution point that works, and checks its hash.
    async fn manifest(
        &self,
        asset: &LibraryAsset,
        version: &AssetVersion,
    ) -> Result<DownloadManifest> {
        let infos = self
            .session
            .egs()
            .fab_asset_manifest(&version.id, &asset.namespace, &asset.id, None)
            .await
            .map_err(|e| from_egs("asset manifest", e))?;
        let fetch = HttpFetch(self.session.http().clone());
        let mut last_error = None;
        for info in &infos {
            for base_url in &info.distribution_point_base_urls {
                let Some(point) = info.get_distribution_point_by_base_url(base_url) else {
                    continue;
                };
                let bytes = match fetch.fetch(&point.manifest_url).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        tracing::warn!(%error, "distribution point failed; trying the next one");
                        last_error = Some(error);
                        continue;
                    }
                };
                let mut manifest = parse_manifest(bytes).await?;
                let hash = manifest.custom_field("DownloadedManifestHash");
                if !info.manifest_hash.is_empty()
                    && hash.is_some_and(|hash| !hash.eq_ignore_ascii_case(&info.manifest_hash))
                {
                    tracing::warn!("manifest hash mismatch; trying the next distribution point");
                    last_error = Some(Error::ManifestCorrupt);
                    continue;
                }
                // Same as egs-api's fab_download_manifest: chunk links are
                // built from this field.
                manifest
                    .custom_fields
                    .get_or_insert_with(Default::default)
                    .insert("SourceURL".to_owned(), base_url.clone());
                return Ok(manifest);
            }
        }
        Err(last_error.unwrap_or_else(|| Error::NoDownload {
            title: asset.title.clone(),
        }))
    }
}

/// Parses a manifest off the async threads; `egs-api`'s binary parser can
/// panic on truncated data, which becomes [`Error::ApiChanged`] here.
async fn parse_manifest(bytes: Vec<u8>) -> Result<DownloadManifest> {
    tokio::task::spawn_blocking(move || DownloadManifest::parse(bytes))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| Error::api_changed("download manifest cannot be parsed"))
}

impl AssetSource for EpicAssetSource {
    fn list(&self) -> BoxFuture<'_, Result<Vec<LibraryAsset>>> {
        Box::pin(async move {
            self.session.ensure_fresh().await?;
            let mut egs = self.session.egs();
            // egs-api follows the `cursors.next` pages itself.
            let library = egs
                .try_fab_library_items(self.session.account_id().to_owned())
                .await
                .map_err(|e| from_egs("library", e))?;
            tracing::info!(assets = library.results.len(), "Fab library listed");
            Ok(library.results.iter().map(from_fab).collect())
        })
    }

    fn download<'a>(
        &'a self,
        asset: &'a LibraryAsset,
        version: &'a AssetVersion,
        cache_dir: &'a Path,
        reporter: &'a Reporter,
    ) -> BoxFuture<'a, Result<DownloadedAsset>> {
        Box::pin(async move {
            let dirs = VersionDirs::new(cache_dir, &asset.id, &version.id);
            if let Some(done) = download::completed(&dirs, asset.kind).await {
                return Ok(done);
            }
            reporter.stage("Preparing");
            self.session.ensure_fresh().await?;
            let manifest = self.manifest(asset, version).await?;
            let plan = plan_from_manifest(&manifest)?;
            let fetch = HttpFetch(self.session.http().clone());
            download::run(plan, dirs, &fetch, reporter, asset.kind).await
        })
    }
}
