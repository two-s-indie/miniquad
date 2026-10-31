#![cfg(not(target_arch = "wasm32"))]

mod support;
use miniquad::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use support::{calls, context, Call};

fn texture(context: &mut GlContext, width: u32, height: u32, format: TextureFormat) -> TextureId {
    let texture = context.new_texture(
        TextureAccess::Static,
        TextureSource::Empty,
        TextureParams {
            width,
            height,
            format,
            ..Default::default()
        },
    );
    calls();
    texture
}

#[test]
fn rejects_short_readback_before_opengl() {
    let (_guard, mut context) = context();
    let texture = texture(&mut context, 2, 2, TextureFormat::RGBA8);
    assert!(catch_unwind(AssertUnwindSafe(
        || context.texture_read_pixels(texture, &mut [0; 4])
    ))
    .is_err());
    assert!(calls().is_empty());
}

#[test]
fn rejects_short_resize_before_changing_dimensions() {
    let (_guard, mut context) = context();
    let texture = texture(&mut context, 2, 2, TextureFormat::RGBA8);
    assert!(catch_unwind(AssertUnwindSafe(|| context.texture_resize(
        texture,
        4,
        4,
        Some(&[0; 4])
    )))
    .is_err());
    assert!(calls().is_empty());
    assert_eq!(context.texture_size(texture), (2, 2));
}

#[test]
fn reads_tightly_packed_rgb_rows_and_restores_alignment() {
    let (_guard, mut context) = context();
    let texture = texture(&mut context, 1, 2, TextureFormat::RGB8);
    context.texture_read_pixels(texture, &mut [0; 6]);
    assert_eq!(
        calls(),
        vec![Call::ReadPixels {
            width: 1,
            height: 2,
            format: gl::GL_RGB,
            kind: gl::GL_UNSIGNED_BYTE,
            alignment: 1
        }]
    );
    let mut alignment = 0;
    unsafe { gl::glGetIntegerv(0x0D05, &mut alignment) };
    assert_eq!(alignment, 4);
}

#[test]
fn rejects_short_array_source_before_opengl() {
    let (_guard, mut context) = context();
    assert!(catch_unwind(AssertUnwindSafe(|| context.new_texture(
        TextureAccess::Static,
        TextureSource::Array(&[&[&[0; 4]]]),
        TextureParams {
            width: 2,
            height: 2,
            ..Default::default()
        }
    )))
    .is_err());
    assert!(calls().is_empty());
}

#[test]
fn uses_each_mipmaps_dimensions_for_the_transfer() {
    let (_guard, mut context) = context();
    context.new_texture(
        TextureAccess::Static,
        TextureSource::Array(&[&[&[0; 16], &[0; 4]]]),
        TextureParams {
            width: 2,
            height: 2,
            ..Default::default()
        },
    );
    assert_eq!(
        calls(),
        vec![
            Call::TexImage {
                width: 2,
                height: 2,
                format: gl::GL_RGBA,
                kind: gl::GL_UNSIGNED_BYTE
            },
            Call::TexImage {
                width: 1,
                height: 1,
                format: gl::GL_RGBA,
                kind: gl::GL_UNSIGNED_BYTE
            },
        ]
    );
}

#[test]
fn rejects_size_that_would_wrap_in_u32_arithmetic() {
    let (_guard, mut context) = context();
    assert!(catch_unwind(AssertUnwindSafe(|| context.new_texture(
        TextureAccess::Static,
        TextureSource::Bytes(&[]),
        TextureParams {
            width: 65536,
            height: 65536,
            ..Default::default()
        }
    )))
    .is_err());
    assert!(calls().is_empty());
}
