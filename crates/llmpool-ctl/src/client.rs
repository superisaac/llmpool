use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// HTTP client for the LLMPool Admin API
pub struct ApiClient {
    base_url: String,
    token: String,
    client: Client,
}

/// Standard error response from the API
#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

impl ApiClient {
    pub fn new(base_url: String, token: String) -> Self {
        // Strip trailing slash from base_url
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            token,
            client: Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
    }

    /// Send a GET request and deserialize the JSON response
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = self.url(path);
        let resp = self
            .client
            .get(&url)
            .header("x-admin-token", &self.token)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response(resp).await
    }

    /// Send a POST request with a JSON body and deserialize the response
    pub async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, String> {
        let url = self.url(path);
        let resp = self
            .client
            .post(&url)
            .header("x-admin-token", &self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response(resp).await
    }

    /// Send a DELETE request and deserialize the JSON response
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = self.url(path);
        let resp = self
            .client
            .delete(&url)
            .header("x-admin-token", &self.token)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response(resp).await
    }

    /// Send a PUT request with a JSON body and deserialize the response
    pub async fn put<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, String> {
        let url = self.url(path);
        let resp = self
            .client
            .put(&url)
            .header("x-admin-token", &self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response(resp).await
    }

    /// Handle the HTTP response: check status and deserialize
    async fn handle_response<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T, String> {
        let status = resp.status();

        if status.is_success() {
            let text = resp
                .text()
                .await
                .map_err(|e| format!("Failed to read response body: {}", e))?;
            serde_json::from_str::<T>(&text)
                .map_err(|e| format!("Failed to parse response JSON: {}\nBody: {}", e, text))
        } else {
            let text = resp
                .text()
                .await
                .map_err(|e| format!("Failed to read error response body: {}", e))?;

            // Try to parse as API error response
            if let Ok(err_resp) = serde_json::from_str::<ErrorResponse>(&text) {
                Err(format!(
                    "API error ({}): [{}] {}",
                    status, err_resp.error, err_resp.message
                ))
            } else {
                Err(format!("HTTP error ({}): {}", status, text))
            }
        }
    }

    /// Handle the HTTP response and return raw JSON text
    async fn handle_response_raw(
        &self,
        resp: reqwest::Response,
    ) -> Result<String, String> {
        let status = resp.status();

        let text = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        if status.is_success() {
            Ok(text)
        } else {
            // Try to parse as API error response
            if let Ok(err_resp) = serde_json::from_str::<ErrorResponse>(&text) {
                Err(format!(
                    "API error ({}): [{}] {}",
                    status, err_resp.error, err_resp.message
                ))
            } else {
                Err(format!("HTTP error ({}): {}", status, text))
            }
        }
    }

    /// Send a GET request and return raw JSON text
    pub async fn get_raw(&self, path: &str) -> Result<String, String> {
        let url = self.url(path);
        let resp = self
            .client
            .get(&url)
            .header("x-admin-token", &self.token)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response_raw(resp).await
    }

    /// Send a POST request with a JSON body and return raw JSON text
    pub async fn post_raw<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<String, String> {
        let url = self.url(path);
        let resp = self
            .client
            .post(&url)
            .header("x-admin-token", &self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response_raw(resp).await
    }

    /// Send a PUT request with a JSON body and return raw JSON text
    pub async fn put_raw<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<String, String> {
        let url = self.url(path);
        let resp = self
            .client
            .put(&url)
            .header("x-admin-token", &self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response_raw(resp).await
    }

    /// Send a DELETE request and return raw JSON text
    pub async fn delete_raw(&self, path: &str) -> Result<String, String> {
        let url = self.url(path);
        let resp = self
            .client
            .delete(&url)
            .header("x-admin-token", &self.token)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        self.handle_response_raw(resp).await
    }
}
