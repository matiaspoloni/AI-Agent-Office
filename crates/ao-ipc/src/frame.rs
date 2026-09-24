//! Length-prefixed JSON frames: `u32` little-endian byte length + UTF-8 JSON.

use crate::MAX_FRAME_BYTES;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Read, Write};

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

pub fn encode(value: &impl Serialize) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(invalid(format!("frame too large: {} bytes", body.len())));
    }
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn decode<T: DeserializeOwned>(body: &[u8]) -> io::Result<T> {
    serde_json::from_slice(body).map_err(|e| invalid(e.to_string()))
}

pub fn check_len(len: u32) -> io::Result<usize> {
    let len = len as usize;
    if len > MAX_FRAME_BYTES {
        return Err(invalid(format!("frame too large: {len} bytes")));
    }
    Ok(len)
}

pub fn write_frame(w: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    w.write_all(&encode(value)?)?;
    w.flush()
}

pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = check_len(u32::from_le_bytes(len))?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    decode(&body)
}

#[cfg(feature = "server")]
pub async fn read_frame_async<T: DeserializeOwned>(
    r: &mut (impl tokio::io::AsyncRead + Unpin),
) -> io::Result<T> {
    use tokio::io::AsyncReadExt;
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = check_len(u32::from_le_bytes(len))?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    decode(&body)
}

#[cfg(feature = "server")]
pub async fn write_frame_async(
    w: &mut (impl tokio::io::AsyncWrite + Unpin),
    value: &impl Serialize,
) -> io::Result<()> {
    use tokio::io::AsyncWriteExt;
    w.write_all(&encode(value)?).await?;
    w.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HookResponse;

    #[test]
    fn roundtrip_and_limits() {
        let resp = HookResponse {
            stdout: Some("{\"ok\":true}".into()),
            exit_code: 0,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &resp).unwrap();
        let back: HookResponse = read_frame(&mut buf.as_slice()).unwrap();
        assert_eq!(back, resp);

        let mut huge = Vec::new();
        huge.extend_from_slice(&(u32::MAX).to_le_bytes());
        assert!(read_frame::<HookResponse>(&mut huge.as_slice()).is_err());

        let truncated = &buf[..buf.len() - 2];
        assert!(read_frame::<HookResponse>(&mut &truncated[..]).is_err());
    }
}
