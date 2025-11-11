use {
    anyhow::{Context, Result},
    futures::{FutureExt, TryFuture, TryFutureExt},
    std::future::{Future, ready},
    tap::prelude::*,
    tracing::trace,
};

#[extension_traits::extension(pub(crate) trait FutureAnyhowExt)]
impl<U, T, E> U
where
    Self: Sized,
    E: std::error::Error + Send + Sync + 'static,
    U: TryFuture<Output = std::result::Result<T, E>> + ?Sized,
{
    fn map_context<C>(self, context: C) -> impl Future<Output = Result<T>>
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        self.map(|e| e.context(context))
    }
    fn map_with_context<C, F>(self, context: F) -> impl Future<Output = Result<T>>
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.map(|e| e.with_context(context))
    }
}

async fn json_response<T: serde::de::DeserializeOwned>(text: String) -> Result<T> {
    crate::utils::deserialize_json_with_error_location(&text).tap_err(|_message| {
        #[cfg(test)]
        {
            tracing::error!("dumping to 'failed-response.json': {_message}");
            std::fs::write("failed-response.json", &text)
                .tap_err(|dumping| {
                    tracing::error!("dumping failed: {dumping}");
                })
                .ok();
        }
    })
}

#[extension_traits::extension(pub(crate) trait ReqwestPrettyJsonResponse)]
impl reqwest::Response
where
    Self: Sized,
{
    async fn json_response_ok<T: serde::de::DeserializeOwned, V: FnOnce(&str) -> Result<()>>(self, validate: V) -> Result<T> {
        match self.error_for_status_ref() {
            Ok(_) => Ok(self),
            Err(error) => match self.text().await {
                Ok(error_message) => error_message,
                Err(message) => format!("InvalidResponse<'{message}'>"),
            }
            .pipe(|error_message| Err(error).context(error_message)),
        }
        .pipe(ready)
        .and_then(|response| response.text().map_context("extracting text from response"))
        .inspect_ok(|text| trace!("fetched {} bytes of text ({}...)", text.len(), &text[..(64.min(text.len()))]))
        .and_then(|response| {
            validate(&response)
                .pipe(ready)
                .and_then(|_| json_response::<T>(response))
        })
        .await
    }
}
