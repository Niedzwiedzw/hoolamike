// Import the Texconv builder and related enums
use {
    crate::{compression::SeekWithTempFileExt, consts::TEMP_FILE_DIR, modlist_json::image_format::DXGIFormat},
    ::proton_wrapper::proton_context::{Initialized, WineContext},
    ::texconv_wrapper::{BcFlag, FileType, ImageFilter, Texconv},
    anyhow::{Context, Result},
    itertools::Itertools,
    proton_wrapper::proton_context::CommandWrapInProtonExt,
    std::{
        io::{Read, Write},
        num::NonZeroU32,
        path::{Path, PathBuf},
    },
    tap::{Pipe, TapFallible},
    tracing::info,
};

mod dxgi_format_mapping;

macro_rules! spanned {
    ($expr:expr) => {
        tracing::info_span!(stringify!($expr)).in_scope(|| $expr)
    };
}

/// The number of bytes written to the output stream.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip(input, output))]
pub fn resize_dds<R, W>(
    input: &mut R,
    target_width: u32,
    target_height: u32,
    target_format: DXGIFormat,
    target_mipmaps: u32,
    output: &mut W,
    texconv_binary: &Path,
    proton_context: &Initialized<WineContext>,
    extension: &str,
) -> Result<u64>
where
    R: Read,
    W: Write,
{
    // Map the DXGIFormat to a texconv-compatible format string
    dxgi_format_mapping::map_dxgi_format(target_format)
        .context("mapping DXGI format to texconv format")
        .and_then(|format_str| {
            input
                .seek_with_temp_file_blocking_raw_with_extension(extension, 0)
                .context("loading input")
                .and_then(|(_size, input)| {
                    tempfile::Builder::new()
                        .prefix("dds-output-")
                        .tempdir_in(*TEMP_FILE_DIR)
                        .context("creating output dir")
                        .map(|output_dir| (format_str, input, output_dir))
                })
        })
        .and_then(|(format_str, input_file, output_dir)| {
            Texconv::builder(proton_context.host_to_pfx_path(texconv_binary)?.to_string())
                .input_file(proton_context.host_to_pfx_path(&input_file)?.to_string())
                .output_dir(
                    proton_context
                        .host_to_pfx_path(output_dir.path())?
                        .to_string(),
                )
                .file_type(FileType::Dds)
                .format(format_str)
                .width(target_width)
                .height(target_height)
                // .ignore_mips(true)
                .maybe_mip_levels(NonZeroU32::new(target_mipmaps))
                .image_filter(ImageFilter::Triangle) // Matches TEX_FILTER_FLAGS::TEX_FILTER_TRIANGLE
                .permissive(true) // Matches DDS_FLAGS::DDS_FLAGS_PERMISSIVE
                .maybe_bc_flag(match target_format {
                    DXGIFormat::BC7_TYPELESS | DXGIFormat::BC7_UNORM | DXGIFormat::BC7_UNORM_SRGB => BcFlag::Quick.pipe(Some),
                    _ => None, // Default for other compressed formats
                })
                .no_logo(true)
                .single_proc(true)
                .build()
                .command()
                .wrap_in_proton(proton_context)
                .and_then(|command| spanned!(command.output_blocking()))
                .map(|output| info!("{output}"))
                .context("spawning proton command")
                .and_then(|()| {
                    std::fs::read_dir(output_dir.path())
                        .context("reading output dir")
                        .and_then(|output_dir| {
                            output_dir
                                .filter_ok(|d| d.metadata().map(|d| d.is_file()).unwrap_or(false))
                                .next()
                                .context("output dir empty")
                                .and_then(|e| e.context("bad entry"))
                                .map(|entry| entry.path())
                        })
                        .and_then(|result| {
                            std::fs::File::options()
                                .read(true)
                                .open(&result)
                                .with_context(|| format!("opening {result:?}"))
                                .and_then(|mut result| std::io::copy(&mut result, output).context("copying output into output buffer"))
                        })
                })
                .context("trying to resize texture using texconv + proton")
                .tap_ok(|size| info!("texconv proton success: {size}"))
                .pipe(|reason| match reason {
                    Ok(v) => Ok(v),
                    Err(reason) => {
                        tracing::warn!("could not recompress texture:\n{reason:?}");
                        #[cfg(debug_assertions)]
                        {
                            use crate::install_modlist::download_cache::sha512_hex_string;
                            format!("{reason:?}")
                                .pipe(|reason| sha512_hex_string(reason.as_bytes()))
                                .pipe(|name| format!("debug-dump--{name}.dds"))
                                .pipe(PathBuf::from)
                                .pipe(|output_path| {
                                    std::fs::copy(&input_file, &output_path)
                                        .context("dumping file")
                                        .and_then(|_| output_path.canonicalize().context("canonicalizing"))
                                })
                                .context("preparing debug dump")
                                .pipe(|r| match r {
                                    Ok(output_path) => Err(reason).with_context(|| format!("DEBUG DUMP AVAILABLE AT: {}", output_path.display())),
                                    Err(failed_to_dump) => Err(reason).with_context(|| format!("COULD NOT EVEN DUMP THE FILE: {failed_to_dump:?}")),
                                })
                        }
                        #[cfg(not(debug_assertions))]
                        {
                            Err(reason).context("more details available in debug mode")
                        }
                    }
                })
        })
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     #[test_log::test]
//     fn test_basic() -> Result<()> {
//     // proton_path: "/home/niedzwiedz/.local/share/Steam/steamapps/common/Proton - Experimental/proton"
//     // prefix_dir: "texconv-pfx"
//     // steam_path: "/home/niedzwiedz/.local/share/Steam"
//     // texconv_path: "texconv-pfx/texconv.exe"

//         let proton_context = ProtonContext {
//             proton_path: "/home/niedzwiedz/.local/share/Steam/steamapps/common/Proton - Experimental/proton".into(),
//             prefix_dir: "/tmp/test-pfx",
//             steam_path: "/home/niedzwiedz/.local/share/Steam".to_owned(),
//         };
//         let context = proton_context.initialize()?;
//         let input = std::io::Cursor::new(include_bytes!())
//         super::resize_dds(input, target_width, target_height, target_format, target_mipmaps, output, texconv_binary, proton_context)

//         Ok(())
//     }
// }
