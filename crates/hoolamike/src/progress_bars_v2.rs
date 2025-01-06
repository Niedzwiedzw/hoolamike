pub mod hooks;
pub use hooks::{read::ReadHookExt, write::WriteHookExt};
use {hooks::IoHook, indicatif::ProgressStyle, tracing_indicatif::span_ext::IndicatifSpanExt};

pub(crate) fn io_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{span_child_prefix:.bold}▕{bar:.blue}▏({bytes}/{total_bytes} {bytes_per_sec} ETA {eta:.grey} ELAPSED {elapsed:.yellow}) \
         {span_name:.blue}({span_fields:.yellow})",
    )
    .unwrap()
    .progress_chars("█▇▆▅▄▃▂▁  ")
}

pub(crate) fn count_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{span_child_prefix:.bold}▕{bar:.green}▏({pos}/{len} ETA {eta:.grey} ELAPSED {elapsed:.yellow}) {span_name:.green}({span_fields:.yellow})",
    )
    .unwrap()
    .progress_chars("█▇▆▅▄▃▂▁  ")
}

#[extension_traits::extension(pub trait IndicatifWrapIoExt)]
impl tracing::Span {
    fn wrap_read<R: std::io::Read>(self, expected_size: u64, read: R) -> IoHook<R, impl Fn(usize)> {
        self.pb_set_style(&io_progress_style());
        self.pb_set_length(expected_size);
        read.hook_read(move |size| self.pb_inc(size as _))
    }
    fn wrap_write<W: std::io::Write>(self, expected_size: u64, write: W) -> IoHook<W, impl Fn(usize)> {
        self.pb_set_style(&io_progress_style());
        self.pb_set_length(expected_size);
        write.hook_write(move |size| self.pb_inc(size as _))
    }
    fn wrap_async_write<W: tokio::io::AsyncWrite + Unpin>(self, expected_size: u64, write: W) -> IoHook<W, impl Fn(usize)> {
        self.pb_set_style(&io_progress_style());
        self.pb_set_length(expected_size);
        IoHook {
            inner: write,
            callback: move |size| self.pb_inc(size as _),
        }
    }
}
