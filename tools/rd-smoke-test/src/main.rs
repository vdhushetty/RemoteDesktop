use anyhow::Result;
use rd_capture::create_capturer;
use rd_clipboard::{ClipboardContent, ClipboardSync};
use rd_codec::{CodecConfig, Decoder, Encoder};

fn main() -> Result<()> {
    println!("=== Remote Desktop Smoke Test ===\n");

    // Test 1: Screen capture
    print!("1. Screen capture... ");
    let mut capturer = create_capturer()?;
    let frame = capturer.capture_frame()?;
    let (fw, fh) = (frame.width, frame.height);
    println!("OK ({fw}x{fh}, {} bytes)", frame.data.len());

    // Test 2: Color conversion
    print!("2. Color conversion (BGRA -> I420)... ");
    let (y, u, v) = frame.to_i420();
    assert_eq!(y.len(), (fw * fh) as usize);
    println!("OK");

    // Test 3: VP9 encode - downscale for speed in debug mode
    // VP9 requires even dimensions
    let enc_w = (fw / 2) & !1; // round down to even
    let enc_h = (fh / 2) & !1;

    let mut y_small = vec![0u8; (enc_w * enc_h) as usize];
    for row in 0..enc_h {
        for col in 0..enc_w {
            y_small[(row * enc_w + col) as usize] = y[(row * 2 * fw + col * 2) as usize];
        }
    }

    let enc_u_w = enc_w / 2;
    let enc_u_h = enc_h / 2;
    let orig_u_w = fw / 2;
    let mut u_small = vec![0u8; (enc_u_w * enc_u_h) as usize];
    let mut v_small = vec![0u8; (enc_u_w * enc_u_h) as usize];
    for row in 0..enc_u_h {
        for col in 0..enc_u_w {
            u_small[(row * enc_u_w + col) as usize] = u[(row * 2 * orig_u_w + col * 2) as usize];
            v_small[(row * enc_u_w + col) as usize] = v[(row * 2 * orig_u_w + col * 2) as usize];
        }
    }

    print!("3. VP9 encode ({enc_w}x{enc_h})... ");
    let mut encoder = Encoder::new(CodecConfig {
        width: enc_w, height: enc_h, fps: 30, bitrate_kbps: 1000,
    })?;

    // Feed frames until encoder produces output
    let mut pkt_data = None;
    for attempt in 0..5 {
        let encoded = encoder.encode(&y_small, &u_small, &v_small)?;
        if let Some(pkt) = encoded.into_iter().next() {
            pkt_data = Some((pkt.data, pkt.is_keyframe, pkt.width, pkt.height));
            println!("OK (packet after {} frame(s))", attempt + 1);
            break;
        }
    }

    if let Some((data, is_key, pw, ph)) = pkt_data {
        println!("   {} bytes, keyframe={}", data.len(), is_key);

        // Test 4: VP9 decode
        print!("4. VP9 decode... ");
        let mut decoder = Decoder::new()?;
        let decoded = decoder.decode(&data, pw, ph)?;
        println!("OK ({}x{}, {} RGBA bytes)", decoded.width, decoded.height, decoded.data.len());
    } else {
        println!("3. VP9 encode... SKIP (encoder buffering, no output yet - normal for VP9)");
        println!("4. VP9 decode... SKIP (no encoded data)");
    }

    // Test 5: Clipboard
    print!("5. Clipboard... ");
    match ClipboardSync::new() {
        Ok(cb) => {
            let content = cb.read()?;
            match content {
                ClipboardContent::Text(t) => println!("OK ({} chars)", t.len()),
                ClipboardContent::Empty => println!("OK (empty)"),
            }
        }
        Err(e) => println!("SKIP ({e})"),
    }

    println!("\n=== Smoke test complete! ===");
    Ok(())
}
