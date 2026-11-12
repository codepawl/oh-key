//! HTTPS through WinHTTP and hashing through CNG, both part of Windows, so the updater and
//! the reporter add no dependency. Every request has timeouts, a total deadline and a size
//! cap, and only `https` links are fetched.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_REDIRECTS,
    WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts,
};
use windows::Win32::Security::Cryptography::{
    BCRYPT_SHA256_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, BCryptHash,
};
use windows::core::{HSTRING, PCWSTR};

const RESOLVE_MILLISECONDS: i32 = 10_000;
const CONNECT_MILLISECONDS: i32 = 10_000;
const SEND_MILLISECONDS: i32 = 30_000;
const RECEIVE_MILLISECONDS: i32 = 30_000;
/// A whole request, however slowly the body trickles in.
const TOTAL_DEADLINE: Duration = Duration::from_secs(120);
const MAX_REDIRECTS: usize = 3;
/// Where a download may be sent on: GitHub and the hosts it serves release files from.
pub const GITHUB_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

pub struct Response {
    pub status: u32,
    /// The link of the last request, after the redirects that were followed.
    pub final_url: String,
    pub body: Vec<u8>,
}

/// A WinHTTP handle that closes itself.
struct Handle(*mut c_void);

impl Handle {
    fn open(raw: *mut c_void, what: &str) -> Result<Handle, String> {
        if raw.is_null() {
            Err(format!(
                "{what}: {}",
                windows::core::Error::from_thread().message()
            ))
        } else {
            Ok(Handle(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// `https://host/path?query` split into host and the path with its query.
pub fn split_https_url(url: &str) -> Result<(String, String), String> {
    let without_scheme = url
        .strip_prefix("https://")
        .ok_or("only https links are fetched")?;
    let (host, path) = match without_scheme.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (without_scheme, "/".to_string()),
    };
    if host.is_empty() || host.contains('@') || host.contains(':') {
        return Err("the link has no plain host name".into());
    }
    Ok((host.to_string(), path))
}

pub fn get(url: &str, max_body_bytes: usize) -> Result<Response, String> {
    request("GET", url, None, max_body_bytes, &GITHUB_HOSTS)
}

/// Follows redirects and returns where it ended, without downloading a body.
pub fn head(url: &str) -> Result<Response, String> {
    request("HEAD", url, None, 0, &GITHUB_HOSTS)
}

/// A report is never sent on to another address: a redirect is an answer, not a detour.
pub fn post_json(url: &str, body: &str) -> Result<Response, String> {
    request("POST", url, Some(body.as_bytes()), 64 * 1024, &[])
}

/// True for a host on the list, written in any case.
pub fn host_is_allowed(host: &str, allowed_hosts: &[&str]) -> bool {
    allowed_hosts
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

/// The next link of a redirect: an absolute https link, or a path on the same host. Nothing
/// else (another scheme, a host off the list) is followed.
pub fn resolve_redirect(
    current_url: &str,
    location: &str,
    allowed_hosts: &[&str],
) -> Result<String, String> {
    let next = if location.starts_with('/') && !location.starts_with("//") {
        let (host, _) = split_https_url(current_url)?;
        format!("https://{host}{location}")
    } else {
        location.to_string()
    };
    let (host, _) = split_https_url(&next)?;
    if !host_is_allowed(&host, allowed_hosts) {
        return Err("the server sent the request to a host that is not allowed".into());
    }
    Ok(next)
}

fn request(
    method: &str,
    url: &str,
    body: Option<&[u8]>,
    max_body_bytes: usize,
    redirect_hosts: &[&str],
) -> Result<Response, String> {
    let mut current_url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let (response, location) = send_once(method, &current_url, body, max_body_bytes)?;
        let Some(location) = location else {
            return Ok(response);
        };
        current_url = resolve_redirect(&current_url, &location, redirect_hosts)?;
    }
    Err("too many redirects".into())
}

/// One request without following redirects. The second value is the redirect target, if any.
fn send_once(
    method: &str,
    url: &str,
    body: Option<&[u8]>,
    max_body_bytes: usize,
) -> Result<(Response, Option<String>), String> {
    let (host, path) = split_https_url(url)?;
    let agent = HSTRING::from(format!("oh-key/{}", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = Handle::open(
            WinHttpOpen(
                &agent,
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "could not start WinHTTP",
        )?;
        require_modern_tls(&session);
        let _ = WinHttpSetTimeouts(
            session.0,
            RESOLVE_MILLISECONDS,
            CONNECT_MILLISECONDS,
            SEND_MILLISECONDS,
            RECEIVE_MILLISECONDS,
        );
        let connection = Handle::open(
            WinHttpConnect(
                session.0,
                &HSTRING::from(host.as_str()),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            ),
            "could not reach the server",
        )?;
        let request = Handle::open(
            WinHttpOpenRequest(
                connection.0,
                &HSTRING::from(method),
                &HSTRING::from(path.as_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "could not open the request",
        )?;
        let redirects_off = WINHTTP_DISABLE_REDIRECTS.to_ne_bytes();
        let _ = WinHttpSetOption(
            Some(request.0.cast_const()),
            WINHTTP_OPTION_DISABLE_FEATURE,
            Some(&redirects_off),
        );
        let response = read_response(&request, url, body, max_body_bytes)?;
        let location = redirect_location(&request, response.status);
        Ok((response, location))
    }
}

/// TLS 1.2 and 1.3 only. Older Windows may refuse the 1.3 flag, so 1.2 alone is the fallback.
unsafe fn require_modern_tls(session: &Handle) {
    let both =
        (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3).to_ne_bytes();
    let only_1_2 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_ne_bytes();
    unsafe {
        let set_both = WinHttpSetOption(
            Some(session.0.cast_const()),
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            Some(&both),
        );
        if set_both.is_err() {
            let _ = WinHttpSetOption(
                Some(session.0.cast_const()),
                WINHTTP_OPTION_SECURE_PROTOCOLS,
                Some(&only_1_2),
            );
        }
    }
}

unsafe fn read_response(
    request: &Handle,
    url: &str,
    body: Option<&[u8]>,
    max_body_bytes: usize,
) -> Result<Response, String> {
    let started = Instant::now();
    let headers: Vec<u16> = if body.is_some() {
        "Content-Type: application/json\r\n"
            .encode_utf16()
            .collect()
    } else {
        "Accept: application/octet-stream, */*\r\n"
            .encode_utf16()
            .collect()
    };
    let body_length = body.map_or(0, |bytes| bytes.len() as u32);
    unsafe {
        WinHttpSendRequest(
            request.0,
            Some(&headers),
            body.map(|bytes| bytes.as_ptr().cast::<c_void>()),
            body_length,
            body_length,
            0,
        )
        .map_err(|error| error.message())?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|error| error.message())?;
        let status = response_status(request)?;
        let mut collected = Vec::new();
        loop {
            if started.elapsed() > TOTAL_DEADLINE {
                return Err("the download took too long".into());
            }
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available)
                .map_err(|error| error.message())?;
            if available == 0 {
                break;
            }
            if collected.len() + available as usize > max_body_bytes {
                return Err("the answer is larger than allowed".into());
            }
            let start = collected.len();
            collected.resize(start + available as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                collected[start..].as_mut_ptr().cast(),
                available,
                &mut read,
            )
            .map_err(|error| error.message())?;
            collected.truncate(start + read as usize);
        }
        Ok(Response {
            status,
            final_url: url.to_string(),
            body: collected,
        })
    }
}

unsafe fn response_status(request: &Handle) -> Result<u32, String> {
    let mut status = 0u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(|error| error.message())?;
    }
    Ok(status)
}

/// The `Location` header of a redirect answer.
unsafe fn redirect_location(request: &Handle, status: u32) -> Option<String> {
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    let mut buffer = [0u16; 2048];
    let mut size = (buffer.len() * 2) as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_LOCATION,
            PCWSTR::null(),
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .ok()?;
    }
    let location = String::from_utf16_lossy(&buffer[..size as usize / 2]);
    Some(location.trim_end_matches('\0').trim().to_string())
}

pub fn sha256_hex(bytes: &[u8]) -> Result<String, String> {
    let mut digest = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut digest) };
    if status.is_err() {
        return Err(format!("hashing failed: {:#x}", status.0));
    }
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Random bytes from the system generator.
pub fn random_bytes<const COUNT: usize>() -> Option<[u8; COUNT]> {
    let mut bytes = [0u8; COUNT];
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    status.is_ok().then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_release_link_into_host_and_path() {
        let (host, path) =
            split_https_url("https://github.com/codepawl/oh-key-releases/releases/latest").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(path, "/codepawl/oh-key-releases/releases/latest");
        assert_eq!(split_https_url("https://example.com").unwrap().1, "/");
    }

    #[test]
    fn only_plain_https_links_are_accepted() {
        assert!(split_https_url("http://github.com/x").is_err());
        assert!(split_https_url("ftp://github.com/x").is_err());
        assert!(split_https_url("https://user@github.com/x").is_err());
        assert!(split_https_url("https://github.com:8443/x").is_err());
        assert!(split_https_url("https:///x").is_err());
    }

    #[test]
    fn a_redirect_stays_on_github_over_https() {
        let current = "https://github.com/codepawl/oh-key-releases/releases/latest";
        let tag = "https://github.com/codepawl/oh-key-releases/releases/tag/v1.0.0";
        assert_eq!(
            resolve_redirect(current, tag, &GITHUB_HOSTS).as_deref(),
            Ok(tag)
        );
        assert_eq!(
            resolve_redirect(current, "/codepawl/x", &GITHUB_HOSTS).as_deref(),
            Ok("https://github.com/codepawl/x")
        );
        assert!(
            resolve_redirect(
                current,
                "https://release-assets.githubusercontent.com/a",
                &GITHUB_HOSTS
            )
            .is_ok()
        );
        assert!(resolve_redirect(current, "http://github.com/x", &GITHUB_HOSTS).is_err());
        assert!(resolve_redirect(current, "https://evil.example/x", &GITHUB_HOSTS).is_err());
        assert!(resolve_redirect(current, "//evil.example/x", &GITHUB_HOSTS).is_err());
        assert!(
            resolve_redirect(current, "https://github.com.evil.example/x", &GITHUB_HOSTS).is_err()
        );
        assert!(resolve_redirect(current, "/x", &[]).is_err());
    }

    #[test]
    fn sha256_of_abc_is_the_known_value() {
        assert_eq!(
            sha256_hex(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn random_bytes_differ_between_calls() {
        let first = random_bytes::<16>().unwrap();
        let second = random_bytes::<16>().unwrap();
        assert_ne!(first, second);
    }
}
