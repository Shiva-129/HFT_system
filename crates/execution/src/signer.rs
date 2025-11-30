use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub struct BinanceSigner {
    api_key: String,
    secret_key: String,
}

impl BinanceSigner {
    pub fn new(api_key: String, secret_key: String) -> Self {
        Self {
            api_key,
            secret_key,
        }
    }

    pub fn get_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&self.api_key) {
            headers.insert("X-MBX-APIKEY", val);
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/x-www-form-urlencoded"));
        headers
    }

    pub fn sign(&self, query_string: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    /// Helper to sign a query string that might already contain parameters.
    /// Returns (signed_query_string, signature).
    /// Note: This does NOT add the timestamp; the caller must ensure the query string
    /// includes the timestamp before calling this if they want it signed.
    pub fn sign_with_timestamp(&self, query_string: String) -> (String, String) {
        let signature = self.sign(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);
        (signed_query, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_generation() {
        let secret = "secret";
        let query = "symbol=BTCUSDT&side=BUY";
        // Calculated using Python hmac.new(b"secret", b"symbol=BTCUSDT&side=BUY", hashlib.sha256).hexdigest()
        let expected = "83ef3517b61b829b8755e0f6dcff8b6b1c29f47ae72076ecd2aee6237ffbc10f";

        let signer = BinanceSigner::new("dummy_api_key".to_string(), secret.to_string());
        let signature = signer.sign(query);

        assert_eq!(signature, expected);
    }
}
