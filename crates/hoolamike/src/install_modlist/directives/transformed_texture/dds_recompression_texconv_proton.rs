// Import the Texconv builder and related enums
use {
    crate::{compression::SeekWithTempFileExt, consts::TEMP_FILE_DIR, modlist_json::image_format::DXGIFormat},
    ::proton_wrapper::CommandWrapInProtonExt,
    ::texconv_wrapper::{BcFlag, FileType, ImageFilter, Texconv},
    anyhow::{Context, Result},
    itertools::Itertools,
    proton_wrapper::{Initialized, ProtonContext},
    std::{
        io::{Read, Write},
        path::Path,
    },
    tap::TapFallible,
    tempfile::{NamedTempFile, TempDir},
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
    proton_context: &Initialized<ProtonContext>,
) -> Result<u64>
where
    R: Read,
    W: Write,
{
    // Map the DXGIFormat to a texconv-compatible format string
    let format_str = dxgi_format_mapping::map_dxgi_format(target_format).context("mapping DXGI format to texconv format")?;
    let (_size, input_file) = input
        .seek_with_temp_file_blocking_raw(0)
        .context("loading input")?;
    let output_dir = TempDir::new_in(*TEMP_FILE_DIR).context("creating output dir")?;
    // Configure texconv with the desired options
    Texconv::builder(texconv_binary)
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
        .mip_levels(target_mipmaps)
        .image_filter(ImageFilter::Triangle) // Matches TEX_FILTER_FLAGS::TEX_FILTER_TRIANGLE
        .permissive(true) // Matches DDS_FLAGS::DDS_FLAGS_PERMISSIVE
        .bc_flag(match target_format {
            DXGIFormat::BC7_TYPELESS | DXGIFormat::BC7_UNORM | DXGIFormat::BC7_UNORM_SRGB => BcFlag::Quick,
            _ => BcFlag::Dither, // Default for other compressed formats
        })
        .no_logo(true)
        .build()
        .command()
        .wrap_in_proton(proton_context)
        .and_then(|command| spanned!(command.output()))
        .context("spawning proton command")
        .map(|output| info!("OUTPUT:{output}"))
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
}
