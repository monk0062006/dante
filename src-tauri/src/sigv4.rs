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
    let parsed = url::Url::parse(url).map_err(|e| format!("url: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "url missing host".to_string())?
        .to_string();

    let now = OffsetDateTime::now_utc();
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
