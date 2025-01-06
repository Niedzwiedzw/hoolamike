use {
    crate::{modlist_json::image_format::DXGIFormat, progress_bars_v2::IndicatifWrapIoExt},
    anyhow::{Context, Result},
    directxtex::{self, TexMetadata, DDS_FLAGS, DXGI_FORMAT, TEX_COMPRESS_FLAGS, TEX_FILTER_FLAGS, TEX_THRESHOLD_DEFAULT},
    num::ToPrimitive,
    std::io::{Read, Write},
    tap::prelude::*,
};

pub mod dxgi_format_mapping;

macro_rules! spanned {
    ($expr:expr) => {
        tracing::info_span!(stringify!($expr)).in_scope(|| $expr)
    };
}

#[tracing::instrument(skip(input, output))]
pub fn resize_dds<R, W>(input: &mut R, target_width: u32, target_height: u32, target_format: DXGIFormat, target_mipmaps: u32, output: &mut W) -> Result<u64>
where
    R: Read,
    W: Write,
{
    let dds_flags = DDS_FLAGS::DDS_FLAGS_PERMISSIVE;
    let tex_filter_flags = TEX_FILTER_FLAGS::TEX_FILTER_TRIANGLE;
    let tex_compress_flags = TEX_COMPRESS_FLAGS::TEX_COMPRESS_DEFAULT;

    let target_format = self::dxgi_format_mapping::map_dxgi_format(target_format);

    Vec::new()
        .pipe(|mut buf| spanned!(input.read_to_end(&mut buf)).map(|_| buf))
        .context("reading bytes")
        .and_then(|bytes| {
            let tex_metadata = spanned!(TexMetadata::from_dds(&bytes, dds_flags, None)).context("reading tex metadata")?;
            Ok(())
                .and_then(|_| {
                    Ok(())
                        .and_then(|_| {
                            directxtex::ScratchImage::load_dds(&bytes, dds_flags, None, None)
                                .context("loading dds")
                                .and_then(|image| match tex_metadata.format.is_compressed() {
                                    true => spanned!(image.decompress(DXGI_FORMAT::DXGI_FORMAT_R32G32B32A32_FLOAT))
                                        .context("decompressing image into DXGI_FORMAT::DXGI_FORMAT_R32G32B32A32_FLOAT"),
                                    false => spanned!(image.convert(DXGI_FORMAT::DXGI_FORMAT_R32G32B32A32_FLOAT, tex_filter_flags, TEX_THRESHOLD_DEFAULT))
                                        .context("converting image DXGI_FORMAT::DXGI_FORMAT_R32G32B32A32_FLOAT"),
                                })
                                .context("loading image")
                        })
                        .and_then(|image| {
                            Ok(image)
                                .and_then(|image| {
                                    let (width, height) = (
                                        target_width.to_usize().context("bad target_width")?,
                                        target_height.to_usize().context("bad target_height")?,
                                    );
                                    spanned!(image.resize(width, height, tex_filter_flags,)).context("resizing")
                                })
                                .and_then(|resized| {
                                    let target_mipmaps = target_mipmaps.to_usize().context("bad target_mipmaps")?;

                                    match target_mipmaps {
                                        1 => Ok(resized),
                                        target_mipmaps => spanned!(resized.generate_mip_maps(tex_filter_flags, target_mipmaps)).context("generating mip maps"),
                                    }
                                })
                                .context("modifying image")
                        })
                        .and_then(|image| match target_format.is_compressed() {
                            true => {
                                let tex_compress_flags = tex_compress_flags.pipe(|tex_compress_flags| match target_format {
                                    DXGI_FORMAT::DXGI_FORMAT_BC7_TYPELESS | DXGI_FORMAT::DXGI_FORMAT_BC7_UNORM | DXGI_FORMAT::DXGI_FORMAT_BC7_UNORM_SRGB => {
                                        tex_compress_flags.union(TEX_COMPRESS_FLAGS::TEX_COMPRESS_BC7_QUICK)
                                    }
                                    _ => tex_compress_flags,
                                });
                                spanned!(image.compress(target_format, tex_compress_flags, TEX_THRESHOLD_DEFAULT))
                                    .with_context(|| format!("compressing using target_format={target_format:?}"))
                                    .context("compressing image")
                            }
                            false => spanned!(image.convert(target_format, tex_filter_flags, TEX_THRESHOLD_DEFAULT))
                                .with_context(|| format!("compressing using target_format={target_format:?}"))
                                .context("compressing image"),
                        })
                })
                .and_then(|image| {
                    image
                        .images()
                        .pipe(|images| {
                            spanned!(directxtex::save_dds(
                                images,
                                &tex_metadata.tap_mut(|metadata| {
                                    metadata.width = target_width as _;
                                    metadata.height = target_height as _;
                                    metadata.mip_levels = target_mipmaps as _;
                                    metadata.format = target_format;
                                }),
                                dds_flags,
                            ))
                            .with_context(|| format!("saving [{}] images as dds", images.len()))
                        })
                        .context("saving dds image to bytes")
                })
                .and_then(|blob| {
                    std::io::copy(
                        &mut tracing::Span::current().wrap_read(blob.buffer().len() as _, std::io::Cursor::new(blob.buffer())),
                        output,
                    )
                    .context("writing dds file")
                })
                .with_context(|| format!("recompressing using derived tex_metadata={tex_metadata:? }"))
        })
        .tap_ok(|wrote| tracing::debug!("wrote {wrote} bytes"))
}
