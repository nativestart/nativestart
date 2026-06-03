use std::fs;
use std::fs::File;

use log::*;
use progress_streams::ProgressReader;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tar::Archive;

use crate::descriptor::ApplicationComponent;
use crate::errors::*;
use crate::installation_manager::InstallationManager;
use crate::recompress::recompress;
use crate::UserInterface;

pub struct DownloadManager {}

impl DownloadManager {
    pub fn new() -> DownloadManager {
        return DownloadManager {};
    }

    /// Try to download the content from a specified URL
    pub fn download_and_get(&self, url: &str) -> Option<String> {
        let answer = attohttpc::get(url).send().ok()?;

        if answer.is_success() {
            return Some(answer.text().ok()?);
        } else {
            return Option::None;
        }
    }

    pub fn download_and_store(&self, components: &Vec<ApplicationComponent>, installation: &InstallationManager, ui: &UserInterface) -> Result<()> {
        let mut downloaded: u64 = 0;
        let total_size: u64 = components.iter().map(|ref component| component.download_size.unwrap_or(component.size)).sum();
        info!("Downloading {} components ({} bytes)", components.len(), total_size);
        for component in components {
            let path = installation.path_for_write(&component)?;

            debug!("Downloading {} to {:?}", component.url, path);

            // prepare HTTP client
            let res = attohttpc::get(&component.url).send()
                .chain_err(|| ErrorKind::DownloadError(format!("Could not download file {:?}", &component.url)))?;

            // decorate reader with progress tracking
            let file_progress = Arc::new(AtomicUsize::new(0));
            let mut reader = ProgressReader::new(res, |progress: usize| {
                file_progress.fetch_add(progress, Ordering::SeqCst);
                ui.set_download_progress((downloaded + file_progress.load(Ordering::SeqCst) as u64) as f64 / total_size as f64);
            });

            if component.is_archive() {
                // create empty directory
                fs::create_dir_all(&path)
                    .chain_err(|| ErrorKind::StorageError(format!("Could not create directory {:?}", &path)))?;

                // extract data stream to target location
                let stream = zstd::Decoder::new(reader)?;
                let mut archive = Archive::new(stream);
                archive.unpack(&path)
                    .chain_err(|| ErrorKind::StorageError(format!("Could not unpack compressed file {:?}", &path)))?;
            } else {
                // create parent directories if needed
                path.parent().and_then(|parent| fs::create_dir_all(parent).ok());
                let mut file = File::create(&path)
                    .chain_err(|| ErrorKind::StorageError(format!("Could not create file {:?}", &path)))?;

                // special handling for zstd-compressed JAR files
                if component.url.ends_with(".jar.zstd") && path.to_str().unwrap().ends_with(".jar") {
                    let mut stream = zstd::Decoder::new(reader)?;
                    recompress(&mut stream, &mut file).unwrap();
                } else {
                    io::copy(&mut reader, &mut file).chain_err(|| ErrorKind::DownloadError(format!("Error during download")))?;
                }
            }

            // re-create cache directory if there is one
            match &component.cache_path {
                Some(cache_path) => installation.recreate_dir(cache_path)?,
                None => {}
            }

            downloaded += component.download_size.unwrap_or(component.size);
            ui.set_download_progress(downloaded as f64 / total_size as f64);
        }

        ui.download_done();
        return Ok(());
    }
}