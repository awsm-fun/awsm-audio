//! Small browser helpers.

use wasm_bindgen::{JsCast, JsValue};

/// Trigger a download of raw `bytes` as `filename` with `mime`. Must be called
/// from (or shortly after) a user gesture.
pub fn download_bytes(filename: &str, bytes: &[u8], mime: &str) -> Result<(), JsValue> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let bag = web_sys::BlobPropertyBag::new();
    bag.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &bag)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;

    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let anchor = document.create_element("a")?;
    anchor.set_attribute("href", &url)?;
    anchor.set_attribute("download", filename)?;
    anchor.unchecked_into::<web_sys::HtmlElement>().click();

    web_sys::Url::revoke_object_url(&url)?;
    Ok(())
}

/// Encode planar f32 channels as a 16-bit PCM WAV file.
pub fn encode_wav(channels: &[Vec<f32>], sample_rate: u32) -> Vec<u8> {
    let num_ch = channels.len().max(1) as u16;
    let frames = channels.iter().map(Vec::len).max().unwrap_or(0);
    let bytes_per_sample = 2usize;
    let block_align = num_ch as usize * bytes_per_sample;
    let data_len = frames * block_align;
    let byte_rate = sample_rate as usize * block_align;

    let mut w = Vec::with_capacity(44 + data_len);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    w.extend_from_slice(b"WAVE");
    w.extend_from_slice(b"fmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&num_ch.to_le_bytes());
    w.extend_from_slice(&sample_rate.to_le_bytes());
    w.extend_from_slice(&(byte_rate as u32).to_le_bytes());
    w.extend_from_slice(&(block_align as u16).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        for ch in channels {
            let s = ch.get(i).copied().unwrap_or(0.0).clamp(-1.0, 1.0);
            let v = (s * 32767.0) as i16;
            w.extend_from_slice(&v.to_le_bytes());
        }
    }
    w
}
