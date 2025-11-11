use {
    super::*, case_insensitive_path::ExistingPathBuf, tracing::{info, info_span}
};

const CARGO_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
const INTEGRATION_TEST_DIRECTORY: &str = env!("HOOLAMIKE_INTEGRATION_TEST_DIR");

fn test_data_directory() -> ExistingPathBuf {
    CARGO_MANIFEST_DIR
        .pipe(std::path::Path::new)
        .parent()
        .and_then(|p| p.parent())
        .map(|workspace| workspace.join(INTEGRATION_TEST_DIRECTORY))
        .unwrap()
        .pipe_deref(ExistingPathBuf::new)
        .unwrap()
}

#[ignore]
#[test_log::test]
fn list_archives() -> Result<()> {
    test_data_directory().pipe(|bethesda_directory| -> Result<()> {
        info!("listing {bethesda_directory:?}");
        for file in std::fs::read_dir(bethesda_directory)?.take(10) {
            let file = file?.path();
            let _span = info_span!("reading_archive", file = %file.display()).entered();
            let extension = file
                .extension()
                .as_ref()
                .map(|e| e.to_string_lossy().to_string());
            match extension.as_deref() {
                Some("ba2") => {
                    info!("checking archive");
                    let mut archive = ba2::fo4::Archive::read(file.as_path())?;
                    info!("archive opened");
                    let entries = archive.list_paths()?;
                    info!("archive has {} entries", entries.len());
                    for entry_path in entries.into_iter().take(5) {
                        let mut entry = archive.get_handle(&entry_path)?;
                        let size = std::io::copy(&mut entry, &mut std::io::sink())?;
                        if size == 0 {
                            anyhow::bail!("{entry_path:?} has size of 0, impossible")
                        }
                    }
                }
                _ => continue,
            }
        }
        Ok(())
    })
}
