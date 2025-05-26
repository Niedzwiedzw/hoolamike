// use {
//     crate::modlist_json::image_format::DXGIFormat,
//     anyhow::{Context, Result},
//     ddsfile::{AlphaMode, D3D10ResourceDimension, Dds, DxgiFormat},
//     image::{ImageBuffer, Pixel},
//     intel_tex::{bc1, bc3, bc6h, bc7},
//     std::io::{Read, Write},
//     tap::{Pipe, Tap},
//     tracing::warn,
// };

// #[allow(non_camel_case_types)]
// #[derive(Debug, Clone, Copy)]
// enum OutputFormat {
//     BC1_TYPELESS,
//     BC1_UNORM,
//     BC1_UNORM_SRGB,
//     BC3_TYPELESS,
//     BC3_UNORM,
//     BC3_UNORM_SRGB,
//     BC6H_TYPELESS,
//     BC6H_UF16,
//     BC6H_SF16,
//     BC7_TYPELESS,
//     BC7_UNORM,
//     BC7_UNORM_SRGB,
// }

// impl OutputFormat {
//     fn match_output_format(target_format: DXGIFormat) -> Option<Self> {
//         match target_format {
//             DXGIFormat::BC1_TYPELESS => Some(Self::BC1_TYPELESS),
//             DXGIFormat::BC1_UNORM => Some(Self::BC1_UNORM),
//             DXGIFormat::BC1_UNORM_SRGB => Some(Self::BC1_UNORM_SRGB),
//             DXGIFormat::BC3_TYPELESS => Some(Self::BC3_TYPELESS),
//             DXGIFormat::BC3_UNORM => Some(Self::BC3_UNORM),
//             DXGIFormat::BC3_UNORM_SRGB => Some(Self::BC3_UNORM_SRGB),
//             DXGIFormat::BC6H_TYPELESS => Some(Self::BC6H_TYPELESS),
//             DXGIFormat::BC6H_UF16 => Some(Self::BC6H_UF16),
//             DXGIFormat::BC6H_SF16 => Some(Self::BC6H_SF16),
//             DXGIFormat::BC7_TYPELESS => Some(Self::BC7_TYPELESS),
//             DXGIFormat::BC7_UNORM => Some(Self::BC7_UNORM),
//             DXGIFormat::BC7_UNORM_SRGB => Some(Self::BC7_UNORM_SRGB),
//             _ => None,
//         }
//     }
// }

// impl From<OutputFormat> for DxgiFormat {
//     fn from(val: OutputFormat) -> Self {
//         match val {
//             OutputFormat::BC1_TYPELESS => DxgiFormat::BC1_Typeless,
//             OutputFormat::BC1_UNORM => DxgiFormat::BC1_UNorm,
//             OutputFormat::BC1_UNORM_SRGB => DxgiFormat::BC1_UNorm_sRGB,
//             OutputFormat::BC3_TYPELESS => DxgiFormat::BC3_Typeless,
//             OutputFormat::BC3_UNORM => DxgiFormat::BC3_UNorm,
//             OutputFormat::BC3_UNORM_SRGB => DxgiFormat::BC3_UNorm_sRGB,
//             OutputFormat::BC6H_TYPELESS => DxgiFormat::BC6H_Typeless,
//             OutputFormat::BC6H_UF16 => DxgiFormat::BC6H_UF16,
//             OutputFormat::BC6H_SF16 => DxgiFormat::BC6H_SF16,
//             OutputFormat::BC7_TYPELESS => DxgiFormat::BC7_Typeless,
//             OutputFormat::BC7_UNORM => DxgiFormat::BC7_UNorm,
//             OutputFormat::BC7_UNORM_SRGB => DxgiFormat::BC7_UNorm_sRGB,
//         }
//     }
// }

// macro_rules! spanned {
//     ($expr:expr) => {
//         tracing::info_span!(stringify!($expr)).in_scope(|| $expr)
//     };
// }

// fn load_image_data_from_dds(dds_file: &Dds) -> Result<image::RgbaImage> {
//     image_dds::image_from_dds(dds_file, 0).context("loading dds file")
// }

// #[tracing::instrument(skip(input, output))]
// pub fn resize_dds<R, W>(input: &mut R, target_width: u32, target_height: u32, target_format: DXGIFormat, target_mipmaps: u32, output: &mut W) -> Result<()>
// where
//     R: Read,
//     W: Write,
// {
//     OutputFormat::match_output_format(target_format)
//         .with_context(|| format!("{target_format:?} is not supported by intel tex"))
//         .and_then(|output_format| {
//             warn!("trying experimental intel texture recompression library! if it fails it will fall back to slower microsoft directxtex");
//             spanned!(Dds::read(input))
//                 .context("reading dds file")
//                 .and_then(|dds_file| {
//                     load_image_data_from_dds(&dds_file)
//                         .map(|image| {
//                             spanned!(image::imageops::resize(
//                                 &image,
//                                 target_width,
//                                 target_height,
//                                 image::imageops::FilterType::Lanczos3
//                             ))
//                         })
//                         .and_then(|image| {
//                             image.dimensions().pipe(|(width, height)| {
//                                 ImageBuffer::new(width, height)
//                                     .tap_mut(|rgba_img| {
//                                         (0..width)
//                                             .flat_map(|x| (0..height).map(move |y| (x, y)))
//                                             .map(|(x, y)| (x, y, image.get_pixel(x, y).to_rgba()))
//                                             .for_each(|(x, y, pixel)| {
//                                                 rgba_img.put_pixel(x, y, pixel);
//                                             })
//                                     })
//                                     .pipe(|rgba_img| {
//                                         let mip_count = target_mipmaps;
//                                         let array_layers = dds_file
//                                             .header10
//                                             .as_ref()
//                                             .map(|a| a.array_size)
//                                             .unwrap_or(1);
//                                         let caps2 = dds_file.header.caps2;
//                                         let is_cubemap = false;
//                                         let resource_dimension = dds_file
//                                             .header10
//                                             .as_ref()
//                                             .map(|h| h.resource_dimension)
//                                             .unwrap_or(D3D10ResourceDimension::Texture2D);
//                                         let alpha_mode = dds_file
//                                             .header10
//                                             .as_ref()
//                                             .map(|h| h.alpha_mode)
//                                             .unwrap_or(AlphaMode::Opaque);
//                                         let depth = dds_file.header.depth.unwrap_or(1);

//                                         let is_opaque = match alpha_mode {
//                                             AlphaMode::Opaque => true,
//                                             AlphaMode::Unknown => false,
//                                             AlphaMode::Straight => false,
//                                             AlphaMode::PreMultiplied => false,
//                                             AlphaMode::Custom => false,
//                                         };
//                                         Dds::new_dxgi(ddsfile::NewDxgiParams {
//                                             width: target_width,
//                                             height: target_height,
//                                             depth: Some(depth),
//                                             format: output_format.into(),
//                                             mipmap_levels: Some(mip_count),
//                                             array_layers: Some(array_layers),
//                                             caps2: Some(caps2),
//                                             is_cubemap,
//                                             resource_dimension,
//                                             alpha_mode,
//                                         })
//                                         .context("creating dds file")
//                                         .and_then(|mut dds| {
//                                             intel_tex::RgbaSurface {
//                                                 width: target_width,
//                                                 height: target_height,
//                                                 stride: width * 4,
//                                                 data: &rgba_img,
//                                             }
//                                             .pipe(|surface| {
//                                                 dds.get_mut_data(0)
//                                                     .context("layers")
//                                                     .map(|output_layer| match output_format {
//                                                         OutputFormat::BC7_TYPELESS => {
//                                                             spanned!(bc7::compress_blocks_into(
//                                                                 &match is_opaque {
//                                                                     true => bc7::opaque_ultra_fast_settings(),
//                                                                     false => bc7::alpha_ultra_fast_settings(),
//                                                                 },
//                                                                 &surface,
//                                                                 output_layer,
//                                                             ));
//                                                         }
//                                                         OutputFormat::BC1_TYPELESS => {
//                                                             spanned!(bc1::compress_blocks_into(&surface, output_layer));
//                                                         }
//                                                         OutputFormat::BC1_UNORM => {
//                                                             spanned!(bc1::compress_blocks_into(&surface, output_layer));
//                                                         }
//                                                         OutputFormat::BC1_UNORM_SRGB => {
//                                                             spanned!(bc1::compress_blocks_into(&surface, output_layer));
//                                                         }
//                                                         OutputFormat::BC3_TYPELESS => {
//                                                             spanned!(bc3::compress_blocks_into(&surface, output_layer));
//                                                         }
//                                                         OutputFormat::BC3_UNORM => {
//                                                             spanned!(bc3::compress_blocks_into(&surface, output_layer));
//                                                         }
//                                                         OutputFormat::BC3_UNORM_SRGB => {
//                                                             spanned!(bc3::compress_blocks_into(&surface, output_layer));
//                                                         }
//                                                         OutputFormat::BC6H_TYPELESS => {
//                                                             spanned!(bc6h::compress_blocks_into(
//                                                                 &match is_opaque {
//                                                                     true => bc6h::very_fast_settings(),
//                                                                     false => bc6h::very_fast_settings(),
//                                                                 },
//                                                                 &surface,
//                                                                 output_layer,
//                                                             ));
//                                                         }
//                                                         OutputFormat::BC6H_UF16 => {
//                                                             spanned!(bc6h::compress_blocks_into(
//                                                                 &match is_opaque {
//                                                                     true => bc6h::very_fast_settings(),
//                                                                     false => bc6h::very_fast_settings(),
//                                                                 },
//                                                                 &surface,
//                                                                 output_layer,
//                                                             ));
//                                                         }
//                                                         OutputFormat::BC6H_SF16 => {
//                                                             spanned!(bc6h::compress_blocks_into(
//                                                                 &match is_opaque {
//                                                                     true => bc6h::very_fast_settings(),
//                                                                     false => bc6h::very_fast_settings(),
//                                                                 },
//                                                                 &surface,
//                                                                 output_layer,
//                                                             ));
//                                                         }
//                                                         OutputFormat::BC7_UNORM => {
//                                                             spanned!(bc7::compress_blocks_into(
//                                                                 &match is_opaque {
//                                                                     true => bc7::opaque_ultra_fast_settings(),
//                                                                     false => bc7::alpha_ultra_fast_settings(),
//                                                                 },
//                                                                 &surface,
//                                                                 output_layer,
//                                                             ));
//                                                         }
//                                                         OutputFormat::BC7_UNORM_SRGB => {
//                                                             spanned!(bc7::compress_blocks_into(
//                                                                 &match is_opaque {
//                                                                     true => bc7::opaque_ultra_fast_settings(),
//                                                                     false => bc7::alpha_ultra_fast_settings(),
//                                                                 },
//                                                                 &surface,
//                                                                 output_layer,
//                                                             ));
//                                                         }
//                                                     })
//                                             })
//                                         })
//                                     })
//                             })
//                         })
//                         .and_then(|_| dds_file.write(output).context("writing dds file"))
//                 })
//         })
// }
