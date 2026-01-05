use std::ffi::OsString;

use uni_error::*;

use crate::ServiceErrKind;

pub(crate) fn os_string_to_string(
    os_string: impl Into<OsString>,
) -> UniResult<String, ServiceErrKind> {
    os_string
        .into()
        .into_string()
        .map_err(|_| ServiceErrKind::BadUtf8.into_error())
}
