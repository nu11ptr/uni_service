use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use uni_error::{ResultContext as _, UniResult};

use crate::ServiceErrKind;

pub(crate) const SERVICE_PERMS: u32 = 0o644;

pub(crate) fn write_file(path: &Path, data: &str, mode: u32) -> UniResult<(), ServiceErrKind> {
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).mode(mode);
    let mut file = options.open(path).kind(ServiceErrKind::IoError)?;

    file.write_all(data.as_bytes())
        .kind(ServiceErrKind::IoError)?;
    file.sync_all().kind(ServiceErrKind::IoError)?;
    Ok(())
}
