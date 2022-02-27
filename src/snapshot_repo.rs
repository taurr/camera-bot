use anyhow::Result;
use opencv::{core::Vector, imgcodecs, prelude::Mat};
use std::{
    fs::create_dir_all,
    path::{Path, PathBuf},
};
use tracing::{info, instrument, trace, warn};

#[derive(Debug)]
pub struct SnapshotRepo {
    counter: usize,
    path: PathBuf,
    name: String,
}

impl SnapshotRepo {
    /// Create a new snapshot repository.
    ///
    /// `name` should contain the pattern `$COUNTER$` in order to supstitute the framecounter
    /// when save snapshorts. Also, `name` may contain standard time formatting strings (see `chrono`).
    #[instrument(skip_all)]
    pub fn from_path_and_namepattern(path: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        Self {
            counter: 0,
            path: path.into(),
            name: name.into(),
        }
    }

    #[instrument]
    pub fn save_frame(&mut self, frame: &Mat) -> Result<()> {
        let filename = self.get_filename();
        create_dir_all(Path::new(self.path.as_path()).parent().unwrap())?;
        imgcodecs::imwrite(&filename.display().to_string(), &frame, &Vector::default())?;
        info!(?filename, "Image saved");
        self.counter += 1;
        Ok(())
    }

    fn get_filename(&mut self) -> PathBuf {
        let now = chrono::Local::now().format(&self.name).to_string();
        let mut filename = self
            .path
            .join(now.replace("$COUNTER$", &self.counter.to_string()));
        while filename.exists() {
            warn!(?filename, "file already exists");
            self.counter += 1;
            filename = self
                .path
                .join(self.name.replace("$COUNTER$", &self.counter.to_string()));
        }
        trace!(?filename);
        filename
    }
}
