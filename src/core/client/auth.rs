//! Cookie & crumb acquisition for Yahoo endpoints.

use crate::core::{error::YfError, net::status_error};
use reqwest::{
    RequestBuilder,
    header::{COOKIE, HeaderMap, SET_COOKIE},
};

impl super::YfClient {
    pub(crate) async fn ensure_credentials(&self) -> Result<(), YfError> {
        // Fast path: check if credentials exist with a read lock.
        if self.read_state().crumb.is_some() {
            return Ok(());
        }

        // Slow path: acquire the dedicated fetch lock to ensure only one task proceeds.
        let _guard = self.credential_fetch_lock.lock().await;

        // Double-check: another task might have fetched credentials while this one was waiting.
        if self.read_state().crumb.is_some() {
            return Ok(());
        }

        // With the fetch lock held, we can safely perform the network operations.
        self.get_cookie().await?;
        self.get_crumb_internal().await?;

        Ok(())
    }

    pub(crate) fn clear_crumb(&self) {
        self.write_state().crumb = None;
    }

    pub(crate) fn crumb(&self) -> Option<String> {
        self.read_state().crumb.clone()
    }

    pub(crate) fn with_auth_cookie(&self, req: RequestBuilder) -> RequestBuilder {
        let cookie = self.read_state().cookie.clone();
        match cookie {
            Some(cookie) => req.header(COOKIE, cookie),
            None => req,
        }
    }

    async fn get_cookie(&self) -> Result<(), YfError> {
        let req = self.http.get(self.cookie_url.clone());
        let resp = self.send_with_retry(req, None).await?;
        let cookie = cookie_header(resp.headers())?;

        self.write_state().cookie = Some(cookie);
        Ok(())
    }

    async fn get_crumb_internal(&self) -> Result<(), YfError> {
        let cookie = self
            .read_state()
            .cookie
            .clone()
            .ok_or_else(|| YfError::Auth("Cookie is missing, cannot get crumb".into()))?;

        let url = self.crumb_url.clone();
        let req = self.http.get(url.clone()).header(COOKIE, cookie);
        let resp = self.send_with_retry(req, None).await?;

        if !resp.status().is_success() {
            return Err(status_error(resp.status(), &url));
        }

        let crumb = resp.text().await?;
        let crumb = crumb.trim();

        if crumb.is_empty() || crumb.contains('{') || crumb.contains('<') {
            return Err(YfError::Auth("Received invalid crumb response".into()));
        }

        let crumb = crumb.to_owned();
        self.write_state().crumb = Some(crumb);
        Ok(())
    }
}

fn cookie_header(headers: &HeaderMap) -> Result<String, YfError> {
    let mut cookies = Vec::new();
    let set_cookies = headers.get_all(SET_COOKIE);

    for value in &set_cookies {
        let raw = value
            .to_str()
            .map_err(|_| YfError::Auth("Invalid cookie header format".into()))?;
        let pair = raw.split_once(';').map_or(raw, |(pair, _)| pair).trim();

        if pair.is_empty() || !pair.contains('=') {
            return Err(YfError::Auth("Invalid cookie header format".into()));
        }

        cookies.push(pair.to_owned());
    }

    if cookies.is_empty() {
        return Err(YfError::Auth("No cookie received from fc.yahoo.com".into()));
    }

    Ok(cookies.join("; "))
}
