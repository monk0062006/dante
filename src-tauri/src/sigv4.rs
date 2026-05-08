use hex::ToHex;
use hmac::{Hmac, Mac};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

type HmacSha256 = Hmac<Sha256>;

const AMZ_DATE_FMT: &[FormatItem<'_>] =
    format_description!("[year][month][day]T[hour][minute][second]Z");
const DATE_FMT: &[FormatItem<'_>] = format_description!("[year][month][day]");

const AWS_FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

#[derive(Deserialize)]
pub struct AwsParams {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub service: String,
    pub session_token: Option<String>,
}

pub struct SignedHeaders {
    pub authorization: String,
    pub amz_date: String,
    pub session_token: Option<String>,
    pub host: String,
    pub content_sha256: String,
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.encode_hex()
}

pub fn sign_request(
    params: &AwsParams,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<SignedHeaders, String> {
    sign_request_at(params, method, url, headers, body, OffsetDateTime::now_utc())
}

pub fn sign_request_at(
    params: &AwsParams,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    now: OffsetDateTime,
) -> Result<SignedHeaders, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("url: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "url missing host".to_string())?
        .to_string();

    let amz_date = now.format(AMZ_DATE_FMT).map_err(|e| e.to_string())?;
    let date = now.format(DATE_FMT).map_err(|e| e.to_string())?;

    let path = if parsed.path().is_empty() {
        "/".to_string()
    } else {
        parsed.path().to_string()
    };
    let canonical_uri = canonical_path(&path);

    let canonical_query = canonical_query_string(parsed.query().unwrap_or(""));

    let payload_hash = sha256_hex(body);

    let mut header_map: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in headers {
        let key = k.to_lowercase();
        if key == "host" || key.starts_with("x-amz-") || key == "content-type" {
            header_map.insert(key, v.trim().to_string());
        }
    }
    header_map.insert("host".to_string(), host.clone());
    header_map.insert("x-amz-date".to_string(), amz_date.clone());
    header_map.insert("x-amz-content-sha256".to_string(), payload_hash.clone());
    if let Some(tok) = &params.session_token {
        header_map.insert("x-amz-security-token".to_string(), tok.clone());
    }

    let canonical_headers: String = header_map
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect();
    let signed_headers: String = header_map
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(),
        canonical_uri,
        canonical_query,
        canonical_headers,
        signed_headers,
        payload_hash
    );

    let credential_scope = format!("{}/{}/{}/aws4_request", date, params.region, params.service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        credential_scope,
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac_sha256(format!("AWS4{}", params.secret_key).as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, params.region.as_bytes());
    let k_service = hmac_sha256(&k_region, params.service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hmac_sha256(&k_signing, string_to_sign.as_bytes())
        .encode_hex::<String>();

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        params.access_key, credential_scope, signed_headers, signature
    );

    Ok(SignedHeaders {
        authorization,
        amz_date,
        session_token: params.session_token.clone(),
        host,
        content_sha256: payload_hash,
    })
}

fn canonical_path(path: &str) -> String {
    path.split('/')
        .map(|seg| percent_encode(seg.as_bytes(), AWS_FRAGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn canonical_query_string(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<(String, String)> = vec![];
    for part in query.split('&') {
        let mut split = part.splitn(2, '=');
        let key = split.next().unwrap_or("").to_string();
        let value = split.next().unwrap_or("").to_string();
        pairs.push((
            percent_encode(key.as_bytes(), AWS_FRAGMENT).to_string(),
            percent_encode(value.as_bytes(), AWS_FRAGMENT).to_string(),
        ));
    }
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    // Reference signatures generated from a direct spec implementation in Python
    // (independent reference: hashlib + hmac following AWS SigV4 spec exactly).
    // Any divergence from these = bug in our Rust signer.

    #[test]
    fn signs_get_with_query_matches_reference() {
        let params = AwsParams {
            access_key: "AKIDEXAMPLE".to_string(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            region: "us-east-1".to_string(),
            service: "service".to_string(),
            session_token: None,
        };
        let when = datetime!(2015-08-30 12:36:00 UTC);

        let signed = sign_request_at(
            &params,
            "GET",
            "https://example.amazonaws.com/?Param2=value2&Param1=value1",
            &[],
            b"",
            when,
        )
        .expect("sign should succeed");

        assert_eq!(signed.amz_date, "20150830T123600Z");
        assert_eq!(
            signed.content_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(
            signed.authorization.contains(
                "Signature=311c7f58b10b06de8540bb5a27f441ee0609f1d5ad7b191e68d7ea87d90e3d6b"
            ),
            "authorization mismatch: {}",
            signed.authorization
        );
        assert!(signed
            .authorization
            .contains("Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request"));
        assert!(signed
            .authorization
            .contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"));
    }

    #[test]
    fn signs_post_with_body_matches_reference() {
        let params = AwsParams {
            access_key: "AKIDEXAMPLE".to_string(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            region: "us-west-2".to_string(),
            service: "s3".to_string(),
            session_token: None,
        };
        let when = datetime!(2015-08-30 12:36:00 UTC);

        let signed = sign_request_at(
            &params,
            "POST",
            "https://s3.us-west-2.amazonaws.com/mybucket/key.txt",
            &[("Content-Type".to_string(), "text/plain".to_string())],
            b"hello world",
            when,
        )
        .expect("sign should succeed");

        assert_eq!(
            signed.content_sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert!(
            signed.authorization.contains(
                "Signature=cdc6c910d7cb8378b312c31999e047a6e0e1e349223b581d210eeb43b1a8343d"
            ),
            "authorization mismatch: {}",
            signed.authorization
        );
        // content-type should be in the signed headers since it's in the spec list
        assert!(signed
            .authorization
            .contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date"));
    }

    #[test]
    fn changing_body_changes_signature() {
        let params = AwsParams {
            access_key: "AKIDEXAMPLE".to_string(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            region: "us-east-1".to_string(),
            service: "service".to_string(),
            session_token: None,
        };
        let when = datetime!(2015-08-30 12:36:00 UTC);
        let a = sign_request_at(&params, "POST", "https://x.amazonaws.com/", &[], b"a", when)
            .unwrap();
        let b = sign_request_at(&params, "POST", "https://x.amazonaws.com/", &[], b"b", when)
            .unwrap();
        assert_ne!(a.authorization, b.authorization);
        assert_ne!(a.content_sha256, b.content_sha256);
    }

    #[test]
    fn session_token_is_passed_through() {
        let params = AwsParams {
            access_key: "AKIDEXAMPLE".to_string(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            region: "us-east-1".to_string(),
            service: "service".to_string(),
            session_token: Some("FAKE-SESSION-TOKEN".to_string()),
        };
        let when = datetime!(2015-08-30 12:36:00 UTC);
        let signed = sign_request_at(&params, "GET", "https://x.amazonaws.com/", &[], b"", when)
            .unwrap();
        assert_eq!(signed.session_token.as_deref(), Some("FAKE-SESSION-TOKEN"));
        // session token must be part of signed headers (so server validates it wasn't tampered)
        assert!(signed.authorization.contains("x-amz-security-token"));
    }

    #[test]
    fn determinism_at_fixed_time() {
        let params = AwsParams {
            access_key: "AKIDEXAMPLE".to_string(),
            secret_key: "secret123".to_string(),
            region: "eu-west-1".to_string(),
            service: "lambda".to_string(),
            session_token: None,
        };
        let when = datetime!(2025-01-15 09:00:00 UTC);
        let a = sign_request_at(&params, "GET", "https://lambda.eu-west-1.amazonaws.com/2015-03-31/functions", &[], b"", when).unwrap();
        let b = sign_request_at(&params, "GET", "https://lambda.eu-west-1.amazonaws.com/2015-03-31/functions", &[], b"", when).unwrap();
        assert_eq!(a.authorization, b.authorization);
    }
}
