use {
    bon::Builder,
    std::{path::PathBuf, process::Command},
};

/// Enum for output file types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Bmp,
    Jpg,
    Jpeg,
    Png,
    Dds,
    Ddx,
    Tga,
    Hdr,
    Tif,
    Tiff,
    Wdp,
    Hdp,
    Jxr,
    Ppm,
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

/// Enum for image filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFilter {
    Point,
    Linear,
    Cubic,
    Fant,
    Box,
    Triangle,
    PointDither,
    LinearDither,
    CubicDither,
    FantDither,
    BoxDither,
    TriangleDither,
    PointDitherDiffusion,
    LinearDitherDiffusion,
    CubicDitherDiffusion,
    FantDitherDiffusion,
    BoxDitherDiffusion,
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

/// Enum for rotate color options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateColor {
    Rec709To2020,
    Rec2020To709,
    Rec709ToHdr10,
    Hdr10ToRec709,
    P3To2020,
    P3ToHdr10,
    Rec709ToDisplayP3,
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

/// Enum for block compression flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BcFlag {
    Uniform,
    Dither,
    Quick,
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

/// Enum for normal map flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmapFlag {
    Red,
    Green,
    Blue,
    Alpha,
    Luminance,
    MirrorUv,
    MirrorU,
    MirrorV,
    InvertSign,
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

/// Enum for feature levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureLevel {
    Fl9_1,
    Fl9_2,
    Fl9_3,
    Fl10_0,
    Fl10_1,
    Fl11_0,
    Fl11_1,
    Fl12_0,
    Fl12_1,
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

/// Enum for recursive mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursiveMode {
    Flatten,
    Keep,
}

/// Builder for constructing a `texconv` command.
#[derive(Builder, Debug)]
pub struct Texconv {
    #[builder(field)]
    input_files: Vec<PathBuf>,
    #[builder(field)]
    bc_flags: Vec<BcFlag>,
    #[builder(field)]
    nmap_flags: Vec<NmapFlag>,
    #[builder(into)]
    texconv_path: PathBuf,
    recursive: Option<RecursiveMode>,
    #[builder(into)]
    file_list: Option<PathBuf>,
    #[builder(into)]
    prefix: Option<String>,
    #[builder(into)]
    suffix: Option<String>,
    #[builder(into)]
    output_dir: Option<PathBuf>,
    #[builder(default)]
    to_lowercase: bool,
    #[builder(default)]
    overwrite: bool,
    file_type: Option<FileType>,
    #[builder(into)]
    format: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    mip_levels: Option<u32>,
    #[builder(default)]
    fit_power_of_2: bool,
    image_filter: Option<ImageFilter>,
    #[builder(default)]
    wrap: bool,
    #[builder(default)]
    mirror: bool,
    #[builder(default)]
    no_wic: bool,
    #[builder(default)]
    srgb: bool,
    #[builder(default)]
    srgb_in: bool,
    #[builder(default)]
    srgb_out: bool,
    rotate_color: Option<RotateColor>,
    paper_white_nits: Option<f32>,
    #[builder(default)]
    tonemap: bool,
    #[builder(default)]
    premultiplied_alpha: bool,
    #[builder(default)]
    straight_alpha: bool,
    #[builder(default)]
    separate_alpha: bool,
    alpha_threshold: Option<f32>,
    keep_coverage: Option<f32>,
    #[builder(into)]
    color_key: Option<String>,
    #[builder(default)]
    single_proc: bool,
    gpu: Option<u32>,
    #[builder(default)]
    no_gpu: bool,
    alpha_weight: Option<f32>,
    nmap_amplitude: Option<f32>,
    #[builder(default)]
    invert_y: bool,
    #[builder(default)]
    reconstruct_z: bool,
    #[builder(default)]
    x2_bias: bool,
    #[builder(default)]
    hflip: bool,
    #[builder(default)]
    vflip: bool,
    #[builder(into)]
    swizzle: Option<String>,
    wic_quality: Option<f32>,
    #[builder(default)]
    wic_lossless: bool,
    #[builder(default)]
    wic_uncompressed: bool,
    #[builder(default)]
    wic_multiframe: bool,
    #[builder(default)]
    typeless_unorm: bool,
    #[builder(default)]
    typeless_float: bool,
    #[builder(default)]
    dword_alignment: bool,
    #[builder(default)]
    bad_tails: bool,
    #[builder(default)]
    permissive: bool,
    #[builder(default)]
    ignore_mips: bool,
    #[builder(default)]
    fix_bc_4x4: bool,
    #[builder(default)]
    expand_luminance: bool,
    #[builder(default)]
    dx10: bool,
    #[builder(default)]
    dx9: bool,
    #[builder(default)]
    tga20: bool,
    #[builder(default)]
    tga_zero_alpha: bool,
    feature_level: Option<FeatureLevel>,
    #[builder(default)]
    no_logo: bool,
    #[builder(default)]
    timing: bool,
}

impl<S: texconv_builder::State> TexconvBuilder<S> {
    pub fn input_file(mut self, input_file: impl Into<PathBuf>) -> Self {
        self.input_files.push(input_file.into());
        self
    }
    pub fn bc_flag(mut self, bc_flag: BcFlag) -> Self {
        self.bc_flags.push(bc_flag);
        self
    }
    pub fn nmap_flag(mut self, nmap_flag: NmapFlag) -> Self {
        self.nmap_flags.push(nmap_flag);
        self
    }
}

impl Texconv {
    /// Builds the `std::process::Command`.
    pub fn build(self) -> Command {
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
