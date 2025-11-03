use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use uni_error::SimpleResult;

pub(crate) fn write_file(path: &Path, data: &str, mode: u32) -> SimpleResult<()> {
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).mode(mode);
    let mut file = options.open(path)?;

    file.write_all(data.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
