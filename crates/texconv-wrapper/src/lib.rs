use {
    bon::Builder,
    std::{num::NonZeroU32, path::PathBuf, process::Command},
};

/// Enum for output file types supported by texconv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// Windows BMP format.
    Bmp,
    /// Joint Photographic Experts Group format.
    Jpg,
    /// Alternate name to Jpg for Joint Photographic Experts Group format.
    Jpeg,
    /// Portable Network Graphics format.
    Png,
    /// DirectDraw Surface (Direct3D texture file format).
    Dds,
    /// Alternate name to Dds for DirectDraw Surface.
    Ddx,
    /// Truevision Graphics Adapter format.
    Tga,
    /// Radiance RGBE format.
    Hdr,
    /// Tagged Image File Format.
    Tif,
    /// Alternate name for Tif (Tagged Image File Format).
    Tiff,
    /// Windows Media Photo format.
    Wdp,
    /// Alternate name for Wdp (Windows Media Photo).
    Hdp,
    /// Alternate name for Wdp (Windows Media Photo).
    Jxr,
    /// Portable PixMap format (Netpbm).
    Ppm,
    /// Portable FloatMap format (Netpbm).
    Pfm,
}

impl FileType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Bmp => "bmp",
            Self::Jpg => "jpg",
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Dds => "dds",
            Self::Ddx => "ddx",
            Self::Tga => "tga",
            Self::Hdr => "hdr",
            Self::Tif => "tif",
            Self::Tiff => "tiff",
            Self::Wdp => "wdp",
            Self::Hdp => "hdp",
            Self::Jxr => "jxr",
            Self::Ppm => "ppm",
            Self::Pfm => "pfm",
        }
    }
}

/// Enum for image filters used for resizing images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFilter {
    /// Nearest-neighbor interpolation.
    Point,
    /// Linear interpolation.
    Linear,
    /// Cubic interpolation.
    Cubic,
    /// Fant interpolation.
    Fant,
    /// Box filter.
    Box,
    /// Triangle filter.
    Triangle,
    /// Point filter with ordered dithering.
    PointDither,
    /// Linear filter with ordered dithering.
    LinearDither,
    /// Cubic filter with ordered dithering.
    CubicDither,
    /// Fant filter with ordered dithering.
    FantDither,
    /// Box filter with ordered dithering.
    BoxDither,
    /// Triangle filter with ordered dithering.
    TriangleDither,
    /// Point filter with error diffusion dithering.
    PointDitherDiffusion,
    /// Linear filter with error diffusion dithering.
    LinearDitherDiffusion,
    /// Cubic filter with error diffusion dithering.
    CubicDitherDiffusion,
    /// Fant filter with error diffusion dithering.
    FantDitherDiffusion,
    /// Box filter with error diffusion dithering.
    BoxDitherDiffusion,
    /// Triangle filter with error diffusion dithering.
    TriangleDitherDiffusion,
}

impl ImageFilter {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Point => "POINT",
            Self::Linear => "LINEAR",
            Self::Cubic => "CUBIC",
            Self::Fant => "FANT",
            Self::Box => "BOX",
            Self::Triangle => "TRIANGLE",
            Self::PointDither => "POINT_DITHER",
            Self::LinearDither => "LINEAR_DITHER",
            Self::CubicDither => "CUBIC_DITHER",
            Self::FantDither => "FANT_DITHER",
            Self::BoxDither => "BOX_DITHER",
            Self::TriangleDither => "TRIANGLE_DITHER",
            Self::PointDitherDiffusion => "POINT_DITHER_DIFFUSION",
            Self::LinearDitherDiffusion => "LINEAR_DITHER_DIFFUSION",
            Self::CubicDitherDiffusion => "CUBIC_DITHER_DIFFUSION",
            Self::FantDitherDiffusion => "FANT_DITHER_DIFFUSION",
            Self::BoxDitherDiffusion => "BOX_DITHER_DIFFUSION",
            Self::TriangleDitherDiffusion => "TRIANGLE_DITHER_DIFFUSION",
        }
    }
}

/// Enum for color rotation options for HDR10 and other colorspace conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateColor {
    /// Converts from Rec.709 to Rec.2020 color primaries.
    Rec709To2020,
    /// Converts from Rec.2020 to Rec.709 color primaries.
    Rec2020To709,
    /// Converts from Rec.709 to Rec.2020, normalizing nits and applying ST.2084 curve for HDR10.
    Rec709ToHdr10,
    /// Converts HDR10 signal back to Rec.709 color primaries with linear values.
    Hdr10ToRec709,
    /// Converts from DCI-P3 to Rec.2020 color primaries.
    P3To2020,
    /// Converts from DCI-P3 to Rec.2020, normalizing nits and applying ST.2084 curve for HDR10.
    P3ToHdr10,
    /// Converts from Rec.709 to Display-P3 (D65 white point).
    Rec709ToDisplayP3,
    /// Converts from Display-P3 (D65 white point) to Rec.709 color primaries.
    DisplayP3ToRec709,
}

impl RotateColor {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Rec709To2020 => "709to2020",
            Self::Rec2020To709 => "2020to709",
            Self::Rec709ToHdr10 => "709toHDR10",
            Self::Hdr10ToRec709 => "HDR10to709",
            Self::P3To2020 => "P3to2020",
            Self::P3ToHdr10 => "P3toHDR10",
            Self::Rec709ToDisplayP3 => "709toDisplayP3",
            Self::DisplayP3ToRec709 => "DisplayP3to709",
        }
    }
}

/// Enum for block compression flags used with BC formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BcFlag {
    /// Use uniform weighting instead of perceptual for BC1-BC3.
    Uniform,
    /// Use dithering for BC1-BC3 compression.
    Dither,
    /// Use minimal compression for BC7 (mode 6 only).
    Quick,
    /// Use maximum compression for BC7 (enables mode 0 & 2).
    Exhaustive,
}

impl BcFlag {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Uniform => "u",
            Self::Dither => "d",
            Self::Quick => "q",
            Self::Exhaustive => "x",
        }
    }
}

/// Enum for normal map generation flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmapFlag {
    /// Use red channel as height for normal map generation.
    Red,
    /// Use green channel as height for normal map generation.
    Green,
    /// Use blue channel as height for normal map generation.
    Blue,
    /// Use alpha channel as height for normal map generation.
    Alpha,
    /// Use luminance computed from RGB channels as height.
    Luminance,
    /// Use mirroring in both U and V for central difference computation.
    MirrorUv,
    /// Use mirroring in U for central difference computation.
    MirrorU,
    /// Use mirroring in V for central difference computation.
    MirrorV,
    /// Invert sign of the computed normal.
    InvertSign,
    /// Compute a rough occlusion term and encode in alpha channel.
    Occlusion,
}

impl NmapFlag {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Red => "r",
            Self::Green => "g",
            Self::Blue => "b",
            Self::Alpha => "a",
            Self::Luminance => "l",
            Self::MirrorUv => "m",
            Self::MirrorU => "u",
            Self::MirrorV => "v",
            Self::InvertSign => "i",
            Self::Occlusion => "o",
        }
    }
}

/// Enum for Direct3D feature levels determining maximum texture size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureLevel {
    /// Feature Level 9.1 (max texture size: 2048).
    Fl9_1,
    /// Feature Level 9.2 (max texture size: 2048).
    Fl9_2,
    /// Feature Level 9.3 (max texture size: 4096).
    Fl9_3,
    /// Feature Level 10.0 (max texture size: 8192).
    Fl10_0,
    /// Feature Level 10.1 (max texture size: 8192).
    Fl10_1,
    /// Feature Level 11.0 (max texture size: 16384).
    Fl11_0,
    /// Feature Level 11.1 (max texture size: 16384).
    Fl11_1,
    /// Feature Level 12.0 (max texture size: 16384).
    Fl12_0,
    /// Feature Level 12.1 (max texture size: 16384).
    Fl12_1,
    /// Feature Level 12.2 (max texture size: 16384).
    Fl12_2,
}

impl FeatureLevel {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Fl9_1 => "9.1",
            Self::Fl9_2 => "9.2",
            Self::Fl9_3 => "9.3",
            Self::Fl10_0 => "10.0",
            Self::Fl10_1 => "10.1",
            Self::Fl11_0 => "11.0",
            Self::Fl11_1 => "11.1",
            Self::Fl12_0 => "12.0",
            Self::Fl12_1 => "12.1",
            Self::Fl12_2 => "12.2",
        }
    }
}

/// Enum for recursive mode when processing input files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursiveMode {
    /// Flatten subdirectory structure in output directory.
    Flatten,
    /// Keep subdirectory structure in output directory.
    Keep,
}

/// Builder for constructing a `texconv` command with type-safe options.
///
/// This struct uses the `bon` crate to generate a builder pattern for configuring
/// the `texconv` command-line tool, which processes images for texture conversion,
/// resizing, format conversion, mip-map generation, and block compression.
#[derive(Builder, Debug)]
#[builder(derive(Debug))]
pub struct Texconv {
    /// Path to the `texconv.exe` executable.
    ///
    /// Must point to the Windows `texconv.exe`, typically run via Proton on Linux.
    ///
    /// # Aliases
    /// - `executable`
    /// - `texconv`
    #[builder(start_fn, into)]
    texconv_path: PathBuf,

    /// List of input files to process (e.g., dds, tga, jpg, png).
    ///
    /// Supports formats like dds, tga, hdr, phm, ppm, pfm, or WIC-supported formats
    /// (bmp, jpg, png, jxr, heif, webp, etc.).
    ///
    /// # Aliases
    /// - `files`
    /// - `input`
    #[builder(field)]
    input_files: Vec<PathBuf>,

    /// Block compression flags for BC formats (e.g., BC1, BC7).
    ///
    /// Combine flags like `Uniform`, `Dither`, `Quick`, or `Exhaustive` to control
    /// compression behavior for BC1-BC3 or BC7 formats.
    ///
    /// # Aliases
    /// - `bc`
    /// - `block_compress`
    #[builder(field)]
    bc_flags: Vec<BcFlag>,

    /// Normal map generation flags.
    ///
    /// Combine flags like `Red`, `Green`, `Blue`, `Alpha`, `Luminance`, etc., to
    /// control normal map generation from height maps.
    ///
    /// # Aliases
    /// - `nmap`
    /// - `normal_map`
    #[builder(field)]
    nmap_flags: Vec<NmapFlag>,

    /// Recursive mode for processing files with wildcards.
    ///
    /// Use `Flatten` to ignore subdirectory structure or `Keep` to preserve it when
    /// searching subdirectories with `-r`.
    ///
    /// # Aliases
    /// - `recurse`
    recursive: Option<RecursiveMode>,

    /// Path to a text file containing a list of input files (one per line).
    ///
    /// Lines starting with `#` are treated as comments. Does not support wildcards or
    /// additional arguments.
    ///
    /// # Aliases
    /// - `filelist`
    /// - `input_list`
    #[builder(into)]
    file_list: Option<PathBuf>,

    /// Prefix to attach to the output texture's name.
    ///
    /// # Aliases
    /// - `name_prefix`
    #[builder(into)]
    prefix: Option<String>,

    /// Suffix to attach to the output texture's name.
    ///
    /// # Aliases
    /// - `name_suffix`
    #[builder(into)]
    suffix: Option<String>,

    /// Output directory for processed files.
    ///
    /// # Aliases
    /// - `out_dir`
    /// - `output`
    #[builder(into)]
    output_dir: Option<PathBuf>,

    /// Force output path and filename to lowercase.
    ///
    /// Useful for case-sensitive systems like git, as Windows is case-insensitive.
    ///
    /// # Aliases
    /// - `lowercase`
    #[builder(default)]
    to_lowercase: bool,

    /// Overwrite existing output files.
    ///
    /// By default, texconv skips writing if the output file exists.
    ///
    /// # Aliases
    /// - `force`
    #[builder(default)]
    overwrite: bool,

    /// Output file type (e.g., `Dds`, `Png`, `Jpg`).
    ///
    /// Defaults to `Dds` if not specified.
    ///
    /// # Aliases
    /// - `filetype`
    /// - `output_format`
    file_type: Option<FileType>,

    /// Output DXGI format (e.g., `R10G10B10A2_UNORM`, `DXT1`).
    ///
    /// Supports common aliases like `DXT1` (BC1_UNORM), `DXT5` (BC3_UNORM),
    /// `BGRA` (B8G8R8A8_UNORM), etc.
    ///
    /// # Aliases
    /// - `dxgi_format`
    #[builder(into)]
    format: Option<String>,

    /// Width of the output texture in pixels.
    ///
    /// If not specified, uses the width of the first input image.
    ///
    /// # Aliases
    /// - `w`
    width: Option<u32>,

    /// Height of the output texture in pixels.
    ///
    /// If not specified, uses the height of the first input image.
    ///
    /// # Aliases
    /// - `h`
    height: Option<u32>,

    /// Number of mipmap levels to generate.
    ///
    /// Applies to DDS output; defaults to 0 (generate all mipmaps). Use 1 to disable mipmaps.
    ///
    /// # Aliases
    /// - `mipmaps`
    /// - `mipmap_levels`
    mip_levels: Option<NonZeroU32>,

    /// Fit texture dimensions to power-of-2, minimizing aspect ratio changes.
    ///
    /// Maximum size depends on feature level (defaults to 16384).
    ///
    /// # Aliases
    /// - `power_of_2`
    #[builder(default)]
    fit_power_of_2: bool,

    /// Image filter for resizing.
    ///
    /// Options include `Point`, `Linear`, `Cubic`, etc., with or without dithering.
    ///
    /// # Aliases
    /// - `filter`
    image_filter: Option<ImageFilter>,

    /// Use wrap addressing mode for filtering.
    ///
    /// Defaults to clamp if not specified.
    ///
    /// # Aliases
    /// - `wrap_mode`
    #[builder(default)]
    wrap: bool,

    /// Use mirror addressing mode for filtering.
    ///
    /// Defaults to clamp if not specified.
    ///
    /// # Aliases
    /// - `mirror_mode`
    #[builder(default)]
    mirror: bool,

    /// Force non-WIC-based code paths for filtering.
    ///
    /// Useful to avoid WIC issues on certain operating systems.
    ///
    /// # Aliases
    /// - `disable_wic`
    #[builder(default)]
    no_wic: bool,

    /// Use sRGB for both input and output (gamma ~2.2).
    ///
    /// # Aliases
    /// - `srgb_full`
    #[builder(default)]
    srgb: bool,

    /// Input is in sRGB format.
    ///
    /// # Aliases
    /// - `srgb_input`
    #[builder(default)]
    srgb_in: bool,

    /// Output is in sRGB format.
    ///
    /// # Aliases
    /// - `srgb_output`
    #[builder(default)]
    srgb_out: bool,

    /// Color rotation for HDR10 or other colorspace conversions.
    ///
    /// Options like `Rec709To2020`, `Rec709ToHdr10`, etc.
    ///
    /// # Aliases
    /// - `colorspace`
    rotate_color: Option<RotateColor>,

    /// Paper-white nits value for HDR10 conversions (default: 200.0, max: 10000).
    ///
    /// # Aliases
    /// - `nits`
    paper_white_nits: Option<f32>,

    /// Apply Reinhard tonemap operator to adjust HDR to LDR range.
    ///
    /// # Aliases
    /// - `tone_map`
    #[builder(default)]
    tonemap: bool,

    /// Convert output to premultiplied alpha.
    ///
    /// Sets DDS_ALPHA_MODE_PREMULTIPLIED unless alpha is fully opaque.
    ///
    /// # Aliases
    /// - `pm_alpha`
    #[builder(default)]
    premultiplied_alpha: bool,

    /// Convert premultiplied alpha to straight (non-premultiplied) alpha.
    ///
    /// # Aliases
    /// - `straight_alpha`
    #[builder(default)]
    straight_alpha: bool,

    /// Separate alpha channel for resize/mipmap generation.
    ///
    /// Implies DDS_ALPHA_MODE_CUSTOM, typically used when alpha isn't transparency.
    ///
    /// # Aliases
    /// - `sep_alpha`
    #[builder(default)]
    separate_alpha: bool,

    /// Alpha threshold for 1-bit alpha formats like BC1 or RGBA5551 (default: 0.5).
    ///
    /// # Aliases
    /// - `alpha_thres`
    alpha_threshold: Option<f32>,

    /// Preserve alpha coverage in generated mipmaps (value: 0 to 1).
    ///
    /// # Aliases
    /// - `coverage`
    keep_coverage: Option<f32>,

    /// Hexadecimal RGB color key (e.g., "0000FF") to replace with alpha 0.0.
    ///
    /// # Aliases
    /// - `chroma_key`
    #[builder(into)]
    color_key: Option<String>,

    /// Disable multi-threading for BC6H/BC7 compression, forcing single-core usage.
    ///
    /// # Aliases
    /// - `single_thread`
    #[builder(default)]
    single_proc: bool,

    /// GPU adapter index for BC6H/BC7 compression (default: 0).
    ///
    /// # Aliases
    /// - `gpu_index`
    gpu: Option<u32>,

    /// Force software codec for BC6H/BC7 compression, disabling GPU.
    ///
    /// # Aliases
    /// - `disable_gpu`
    #[builder(default)]
    no_gpu: bool,

    /// Alpha weighting for BC7 GPU compressor (default: 1.0).
    ///
    /// # Aliases
    /// - `alpha_w`
    alpha_weight: Option<f32>,

    /// Amplitude for normal map generation (default: 1.0).
    ///
    /// # Aliases
    /// - `nmap_amp`
    nmap_amplitude: Option<f32>,

    /// Invert green channel for normal maps (OpenGL vs. Direct3D conventions).
    ///
    /// # Aliases
    /// - `flip_y`
    #[builder(default)]
    invert_y: bool,

    /// Rebuild Z (blue) channel for normal maps, assuming X/Y are normals.
    ///
    /// Useful for converting from BC5 format.
    ///
    /// # Aliases
    /// - `rebuild_z`
    #[builder(default)]
    reconstruct_z: bool,

    /// Enable special *2 -1 conversion for unorm/float and positive-only floats.
    ///
    /// Typically used with normal maps.
    ///
    /// # Aliases
    /// - `x2bias`
    #[builder(default)]
    x2_bias: bool,

    /// Perform horizontal flip of the image.
    ///
    /// # Aliases
    /// - `horizontal_flip`
    #[builder(default)]
    hflip: bool,

    /// Perform vertical flip of the image.
    ///
    /// # Aliases
    /// - `vertical_flip`
    #[builder(default)]
    vflip: bool,

    /// HLSL-style swizzle mask for image channels (e.g., "rgba", "rrra").
    ///
    /// Mask is 1 to 4 characters; "0" sets channel to zero, "1" to max.
    ///
    /// # Aliases
    /// - `channel_swizzle`
    #[builder(into)]
    swizzle: Option<String>,

    /// WIC image quality for encoding (0.0 to 1.0).
    ///
    /// Applies to jpg, tif, heif, and jxr formats.
    ///
    /// # Aliases
    /// - `wic_q`
    wic_quality: Option<f32>,

    /// Enable lossless encoding for WIC images (applies to jxr).
    ///
    /// Ignores `wic_quality` if set.
    ///
    /// # Aliases
    /// - `wic_lossless_mode`
    #[builder(default)]
    wic_lossless: bool,

    /// Enable uncompressed encoding for WIC images (applies to tif, heif).
    ///
    /// Ignores `wic_quality` if set.
    ///
    /// # Aliases
    /// - `wic_uncomp`
    #[builder(default)]
    wic_uncompressed: bool,

    /// Encode multiframe WIC images (e.g., gif, tif).
    ///
    /// By default, only the first frame is written.
    ///
    /// # Aliases
    /// - `multiframe`
    #[builder(default)]
    wic_multiframe: bool,

    /// Treat DDS TYPELESS formats as UNORM.
    ///
    /// # Aliases
    /// - `unorm`
    #[builder(default)]
    typeless_unorm: bool,

    /// Treat DDS TYPELESS formats as FLOAT.
    ///
    /// # Aliases
    /// - `float`
    #[builder(default)]
    typeless_float: bool,

    /// Use DWORD alignment instead of BYTE for DDS files.
    ///
    /// Used for some legacy 24bpp files.
    ///
    /// # Aliases
    /// - `dword_align`
    #[builder(default)]
    dword_alignment: bool,

    /// Tolerate malformed DXTn block compression mipchain tails.
    ///
    /// Copies 4x4 blocks to smaller ones for legacy DDS files.
    ///
    /// # Aliases
    /// - `bad_mip_tails`
    #[builder(default)]
    bad_tails: bool,

    /// Allow loading of malformed or variant DDS header files.
    ///
    /// # Aliases
    /// - `permissive_headers`
    #[builder(default)]
    permissive: bool,

    /// Load only the top-level mipmap.
    ///
    /// Useful for malformed DDS files missing mipmap data.
    ///
    /// # Aliases
    /// - `skip_mips`
    #[builder(default)]
    ignore_mips: bool,

    /// Resize DDS to ensure top-level dimensions are multiples of 4 for BC formats.
    ///
    /// Regenerates mipmaps if needed.
    ///
    /// # Aliases
    /// - `fix_bc`
    #[builder(default)]
    fix_bc_4x4: bool,

    /// Expand L8, A8L8, or L16 DDS formats to 8:8:8:8 or 16:16:16:16.
    ///
    /// Without this, they are converted to 1- or 2-channel formats.
    ///
    /// # Aliases
    /// - `expand_lum`
    #[builder(default)]
    expand_luminance: bool,

    /// Force DDS output to use DX10 header extension.
    ///
    /// Allows alpha mode metadata; may break compatibility with older DDS readers.
    ///
    /// # Aliases
    /// - `dx10_header`
    #[builder(default)]
    dx10: bool,

    /// Force DDS output to use legacy DX9 headers.
    ///
    /// Fails for BC6, BC7, UINT, SINT, or array textures; uses non-sRGB formats.
    ///
    /// # Aliases
    /// - `dx9_header`
    #[builder(default)]
    dx9: bool,

    /// Include TGA 2.0 extension area (gamma, alpha mode, timestamp).
    ///
    /// # Aliases
    /// - `tga_extension`
    #[builder(default)]
    tga20: bool,

    /// Preserve zero alpha channels in TGA files instead of treating as opaque.
    ///
    /// # Aliases
    /// - `tga_alpha`
    #[builder(default)]
    tga_zero_alpha: bool,

    /// Target Direct3D feature level (e.g., `Fl11_0` for 16384 max texture size).
    ///
    /// Defaults to 11.0 if not specified.
    ///
    /// # Aliases
    /// - `feature`
    feature_level: Option<FeatureLevel>,

    /// Suppress the copyright message.
    ///
    /// # Aliases
    /// - `hide_logo`
    #[builder(default)]
    no_logo: bool,

    /// Display compression timing information.
    ///
    /// # Aliases
    /// - `show_timing`
    #[builder(default)]
    timing: bool,
}

impl<S: texconv_builder::State> TexconvBuilder<S> {
    /// Adds an input file to process.
    ///
    /// # Arguments
    /// * `input_file` - Path to an input file (e.g., jpg, png, dds).
    ///
    /// # Aliases
    /// - `add_file`
    /// - `add_input`
    pub fn input_file(mut self, input_file: impl Into<PathBuf>) -> Self {
        self.input_files.push(input_file.into());
        self
    }

    /// Adds a block compression flag (e.g., `Uniform`, `Dither`).
    ///
    /// # Arguments
    /// * `bc_flag` - Block compression flag to add.
    ///
    /// # Aliases
    /// - `add_bc_flag`
    /// - `add_block_compress`
    pub fn bc_flag(mut self, bc_flag: BcFlag) -> Self {
        self.bc_flags.push(bc_flag);
        self
    }
    /// Adds a block compression flag (e.g., `Uniform`, `Dither`).
    ///
    /// # Arguments
    /// * `bc_flag` - Block compression flag to add.
    ///
    /// # Aliases
    /// - `add_bc_flag`
    /// - `add_block_compress`
    pub fn maybe_bc_flag(self, bc_flag: Option<BcFlag>) -> Self {
        match bc_flag {
            Some(f) => self.bc_flag(f),
            None => self,
        }
    }

    /// Adds a normal map generation flag (e.g., `Red`, `Luminance`).
    ///
    /// # Arguments
    /// * `nmap_flag` - Normal map flag to add.
    ///
    /// # Aliases
    /// - `add_nmap_flag`
    /// - `add_normal_map`
    pub fn nmap_flag(mut self, nmap_flag: NmapFlag) -> Self {
        self.nmap_flags.push(nmap_flag);
        self
    }
}

impl Texconv {
    /// Builds a `std::process::Command` for executing `texconv`.
    ///
    /// Constructs the command with all configured options and input files,
    /// ready to be executed or further modified.
    ///
    /// # Returns
    /// A `std::process::Command` instance configured with all `texconv` options.
    ///
    /// # Aliases
    /// - `construct_command`
    /// - `to_command`
    pub fn command(self) -> Command {
        let mut cmd = Command::new(self.texconv_path);

        if let Some(rec) = self.recursive {
            cmd.arg("-r");
            match rec {
                RecursiveMode::Keep => cmd.arg(":keep"),
                RecursiveMode::Flatten => cmd.arg(":flatten"),
            };
        }

        if let Some(fl) = self.file_list {
            cmd.arg("--file-list").arg(fl);
        }

        if let Some(p) = self.prefix {
            cmd.arg("--prefix").arg(p);
        }

        if let Some(s) = self.suffix {
            cmd.arg("--suffix").arg(s);
        }

        if let Some(o) = self.output_dir {
            cmd.arg("-o").arg(o);
        }

        if self.to_lowercase {
            cmd.arg("-l");
        }

        if self.overwrite {
            cmd.arg("-y");
        }

        if let Some(ft) = self.file_type {
            cmd.arg("--file-type").arg(ft.as_str());
        }

        if let Some(f) = self.format {
            cmd.arg("--format").arg(f);
        }

        if let Some(w) = self.width {
            cmd.arg("--width").arg(w.to_string());
        }

        if let Some(h) = self.height {
            cmd.arg("--height").arg(h.to_string());
        }

        if let Some(m) = self.mip_levels {
            cmd.arg("--mip-levels").arg(m.to_string());
        }

        if self.fit_power_of_2 {
            cmd.arg("--fit-power-of-2");
        }

        if let Some(ifilter) = self.image_filter {
            cmd.arg("--image-filter").arg(ifilter.as_str());
        }

        if self.wrap {
            cmd.arg("-wrap");
        }

        if self.mirror {
            cmd.arg("-mirror");
        }

        if self.no_wic {
            cmd.arg("--nowic");
        }

        if self.srgb {
            cmd.arg("-srgb");
        }

        if self.srgb_in {
            cmd.arg("--srgb-in");
        }

        if self.srgb_out {
            cmd.arg("--srgb-out");
        }

        if let Some(rc) = self.rotate_color {
            cmd.arg("--rotate-color").arg(rc.as_str());
        }

        if let Some(n) = self.paper_white_nits {
            cmd.arg("--paper-white-nits").arg(n.to_string());
        }

        if self.tonemap {
            cmd.arg("--tonemap");
        }

        if self.premultiplied_alpha {
            cmd.arg("-pmalpha");
        }

        if self.straight_alpha {
            cmd.arg("-alpha");
        }

        if self.separate_alpha {
            cmd.arg("-sepalpha");
        }

        if let Some(at) = self.alpha_threshold {
            cmd.arg("--alpha-threshold").arg(at.to_string());
        }

        if let Some(kc) = self.keep_coverage {
            cmd.arg("--keep-coverage").arg(kc.to_string());
        }

        if let Some(ck) = self.color_key {
            cmd.arg("-c").arg(ck);
        }

        if self.single_proc {
            cmd.arg("--single-proc");
        }

        if let Some(g) = self.gpu {
            cmd.arg("-gpu").arg(g.to_string());
        }

        if self.no_gpu {
            cmd.arg("-nogpu");
        }

        if !self.bc_flags.is_empty() {
            let flags_str = self
                .bc_flags
                .iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join("");
            cmd.arg("--block-compress").arg(flags_str);
        }

        if let Some(aw) = self.alpha_weight {
            cmd.arg("--alpha-weight").arg(aw.to_string());
        }

        if !self.nmap_flags.is_empty() {
            let flags_str = self
                .nmap_flags
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("");
            cmd.arg("--normal-map").arg(flags_str);
        }

        if let Some(na) = self.nmap_amplitude {
            cmd.arg("--normal-map-amplitude").arg(na.to_string());
        }

        if self.invert_y {
            cmd.arg("--invert-y");
        }

        if self.reconstruct_z {
            cmd.arg("--reconstruct-z");
        }

        if self.x2_bias {
            cmd.arg("--x2-bias");
        }

        if self.hflip {
            cmd.arg("-hflip");
        }

        if self.vflip {
            cmd.arg("-vflip");
        }

        if let Some(sw) = self.swizzle {
            cmd.arg("--swizzle").arg(sw);
        }

        if let Some(wq) = self.wic_quality {
            cmd.arg("--wic-quality").arg(wq.to_string());
        }

        if self.wic_lossless {
            cmd.arg("--wic-lossless");
        }

        if self.wic_uncompressed {
            cmd.arg("--wic-uncompressed");
        }

        if self.wic_multiframe {
            cmd.arg("--wic-multiframe");
        }

        if self.typeless_unorm {
            cmd.arg("--typeless-unorm");
        }

        if self.typeless_float {
            cmd.arg("--typeless-float");
        }

        if self.dword_alignment {
            cmd.arg("--dword-alignment");
        }

        if self.bad_tails {
            cmd.arg("--bad-tails");
        }

        if self.permissive {
            cmd.arg("--permissive");
        }

        if self.ignore_mips {
            cmd.arg("--ignore-mips");
        }

        if self.fix_bc_4x4 {
            cmd.arg("--fix-bc-4x4");
        }

        if self.expand_luminance {
            cmd.arg("-xlum");
        }

        if self.dx10 {
            cmd.arg("-dx10");
        }

        if self.dx9 {
            cmd.arg("-dx9");
        }

        if self.tga20 {
            cmd.arg("-tga20");
        }

        if self.tga_zero_alpha {
            cmd.arg("--tga-zero-alpha");
        }

        if let Some(fl) = self.feature_level {
            cmd.arg("--feature-level").arg(fl.as_str());
        }

        if self.no_logo {
            cmd.arg("-nologo");
        }

        if self.timing {
            cmd.arg("--timing");
        }

        for file in self.input_files {
            cmd.arg(file);
        }

        cmd
    }
}
