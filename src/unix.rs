use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use uni_error::SimpleResult;

use crate::{Result, ServiceApp, run_service};

pub(crate) fn start_service(app: Box<dyn ServiceApp + Send>) -> Result<()> {
    // Won't endlessly loop because this is only called when service_mode is true
    run_service(app, false)
}

pub(crate) fn write_file(path: &Path, data: &str, mode: u32) -> SimpleResult<()> {
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).mode(mode);
    let mut file = options.open(path)?;

    file.write_all(data.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
