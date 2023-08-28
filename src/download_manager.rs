use std::fs;
use std::fs::File;

use log::*;
use progress_streams::ProgressReader;
use std::io;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tar::Archive;

use crate::errors::*;
use crate::descriptor::ApplicationArtifact;
use crate::UserInterface;
use crate::installation_manager::InstallationManager;

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

    pub fn download_and_store(&self, artifacts: &Vec<ApplicationArtifact>, installation: &InstallationManager, ui: &UserInterface) -> Result<()> {
        let mut downloaded: u64 = 0;
        let total_size: u64 = artifacts.iter().map(|ref artifact| artifact.download_size.unwrap_or(artifact.size)).sum();
        info!("Downloading {} artifacts ({} bytes)", artifacts.len(), total_size);
        for artifact in artifacts {
            let path = installation.path_for_write(&artifact)?;

            debug!("Downloading {} to {:?}", artifact.url, path);

            if artifact.is_archive() {
                // create empty directory
                fs::create_dir_all(&path)
                    .chain_err(|| ErrorKind::StorageError(format!("Could not create directory {:?}", &path)))?;

                // prepare HTTP client
                let res = attohttpc::get(&artifact.url).send()
                    .chain_err(|| ErrorKind::DownloadError(format!("Could not download file {:?}", &artifact.url)))?;

                // decorate reader with progress tracking
                let file_progress = Arc::new(AtomicUsize::new(0));
                let reader = ProgressReader::new(res, |progress: usize| {
                    file_progress.fetch_add(progress, Ordering::SeqCst);
                    ui.set_download_progress((downloaded + file_progress.load(Ordering::SeqCst) as u64) as f64 / total_size as f64);
                });

                // extract data stream to target location
                let stream = xz2::read::XzDecoder::new(reader);
                let mut archive = Archive::new(stream);
                archive.unpack(&path)
                    .chain_err(|| ErrorKind::StorageError(format!("Could not unpack compressed file {:?}", &path)))?;
            } else {
                // create parent directories if needed
                path.parent().and_then(|parent| fs::create_dir_all(parent).ok());

                // download to correct location
                let mut file = File::create(&path)
                    .chain_err(|| ErrorKind::StorageError(format!("Could not create file {:?}", &path)))?;

                let mut res = attohttpc::get(&artifact.url).send()
                    .chain_err(|| ErrorKind::DownloadError(format!("Could not download file {:?}", &artifact.url)))?;
                self.download(&mut res, &mut file, ui, downloaded, total_size)?;
            }

            downloaded += artifact.download_size.unwrap_or(artifact.size);
            ui.set_download_progress(downloaded as f64 / total_size as f64);
        }

        ui.download_done();
        return Ok(());
    }

    fn download(&self, reader: &mut dyn Read, writer: &mut dyn Write, ui: &UserInterface, downloaded: u64, total_size: u64) -> Result<()> {
        let file_progress = Arc::new(AtomicUsize::new(0));
        let mut reader = ProgressReader::new(reader, |progress: usize| {
            file_progress.fetch_add(progress, Ordering::SeqCst);
            ui.set_download_progress((downloaded + file_progress.load(Ordering::SeqCst) as u64) as f64 / total_size as f64);
        });
        io::copy(&mut reader, writer).chain_err(|| ErrorKind::DownloadError(format!("Error during download")))?;
        return Ok(());
    }
}