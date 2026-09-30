use crate::avatars::{Identity, MAX_BYTES, Provider};
use std::{sync::Arc, time::Duration};

pub(super) struct Native {
    client: Arc<whatsapp_rust::Client>,
    http: reqwest::Client,
}
impl Native {
    pub fn new(client: Arc<whatsapp_rust::Client>) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client,
            http: reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .pool_max_idle_per_host(1)
                .build()?,
        })
    }
}
#[async_trait::async_trait]
impl Provider for Native {
    async fn fetch(&self, identity: &Identity) -> Result<Option<Vec<u8>>, String> {
        if super::native::account(&self.client).as_ref() != Some(&identity.account) {
            return Err("Account changed".into());
        }
        let jid = identity
            .jid
            .parse::<whatsapp_rust::Jid>()
            .map_err(|_| "Invalid profile identity")?;
        let photo = self
            .client
            .contacts()
            .get_profile_picture_with_timeout(&jid, true, Some(Duration::from_secs(4)))
            .await
            .map_err(|_| "Profile photo unavailable")?;
        let Some(photo) = photo else {
            return Ok(None);
        };
        let url = reqwest::Url::parse(&photo.url).map_err(|_| "Invalid photo URL")?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || !url
                .host_str()
                .is_some_and(|h| h.ends_with(".whatsapp.net") || h.ends_with(".fbcdn.net"))
        {
            return Err("Invalid photo URL".into());
        }
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| "Profile download unavailable")?;
        if [401, 403, 404, 410].contains(&response.status().as_u16()) {
            return Ok(None);
        }
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|n| n > MAX_BYTES as u64)
        {
            return Err("Profile download unavailable".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Profile download interrupted")?
        {
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err("Profile photo exceeds 1 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        if super::native::account(&self.client).as_ref() != Some(&identity.account) {
            return Err("Account changed".into());
        }
        Ok(Some(bytes))
    }
}
