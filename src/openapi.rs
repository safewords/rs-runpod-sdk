//! `GET /v2/openapi.json`: Runpod's OpenAPI document, which the API overview
//! offers for client generation but does not list among its own paths.

use reqwest::Method;

use crate::client::{Client, Request, Retry};
use crate::error::Error;

impl Client {
    /// Runpod's current OpenAPI document for this API, as JSON. Needs no API
    /// key, so it works on [`Client::anonymous`].
    ///
    /// This crate was generated from a copy of it (`openapi/runpod-v2.json`);
    /// comparing the two is how to tell whether the API has moved on.
    ///
    /// `GET /v2/openapi.json`
    pub async fn openapi_document(&self) -> Result<serde_json::Value, Error> {
        self.execute(Request {
            method: Method::GET,
            path: "/v2/openapi.json".into(),
            query: Vec::new(),
            headers: Vec::new(),
            body: None,
            retry: Retry::Idempotent,
            auth: false,
            timeout: None,
        })
        .await?
        .json()
    }
}
