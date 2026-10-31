#![cfg(not(target_arch = "wasm32"))]
mod support;
use miniquad::*;
use support::{calls, context, Call};

#[test]
fn half_float_transfers_use_two_bytes_per_component() {
    let (_guard, mut context) = context();
    let data = [0u8; 8];
    let texture = context.new_texture(
        TextureAccess::Static,
        TextureSource::Bytes(&data),
        TextureParams {
            width: 1,
            height: 1,
            format: TextureFormat::RGBA16F,
            ..Default::default()
        },
    );
    assert_eq!(
        calls(),
        vec![Call::TexImage {
            width: 1,
            height: 1,
            format: gl::GL_RGBA,
            kind: 0x140B
        }]
    );
    context.texture_resize(texture, 1, 1, Some(&data));
    assert_eq!(
        calls(),
        vec![Call::TexImage {
            width: 1,
            height: 1,
            format: gl::GL_RGBA,
            kind: 0x140B
        }]
    );
    context.texture_read_pixels(texture, &mut [0; 8]);
    assert_eq!(
        calls(),
        vec![Call::ReadPixels {
            width: 1,
            height: 1,
            format: gl::GL_RGBA,
            kind: 0x140B,
            alignment: 1
        }]
    );
}
