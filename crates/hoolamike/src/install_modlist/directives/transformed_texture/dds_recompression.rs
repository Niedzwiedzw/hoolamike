use {
    crate::modlist_json::image_format::DXGIFormat,
    anyhow::{Context, Result},
    image_dds::{self, image::DynamicImage, mip_dimension, SurfaceRgba32Float},
    std::io::{Read, Write},
    tracing::warn,
    write_counter::ByteCounter,
};

mod write_counter {
    use std::io::{self, Write};

    pub struct ByteCounter<W> {
        inner: W,
        count: usize,
    }
    #[allow(dead_code)]
    impl<W> ByteCounter<W> {
        pub fn new(inner: W) -> Self {
            ByteCounter { inner, count: 0 }
        }

        pub fn get_count(&self) -> usize {
            self.count
        }

        pub fn into_inner(self) -> W {
            self.inner
        }
    }

    impl<W: Write> Write for ByteCounter<W> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let bytes_written = self.inner.write(buf)?;
            self.count += bytes_written;
            Ok(bytes_written)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }

        // Forward vectored write implementation if inner writer supports it
        fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
            let bytes_written = self.inner.write_vectored(bufs)?;
            self.count += bytes_written;
            Ok(bytes_written)
        }
    }

    impl<W> From<W> for ByteCounter<W> {
        fn from(inner: W) -> Self {
            ByteCounter::new(inner)
        }
    }
}

#[allow(dead_code)]
fn match_dxgi_format(format: DXGIFormat) -> Option<image_dds::ImageFormat> {
    match format {
        DXGIFormat::R8_UNORM => Some(image_dds::ImageFormat::R8Unorm),
        DXGIFormat::R8_SNORM => Some(image_dds::ImageFormat::R8Snorm),
        DXGIFormat::R8G8_UNORM => Some(image_dds::ImageFormat::Rg8Unorm),
        DXGIFormat::R8G8_SNORM => Some(image_dds::ImageFormat::Rg8Snorm),
        DXGIFormat::R8G8B8A8_UNORM => Some(image_dds::ImageFormat::Rgba8Unorm),
        DXGIFormat::R8G8B8A8_UNORM_SRGB => Some(image_dds::ImageFormat::Rgba8UnormSrgb),
        DXGIFormat::R8G8B8A8_SNORM => Some(image_dds::ImageFormat::Rgba8Snorm),
        DXGIFormat::R16_UNORM => Some(image_dds::ImageFormat::R16Unorm),
        DXGIFormat::R16_SNORM => Some(image_dds::ImageFormat::R16Snorm),
        DXGIFormat::R16G16_UNORM => Some(image_dds::ImageFormat::Rg16Unorm),
        DXGIFormat::R16G16_SNORM => Some(image_dds::ImageFormat::Rg16Snorm),
        DXGIFormat::R16G16B16A16_UNORM => Some(image_dds::ImageFormat::Rgba16Unorm),
        DXGIFormat::R16G16B16A16_SNORM => Some(image_dds::ImageFormat::Rgba16Snorm),
        DXGIFormat::R16_FLOAT => Some(image_dds::ImageFormat::R16Float),
        DXGIFormat::R16G16_FLOAT => Some(image_dds::ImageFormat::Rg16Float),
        DXGIFormat::R32_FLOAT => Some(image_dds::ImageFormat::R32Float),
        DXGIFormat::R32G32_FLOAT => Some(image_dds::ImageFormat::Rg32Float),
        DXGIFormat::R32G32B32_FLOAT => Some(image_dds::ImageFormat::Rgb32Float),
        DXGIFormat::R32G32B32A32_FLOAT => Some(image_dds::ImageFormat::Rgba32Float),
        DXGIFormat::R16G16B16A16_FLOAT => Some(image_dds::ImageFormat::Rgba16Float),
        DXGIFormat::B8G8R8A8_UNORM => Some(image_dds::ImageFormat::Bgra8Unorm),
        DXGIFormat::B8G8R8A8_UNORM_SRGB => Some(image_dds::ImageFormat::Bgra8UnormSrgb),
        DXGIFormat::B4G4R4A4_UNORM => Some(image_dds::ImageFormat::Bgra4Unorm),
        DXGIFormat::B5G5R5A1_UNORM => Some(image_dds::ImageFormat::Bgr5A1Unorm),
        DXGIFormat::BC1_UNORM => Some(image_dds::ImageFormat::BC1RgbaUnorm),
        DXGIFormat::BC1_UNORM_SRGB => Some(image_dds::ImageFormat::BC1RgbaUnormSrgb),
        DXGIFormat::BC2_UNORM => Some(image_dds::ImageFormat::BC2RgbaUnorm),
        DXGIFormat::BC2_UNORM_SRGB => Some(image_dds::ImageFormat::BC2RgbaUnormSrgb),
        DXGIFormat::BC3_UNORM => Some(image_dds::ImageFormat::BC3RgbaUnorm),
        DXGIFormat::BC3_UNORM_SRGB => Some(image_dds::ImageFormat::BC3RgbaUnormSrgb),
        DXGIFormat::BC4_UNORM => Some(image_dds::ImageFormat::BC4RUnorm),
        DXGIFormat::BC4_SNORM => Some(image_dds::ImageFormat::BC4RSnorm),
        DXGIFormat::BC5_UNORM => Some(image_dds::ImageFormat::BC5RgUnorm),
        DXGIFormat::BC5_SNORM => Some(image_dds::ImageFormat::BC5RgSnorm),
        DXGIFormat::BC6H_UF16 => Some(image_dds::ImageFormat::BC6hRgbUfloat),
        DXGIFormat::BC6H_SF16 => Some(image_dds::ImageFormat::BC6hRgbSfloat),
        DXGIFormat::BC7_UNORM => Some(image_dds::ImageFormat::BC7RgbaUnorm),
        DXGIFormat::BC7_UNORM_SRGB => Some(image_dds::ImageFormat::BC7RgbaUnormSrgb),
        _ => None, // No match for typeless, depth/stencil, video, or other unsupported formats
    }
}

#[tracing::instrument(skip(input, output))]
pub fn resize_dds<R, W>(input: &mut R, target_width: u32, target_height: u32, target_format: DXGIFormat, target_mipmaps: u32, output: &mut W) -> Result<u64>
where
    R: Read,
    W: Write,
{
    warn!("[EXPERIMENTAL] trying experimental intel tex library");
    let target_format = match_dxgi_format(target_format).with_context(|| format!("unsupported format: {target_format:?}"))?;
    let mut output = ByteCounter::new(output);
    image_dds::ddsfile::Dds::read(input)
        .context("reading dds file")
        .and_then(|dds| {
            image_dds::Surface::from_dds(&dds)
                .context("reading surface")
                .and_then(|surface| {
                    surface
                        .decode_rgbaf32()
                        .context("decoding rgbaf32")
                        .and_then(|decoded| {
                            // note to self: layer == face
                            std::iter::once(())
                                .flat_map(|_| (0..decoded.layers))
                                .flat_map(|layer| (0..decoded.depth).map(move |depth| (layer, depth)))
                                .map(|(layer, depth)| {
                                    // we will regenerate mipmaps
                                    let mipmap = 0;
                                    decoded
                                        .get(layer, depth, mipmap)
                                        .context("getting the chunk from decoded surface")
                                        .and_then(|data| {
                                            image_dds::image::ImageBuffer::from_raw(
                                                mip_dimension(surface.width, mipmap),
                                                mip_dimension(surface.height, mipmap),
                                                data.to_vec(),
                                            )
                                            .context("loading part into an ImageBuffer failed")
                                        })
                                        .map(DynamicImage::ImageRgba32F)
                                        .map(|image| image.resize_exact(target_width, target_height, image_dds::image::imageops::FilterType::Lanczos3))
                                        .map(|resized| resized.into_rgba32f())
                                        .with_context(|| format!("processing part layer={layer}, depth={depth}, mipmap={mipmap}"))
                                })
                                .try_fold(Vec::new(), |mut acc, part| {
                                    part.map(|part| {
                                        acc.extend(part.into_vec());
                                        acc
                                    })
                                })
                                .with_context(|| {
                                    format!(
                                        "resizing all parts of dds (layers={}, depths={}, mipmaps={}, image_format={:?}, data_len=[{}])",
                                        surface.layers,
                                        surface.depth,
                                        surface.mipmaps,
                                        surface.image_format,
                                        surface.data.len()
                                    )
                                })
                        })
                        .map(|data| SurfaceRgba32Float {
                            data,
                            width: target_width,
                            height: target_height,
                            depth: surface.depth,
                            layers: surface.layers,
                            // this newly created surface only has 1 mipmap, the encoder will generate the desired amount
                            mipmaps: 1,
                        })
                        .and_then(|resized_surface| {
                            resized_surface
                                .encode(
                                    target_format,
                                    image_dds::Quality::Normal,
                                    image_dds::Mipmaps::GeneratedExact(target_mipmaps.saturating_sub(1)),
                                )
                                .context("reencoding surface")
                        })
                })
        })
        .and_then(|reencoded| reencoded.to_dds().context("creating a dds file"))
        .and_then(|dds| {
            dds.write(&mut output)
                .context("writing dds file to output")
                .map(|_| output.get_count() as u64)
        })
        .context("recompressing/resizing a dds file")
}
