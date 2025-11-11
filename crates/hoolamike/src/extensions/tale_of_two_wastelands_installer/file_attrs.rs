use {
    super::manifest_file::FileAttr,
    anyhow::{Context, Result},
    case_insensitive_path::CaseInsensitivePathBuf,
    chrono::{DateTime, Utc},
    std::{
        str::FromStr,
        time::{SystemTime, UNIX_EPOCH},
    },
    tap::prelude::*,
    tracing::info,
};
fn chrono_to_system_time(dt: DateTime<Utc>) -> SystemTime {
    // The number of whole seconds since the Unix epoch
    let secs = dt.timestamp();
    // The subsecond nanoseconds
    let nsecs = dt.timestamp_subsec_nanos();

    if secs >= 0 {
        UNIX_EPOCH + std::time::Duration::new(secs as u64, nsecs)
    } else {
        // For times before the Unix epoch, subtract:
        UNIX_EPOCH - std::time::Duration::new((-secs) as u64, nsecs)
    }
}
pub fn handle_file_attrs(file_attrs: Vec<FileAttr>) -> Result<()> {
    file_attrs
        .into_iter()
        .try_for_each(|FileAttr { value, last_modified }| {
            CaseInsensitivePathBuf::from_str(&value)
                .and_then(|value| value.try_exists())
                .and_then(|path| {
                    let last_modified = last_modified
                        .with_timezone(&chrono::Utc)
                        .pipe(chrono_to_system_time);
                    let file_time = filetime::FileTime::from_system_time(last_modified);
                    info!("updating [{path:?}]: modified_time = [{file_time}]");
                    filetime::set_file_mtime(&path, file_time).with_context(|| format!("setting file time of [{path:?}] to [{file_time}]"))
                })
        })
}
